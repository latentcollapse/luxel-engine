module LavaAdapter

using Base64
using GeometryBasics: Vec4f
using Lava
using SHA
using Statistics
using Vulkan

export AdapterError,
    LavaBackend,
    backend,
    backend_probe,
    render_probe,
    render_scene

const ADAPTER_REVISION = "wge.lava-adapter/v1"
const LAVA_REVISION = "11c7e31bdf62408d22bf379e9e59510f69d2103e"
const VULKAN_REVISION = "03b4ca2351477ccbb8ee378f512da50f7eec7bac"
const VULKAN_CORE_REVISION = "1d02829e8fa92da430d879db4dd7bf564a872035"

struct AdapterError <: Exception
    code::String
    detail::String
end

Base.showerror(io::IO, error::AdapterError) = print(io, error.code, ": ", error.detail)

struct TerrainResources
    packet_sha256::String
    heights::Lava.LavaArray{Float32, 1}
    slopes::Lava.LavaArray{Float32, 1}
    resolution::Int32
end

struct OverlayResources
    packet_sha256::String
    positions::Lava.LavaArray{Vec4f, 1}
    colors::Lava.LavaArray{Vec4f, 1}
    vertex_count::Int
end

mutable struct LavaBackend{C, Q, PP, TP, OP}
    context::C
    queue::Q
    probe_pipeline::PP
    terrain_pipeline::TP
    overlay_pipeline::OP
    framebuffers::Dict{Tuple{Int, Int}, Lava.LavaFramebuffer}
    terrain_resources::Union{Nothing, TerrainResources}
    overlay_resources::Union{Nothing, OverlayResources}
    upload_bytes::UInt64
    draw_calls::UInt64
    readback_bytes::UInt64
    pipeline_compilations::UInt64
    probe_compiled::Bool
    terrain_compiled::Bool
    overlay_compiled::Bool
end

const BACKEND_REF = Ref{Union{Nothing, LavaBackend}}(nothing)
const BACKEND_LOCK = ReentrantLock()

function _probe_vertex()
    vertex_id = Lava.vertex_index() - Int32(1)
    x = Float32(Int32(vertex_id & Int32(1)) * 4 - 1)
    y = Float32(Int32((vertex_id >> Int32(1)) & Int32(1)) * 4 - 1)
    Lava.set_position!(Vec4f(x, y, 0.5f0, 1.0f0))
    return nothing
end

function _probe_fragment()
    Lava.gfx_output(0, Vec4f(0.0f0, 1.0f0, 0.0f0, 1.0f0))
    return nothing
end

function _terrain_vertex(
    heights::Lava.LavaDeviceArray{Float32, 1},
    slopes::Lava.LavaDeviceArray{Float32, 1},
    resolution::Int32,
    width_m::Float32,
    length_m::Float32,
    span_m::Float32,
    aspect::Float32,
    base_color::Vec4f,
)
    vertex_id = Lava.vertex_index() - Int32(1)
    cells_per_axis = resolution - Int32(1)
    cell_id = div(vertex_id, Int32(6))
    corner = rem(vertex_id, Int32(6))
    cell_x = rem(cell_id, cells_per_axis)
    cell_z = div(cell_id, cells_per_axis)
    right = corner == Int32(1) || corner == Int32(2) || corner == Int32(4)
    far = corner == Int32(2) || corner == Int32(4) || corner == Int32(5)
    sample_x = cell_x + (right ? Int32(1) : Int32(0))
    sample_z = cell_z + (far ? Int32(1) : Int32(0))
    sample_index = sample_z * resolution + sample_x + Int32(1)
    height = heights[sample_index]
    slope = slopes[sample_index]
    normalized_x = Float32(sample_x) / Float32(cells_per_axis)
    normalized_z = Float32(sample_z) / Float32(cells_per_axis)
    world_x = (normalized_x - 0.5f0) * width_m
    world_z = (0.5f0 - normalized_z) * length_m
    ndc_x = world_x / (span_m * aspect * 0.5f0)
    ndc_y = -world_z / (span_m * 0.5f0)
    depth = 0.5f0 - height * 0.001f0
    Lava.set_position!(Vec4f(ndc_x, ndc_y, depth, 1.0f0))
    slope_factor = min(max(slope * 0.8f0, 0.0f0), 1.0f0)
    shade = 1.0f0 - 0.45f0 * slope_factor
    Lava.gfx_output(0, Vec4f(base_color[1] * shade, base_color[2] * shade, base_color[3] * shade, base_color[4]))
    return nothing
end

function _terrain_fragment()
    Lava.gfx_output(0, Lava.gfx_input(Vec4f, 0))
    return nothing
end

function _overlay_vertex(
    positions::Lava.LavaDeviceArray{Vec4f, 1},
    colors::Lava.LavaDeviceArray{Vec4f, 1},
)
    vertex_id = Lava.vertex_index()
    Lava.set_position!(positions[vertex_id])
    Lava.gfx_output(0, colors[vertex_id])
    return nothing
end

function _overlay_fragment()
    Lava.gfx_output(0, Lava.gfx_input(Vec4f, 0))
    return nothing
end

function backend()::LavaBackend
    lock(BACKEND_LOCK) do
        current = BACKEND_REF[]
        current isa LavaBackend && return current

        context = Lava.vk_context()
        probe_pipeline = GraphicsPipeline(
            ;
            vertex=_probe_vertex,
            fragment=_probe_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthOff(),
        )
        terrain_pipeline = GraphicsPipeline(
            ;
            vertex=_terrain_vertex,
            fragment=_terrain_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthOff(),
        )
        overlay_pipeline = GraphicsPipeline(
            ;
            vertex=_overlay_vertex,
            fragment=_overlay_fragment,
            topology=LineList(),
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthOff(),
        )
        created = LavaBackend(
            context,
            context.default_bq,
            probe_pipeline,
            terrain_pipeline,
            overlay_pipeline,
            Dict{Tuple{Int, Int}, Lava.LavaFramebuffer}(),
            nothing,
            nothing,
            UInt64(0),
            UInt64(0),
            UInt64(0),
            UInt64(0),
            false,
            false,
            false,
        )
        BACKEND_REF[] = created
        return created
    end
end

function _validate_dimensions(width_px::Integer, height_px::Integer)
    1 <= width_px <= 8192 || throw(AdapterError("invalid_dimensions", "width is outside the adapter bound"))
    1 <= height_px <= 8192 || throw(AdapterError("invalid_dimensions", "height is outside the adapter bound"))
    return (Int(width_px), Int(height_px))
end

function _framebuffer!(state::LavaBackend, width_px::Int, height_px::Int)::Lava.LavaFramebuffer
    key = (width_px, height_px)
    return get!(state.framebuffers, key) do
        Lava.LavaFramebuffer(
            width_px,
            height_px;
            ctx=state.context,
            depth=false,
            color_format=Vulkan.FORMAT_R32G32B32A32_SFLOAT,
        )
    end
end

function _rgba8(value::NTuple{4, <:Real})::NTuple{4, UInt8}
    return ntuple(index -> UInt8(clamp(round(Int, Float32(value[index]) * 255.0f0), 0, 255)), 4)
end

function _capture_bytes(pixels::AbstractMatrix{<:NTuple{4, <:Real}})::Vector{UInt8}
    bytes = Vector{UInt8}(undef, 4 * length(pixels))
    offset = 1
    for row in axes(pixels, 1), column in axes(pixels, 2)
        rgba = _rgba8(pixels[row, column])
        bytes[offset:(offset + 3)] = collect(rgba)
        offset += 4
    end
    return bytes
end

function _device_uuid(context)
    properties = Vulkan.get_physical_device_properties_2(
        context.physical_device,
        Vulkan.PhysicalDeviceIDProperties,
    )
    return bytes2hex(UInt8[properties.next.device_uuid...])
end

function _physical_properties(context)
    return Vulkan.get_physical_device_properties(context.physical_device)
end

function backend_probe(state::LavaBackend=backend())
    properties = _physical_properties(state.context)
    return (
        schema="wge.lava-backend-probe/v1",
        adapter_revision=ADAPTER_REVISION,
        lava_revision=LAVA_REVISION,
        vulkan_revision=VULKAN_REVISION,
        vulkan_core_revision=VULKAN_CORE_REVISION,
        julia_version=string(VERSION),
        backend_id="lava-vulkan",
        device_name=state.context.device_name,
        device_uuid=_device_uuid(state.context),
        vulkan_api_version=string(properties.api_version),
        hardware_ray_tracing=state.context.rt_pipeline_properties !== nothing,
        persistent_context=true,
        offscreen_raster=state.probe_compiled,
        depth_attachment=false,
        texture_sampling=false,
        readback=state.probe_compiled,
    )
end

function render_probe(
    width_px::Integer=16,
    height_px::Integer=16,
    state::LavaBackend=backend(),
)
    width, height = _validate_dimensions(width_px, height_px)
    framebuffer = _framebuffer!(state, width, height)
    target = OffscreenTarget(framebuffer)
    draw!(state.queue, state.probe_pipeline, target, 3; clear_color=(0.0f0, 0.0f0, 0.0f0, 1.0f0))
    Lava.vk_flush!(state.context)
    pixels = readback_framebuffer(framebuffer)
    capture_bytes = _capture_bytes(pixels)
    state.draw_calls += 1
    state.readback_bytes += length(capture_bytes)
    if !state.probe_compiled
        state.probe_compiled = true
        state.pipeline_compilations += 1
    end
    center = pixels[cld(size(pixels, 1), 2), cld(size(pixels, 2), 2)]
    green_pixels = count(pixel -> pixel[2] > 0.9f0 && pixel[1] < 0.1f0, pixels)
    return (
        schema="wge.lava-render-probe/v1",
        backend_id="lava-vulkan",
        adapter_revision=ADAPTER_REVISION,
        lava_revision=LAVA_REVISION,
        width_px=width,
        height_px=height,
        matrix_size=(size(pixels, 1), size(pixels, 2)),
        center_rgba=collect(center),
        green_pixels=green_pixels,
        capture_sha256="sha256:" * bytes2hex(sha256(capture_bytes)),
        capture_base64=base64encode(capture_bytes),
        telemetry=(
            upload_bytes=0,
            readback_bytes=Int(state.readback_bytes),
            draw_calls=Int(state.draw_calls),
            dispatch_calls=0,
            pipeline_compilations=Int(state.pipeline_compilations),
        ),
    )
end

function _orthographic_span(camera)
    projection = camera.projection
    hasproperty(projection, :span_m) ||
        throw(AdapterError("unsupported_projection", "native terrain path requires orthographic projection"))
    span = Float32(projection.span_m)
    span > 0.0f0 || throw(AdapterError("invalid_projection", "orthographic span must be positive"))
    return span
end

function _terrain_material(packet)
    for material in packet.materials
        material.material_id == packet.terrain.material_id && return material
    end
    throw(AdapterError("provenance", "terrain material is absent from the validated packet"))
end

function _terrain_resources!(state::LavaBackend, packet)
    current = state.terrain_resources
    current !== nothing && current.packet_sha256 == packet.packet_sha256 && return current
    heights = Lava.LavaArray{Float32, 1}(packet.terrain.heights_m; bq=state.queue)
    slopes = Lava.LavaArray{Float32, 1}(packet.terrain.slope_grade; bq=state.queue)
    state.upload_bytes += UInt64(sizeof(Float32) * (length(packet.terrain.heights_m) + length(packet.terrain.slope_grade)))
    created = TerrainResources(packet.packet_sha256, heights, slopes, Int32(packet.terrain.resolution))
    state.terrain_resources = created
    return created
end

function _project_point(camera, terrain, point::NTuple{3, <:Real})::Vec4f
    span = _orthographic_span(camera)
    aspect = Float32(camera.width_px) / Float32(camera.height_px)
    x = Float32(point[1]) / (span * aspect * 0.5f0)
    y = -Float32(point[3]) / (span * 0.5f0)
    depth = 0.45f0 - Float32(point[2]) * 0.001f0
    return Vec4f(x, y, depth, 1.0f0)
end

function _append_segment!(positions::Vector{Vec4f}, colors::Vector{Vec4f}, first::Vec4f, last::Vec4f, color::Vec4f)
    push!(positions, first)
    push!(positions, last)
    push!(colors, color)
    push!(colors, color)
    return nothing
end

function _overlay_color(overlay)::Vec4f
    return Vec4f(overlay.color_rgba...)
end

function _append_overlay!(positions, colors, ::Val{:point}, overlay, camera, terrain)
    center = overlay.position_xyz_m
    radius = overlay.radius_m
    color = _overlay_color(overlay)
    _append_segment!(
        positions,
        colors,
        _project_point(camera, terrain, (center[1] - radius, center[2], center[3])),
        _project_point(camera, terrain, (center[1] + radius, center[2], center[3])),
        color,
    )
    _append_segment!(
        positions,
        colors,
        _project_point(camera, terrain, (center[1], center[2], center[3] - radius)),
        _project_point(camera, terrain, (center[1], center[2], center[3] + radius)),
        color,
    )
    return nothing
end

function _append_overlay!(positions, colors, ::Val{:circle}, overlay, camera, terrain)
    center = overlay.center_xyz_m
    color = _overlay_color(overlay)
    steps = 16
    for index in 0:(steps - 1)
        first_angle = 2.0f0 * Float32(pi) * Float32(index) / Float32(steps)
        last_angle = 2.0f0 * Float32(pi) * Float32(index + 1) / Float32(steps)
        first = (center[1] + overlay.radius_m * cos(first_angle), center[2], center[3] + overlay.radius_m * sin(first_angle))
        last = (center[1] + overlay.radius_m * cos(last_angle), center[2], center[3] + overlay.radius_m * sin(last_angle))
        _append_segment!(positions, colors, _project_point(camera, terrain, first), _project_point(camera, terrain, last), color)
    end
    return nothing
end

function _append_overlay!(positions, colors, ::Val{:polyline}, overlay, camera, terrain)
    color = _overlay_color(overlay)
    for index in 1:(length(overlay.points_xyz_m) - 1)
        first = _project_point(camera, terrain, overlay.points_xyz_m[index])
        last = _project_point(camera, terrain, overlay.points_xyz_m[index + 1])
        _append_segment!(positions, colors, first, last, color)
    end
    return nothing
end

function _append_overlay!(positions, colors, overlay, camera, terrain)
    if hasproperty(overlay, :position_xyz_m)
        return _append_overlay!(positions, colors, Val(:point), overlay, camera, terrain)
    elseif hasproperty(overlay, :center_xyz_m)
        return _append_overlay!(positions, colors, Val(:circle), overlay, camera, terrain)
    elseif hasproperty(overlay, :points_xyz_m)
        return _append_overlay!(positions, colors, Val(:polyline), overlay, camera, terrain)
    end
    throw(AdapterError("unsupported_overlay", "validated overlay has no renderable geometry"))
end

function _overlay_resources!(state::LavaBackend, packet)
    current = state.overlay_resources
    current !== nothing && current.packet_sha256 == packet.packet_sha256 && return current
    positions = Vec4f[]
    colors = Vec4f[]
    for overlay in packet.overlays
        _append_overlay!(positions, colors, overlay, packet.camera, packet.terrain)
    end
    isempty(positions) && return nothing
    gpu_positions = Lava.LavaArray{Vec4f, 1}(positions; bq=state.queue)
    gpu_colors = Lava.LavaArray{Vec4f, 1}(colors; bq=state.queue)
    state.upload_bytes += UInt64(sizeof(Vec4f) * (length(positions) + length(colors)))
    created = OverlayResources(packet.packet_sha256, gpu_positions, gpu_colors, length(positions))
    state.overlay_resources = created
    return created
end

function _pixel_luminance(pixel::NTuple{4, <:Real})
    return 0.2126 * Float64(pixel[1]) + 0.7152 * Float64(pixel[2]) + 0.0722 * Float64(pixel[3])
end

function _distinct_colors(pixels)
    return length(Set((rgba[1], rgba[2], rgba[3]) for rgba in (_rgba8(pixel) for pixel in pixels)))
end

function _role_pixels(pixels, overlays, role::Symbol)
    colors = Set{NTuple{4, UInt8}}()
    for overlay in overlays
        overlay.role == role && push!(colors, _rgba8(overlay.color_rgba))
    end
    isempty(colors) && return 0
    return count(pixel -> _rgba8(pixel) in colors, pixels)
end

function _scene_measurements(pixels, packet)
    luminance = Float64[_pixel_luminance(pixel) for pixel in pixels]
    return (
        terrain_luminance_stddev=std(luminance; corrected=false),
        distinct_terrain_colors=_distinct_colors(pixels),
        route_visible_pixels=_role_pixels(pixels, packet.overlays, :route),
        player_spawn_visible_pixels=_role_pixels(pixels, packet.overlays, :player_spawn),
        opponent_spawn_visible_pixels=_role_pixels(pixels, packet.overlays, :opponent_spawn),
        encounter_visible_pixels=_role_pixels(pixels, packet.overlays, :encounter),
        objective_visible_pixels=_role_pixels(pixels, packet.overlays, :objective),
    )
end

function render_scene(packet, state::LavaBackend=backend())
    width, height = _validate_dimensions(packet.width_px, packet.height_px)
    resources = _terrain_resources!(state, packet)
    overlay_resources = _overlay_resources!(state, packet)
    framebuffer = _framebuffer!(state, width, height)
    target = OffscreenTarget(framebuffer)
    material = _terrain_material(packet)
    span = _orthographic_span(packet.camera)
    aspect = Float32(width) / Float32(height)
    terrain_vertices = 6 * (packet.terrain.resolution - 1)^2
    draw!(
        state.queue,
        state.terrain_pipeline,
        target,
        terrain_vertices;
        args=(
            resources.heights,
            resources.slopes,
            resources.resolution,
            Float32(packet.terrain.width_m),
            Float32(packet.terrain.length_m),
            span,
            aspect,
            Vec4f(material.base_color_rgba...),
        ),
        clear_color=(0.035f0, 0.045f0, 0.06f0, 1.0f0),
    )
    state.draw_calls += 1
    if !state.terrain_compiled
        state.terrain_compiled = true
        state.pipeline_compilations += 1
    end
    if overlay_resources !== nothing
        draw!(
            state.queue,
            state.overlay_pipeline,
            target,
            overlay_resources.vertex_count;
            args=(overlay_resources.positions, overlay_resources.colors),
            clear_color=nothing,
        )
        state.draw_calls += 1
        if !state.overlay_compiled
            state.overlay_compiled = true
            state.pipeline_compilations += 1
        end
    end
    Lava.vk_flush!(state.context)
    pixels = readback_framebuffer(framebuffer)
    capture_bytes = _capture_bytes(pixels)
    state.readback_bytes += length(capture_bytes)
    return (
        schema="wge.lava-frame/v1",
        backend_id="lava-vulkan",
        adapter_revision=ADAPTER_REVISION,
        lava_revision=LAVA_REVISION,
        packet_sha256=packet.packet_sha256,
        capture_id=packet.capture_id,
        width_px=width,
        height_px=height,
        capture_sha256="sha256:" * bytes2hex(sha256(capture_bytes)),
        capture_base64=base64encode(capture_bytes),
        measurements=_scene_measurements(pixels, packet),
        telemetry=(
            upload_bytes=Int(state.upload_bytes),
            readback_bytes=Int(state.readback_bytes),
            draw_calls=Int(state.draw_calls),
            dispatch_calls=0,
            pipeline_compilations=Int(state.pipeline_compilations),
        ),
    )
end

end
