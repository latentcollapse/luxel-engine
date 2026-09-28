module LavaAdapter

using Base64
using GeometryBasics: Vec2f, Vec4f
using Lava
using SHA
using Statistics
using Vulkan
import WGEGraphics

export AdapterError,
    LavaBackend,
    backend,
    backend_probe,
    backend_ready,
    render_probe,
    render_depth_probe,
    render_texture_probe,
    render_scene

const ADAPTER_REVISION = "wge.lava-adapter/v1"
const LAVA_REVISION = "11c7e31bdf62408d22bf379e9e59510f69d2103e"
const VULKAN_REVISION = "03b4ca2351477ccbb8ee378f512da50f7eec7bac"
const VULKAN_CORE_REVISION = "1d02829e8fa92da430d879db4dd7bf564a872035"
const MAX_CAPTURE_BYTES = 32 * 1024 * 1024

struct AdapterError <: Exception
    code::String
    detail::String
end

Base.showerror(io::IO, error::AdapterError) = print(io, error.code, ": ", error.detail)

struct TerrainResources
    packet_sha256::String
    heights::Lava.LavaArray{Float32, 1}
    slopes::Lava.LavaArray{Float32, 1}
    regions::Lava.LavaArray{UInt8, 1}
    resolution::Int32
end

struct OverlayResources
    packet_sha256::String
    positions::Lava.LavaArray{Vec4f, 1}
    colors::Lava.LavaArray{Vec4f, 1}
    vertex_count::Int
end

struct MeshResources
    packet_sha256::String
    positions::Lava.LavaArray{Vec4f, 1}
    normals::Lava.LavaArray{Vec4f, 1}
    colors::Lava.LavaArray{Vec4f, 1}
    material_parameters::Lava.LavaArray{Vec4f, 1}
    world_positions::Lava.LavaArray{Vec4f, 1}
    vertex_count::Int
    instance_count::Int
    visible_instance_count::Int
    culled_instance_count::Int
end

struct TextureProbeResources
    texture::Lava.LavaTexture2D{NTuple{4, Float32}}
    sampler::Lava.LavaSampler
    bindings::Lava.TextureBindings
end

struct MaterialTextureResources
    packet_sha256::String
    texture::Lava.LavaTexture2D{NTuple{4, Float32}}
    sampler::Lava.LavaSampler
    bindings::Lava.TextureBindings
end

mutable struct LavaBackend{C, Q, PP, SP, TP, OP, MP, TXP, DP}
    context::C
    queue::Q
    probe_pipeline::PP
    sky_pipeline::SP
    terrain_pipeline::TP
    overlay_pipeline::OP
    mesh_pipeline::MP
    texture_pipeline::TXP
    depth_pipeline::DP
    framebuffers::Dict{Tuple{Int, Int, Bool}, Lava.LavaFramebuffer}
    terrain_resources::Union{Nothing, TerrainResources}
    overlay_resources::Union{Nothing, OverlayResources}
    mesh_resources::Union{Nothing, MeshResources}
    texture_resources::Union{Nothing, TextureProbeResources}
    material_texture_resources::Union{Nothing, MaterialTextureResources}
    upload_bytes::UInt64
    draw_calls::UInt64
    readback_bytes::UInt64
    pipeline_compilations::UInt64
    probe_compiled::Bool
    terrain_compiled::Bool
    overlay_compiled::Bool
    sky_compiled::Bool
    mesh_compiled::Bool
    texture_compiled::Bool
    depth_compiled::Bool
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

function _sky_vertex(sky_top::Vec4f, sky_horizon::Vec4f)
    vertex_id = Lava.vertex_index() - Int32(1)
    x = Float32(Int32(vertex_id & Int32(1)) * 4 - 1)
    y = Float32(Int32((vertex_id >> Int32(1)) & Int32(1)) * 4 - 1)
    Lava.set_position!(Vec4f(x, y, 0.99f0, 1.0f0))
    sky_weight = clamp((y + 1.0f0) * 0.5f0, 0.0f0, 1.0f0)
    horizon_weight = 1.0f0 - sky_weight
    Lava.gfx_output(
        0,
        Vec4f(
            sky_horizon[1] * horizon_weight + sky_top[1] * sky_weight,
            sky_horizon[2] * horizon_weight + sky_top[2] * sky_weight,
            sky_horizon[3] * horizon_weight + sky_top[3] * sky_weight,
            1.0f0,
        ),
    )
    return nothing
end

function _sky_fragment(
    fog_color::Vec4f,
    exposure::Float32,
)
    sky = Lava.gfx_input(Vec4f, 0)
    exposure_factor = max(exposure, 0.01f0)
    atmosphere = 0.08f0 * exposure_factor
    Lava.gfx_output(
        0,
        Vec4f(
            min(sky[1] + fog_color[1] * atmosphere, 1.0f0),
            min(sky[2] + fog_color[2] * atmosphere, 1.0f0),
            min(sky[3] + fog_color[3] * atmosphere, 1.0f0),
            1.0f0,
        ),
    )
    return nothing
end

function _texture_probe_vertex()
    vertex_id = Lava.vertex_index() - Int32(1)
    x = Float32(Int32(vertex_id & Int32(1)) * 4 - 1)
    y = Float32(Int32((vertex_id >> Int32(1)) & Int32(1)) * 4 - 1)
    Lava.set_position!(Vec4f(x, y, 0.5f0, 1.0f0))
    return nothing
end

function _texture_probe_fragment()
    red = Lava.sample_texture_2d(UInt32(0), 0.5f0, 0.5f0, UInt32(0))
    green = Lava.sample_texture_2d(UInt32(0), 0.5f0, 0.5f0, UInt32(1))
    blue = Lava.sample_texture_2d(UInt32(0), 0.5f0, 0.5f0, UInt32(2))
    alpha = Lava.sample_texture_2d(UInt32(0), 0.5f0, 0.5f0, UInt32(3))
    Lava.gfx_output(0, Vec4f(red, green, blue, alpha))
    return nothing
end

function _depth_probe_vertex(color::Vec4f, depth::Float32)
    vertex_id = Lava.vertex_index() - Int32(1)
    x = Float32(Int32(vertex_id & Int32(1)) * 4 - 1)
    y = Float32(Int32((vertex_id >> Int32(1)) & Int32(1)) * 4 - 1)
    Lava.set_position!(Vec4f(x, y, depth, 1.0f0))
    Lava.gfx_output(0, color)
    return nothing
end

function _depth_probe_fragment()
    Lava.gfx_output(0, Lava.gfx_input(Vec4f, 0))
    return nothing
end

function _normalize_vector(vector::Vec4f)
    length_squared = vector[1] * vector[1] + vector[2] * vector[2] + vector[3] * vector[3]
    inverse_length = inv(sqrt(max(length_squared, 1.0f-8)))
    return Vec4f(
        vector[1] * inverse_length,
        vector[2] * inverse_length,
        vector[3] * inverse_length,
        0.0f0,
    )
end

function _material_response(
    base_color::Vec4f,
    normal::Vec4f,
    light_direction::Vec4f,
    light_color::Vec4f,
    light_intensity::Float32,
    ambient::Float32,
    metallic::Float32,
    roughness::Float32,
)
    surface_normal = _normalize_vector(normal)
    light_vector = _normalize_vector(
        Vec4f(-light_direction[1], -light_direction[2], -light_direction[3], 0.0f0),
    )
    view_vector = Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0)
    half_vector = _normalize_vector(
        Vec4f(
            light_vector[1] + view_vector[1],
            light_vector[2] + view_vector[2],
            light_vector[3] + view_vector[3],
            0.0f0,
        ),
    )
    normal_light = max(
        surface_normal[1] * light_vector[1] +
        surface_normal[2] * light_vector[2] +
        surface_normal[3] * light_vector[3],
        0.0f0,
    )
    normal_half = max(
        surface_normal[1] * half_vector[1] +
        surface_normal[2] * half_vector[2] +
        surface_normal[3] * half_vector[3],
        0.0f0,
    )
    half_squared = normal_half * normal_half
    highlight = half_squared * half_squared
    metalness = clamp(metallic, 0.0f0, 1.0f0)
    surface_roughness = clamp(roughness, 0.0f0, 1.0f0)
    diffuse_factor = (1.0f0 - metalness) * (ambient + light_intensity * normal_light)
    specular_factor =
        (0.04f0 + 0.96f0 * metalness) *
        (1.0f0 - 0.8f0 * surface_roughness) *
        light_intensity *
        highlight
    return Vec4f(
        min(
            max(
                base_color[1] * light_color[1] * diffuse_factor +
                light_color[1] * specular_factor,
                0.0f0,
            ),
            1.0f0,
        ),
        min(
            max(
                base_color[2] * light_color[2] * diffuse_factor +
                light_color[2] * specular_factor,
                0.0f0,
            ),
            1.0f0,
        ),
        min(
            max(
                base_color[3] * light_color[3] * diffuse_factor +
                light_color[3] * specular_factor,
                0.0f0,
            ),
            1.0f0,
        ),
        base_color[4],
    )
end

function _apply_fog(
    color::Vec4f,
    fog_color::Vec4f,
    distance::Float32,
    density::Float32,
)
    fog_weight = clamp(distance * density, 0.0f0, 0.92f0)
    clear_weight = 1.0f0 - fog_weight
    return Vec4f(
        color[1] * clear_weight + fog_color[1] * fog_weight,
        color[2] * clear_weight + fog_color[2] * fog_weight,
        color[3] * clear_weight + fog_color[3] * fog_weight,
        color[4],
    )
end

function _terrain_vertex(
    heights::Lava.LavaDeviceArray{Float32, 1},
    slopes::Lava.LavaDeviceArray{Float32, 1},
    regions::Lava.LavaDeviceArray{UInt8, 1},
    resolution::Int32,
    width_m::Float32,
    length_m::Float32,
    span_m::Float32,
    aspect::Float32,
    base_color::Vec4f,
    light_direction::Vec4f,
    light_color::Vec4f,
    light_intensity::Float32,
    ambient::Float32,
    metallic::Float32,
    roughness::Float32,
    fog_color::Vec4f,
    fog_density::Float32,
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
    region = regions[sample_index]
    normalized_x = Float32(sample_x) / Float32(cells_per_axis)
    normalized_z = Float32(sample_z) / Float32(cells_per_axis)
    world_x = (normalized_x - 0.5f0) * width_m
    world_z = (0.5f0 - normalized_z) * length_m
    ndc_x = world_x / (span_m * aspect * 0.5f0)
    ndc_y = -world_z / (span_m * 0.5f0)
    depth = 0.5f0 - height * 0.001f0
    Lava.set_position!(Vec4f(ndc_x, ndc_y, depth, 1.0f0))
    slope_factor = min(max(slope * 0.8f0, 0.0f0), 1.0f0)
    region_tint = 0.82f0 + min(Float32(region) * 0.015f0, 0.18f0)
    shade = (1.0f0 - 0.45f0 * slope_factor) * region_tint
    shaded_base = Vec4f(
        base_color[1] * shade,
        base_color[2] * shade,
        base_color[3] * shade,
        base_color[4],
    )
    lit_color = _material_response(
        shaded_base,
        Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0),
        light_direction,
        light_color,
        light_intensity,
        ambient,
        metallic,
        roughness,
    )
    Lava.gfx_output(
        0,
        _apply_fog(lit_color, fog_color, sqrt(world_x * world_x + world_z * world_z), fog_density),
    )
    Lava.gfx_output(1, Vec2f(normalized_x, normalized_z))
    return nothing
end

function _mesh_vertex(
    positions::Lava.LavaDeviceArray{Vec4f, 1},
    normals::Lava.LavaDeviceArray{Vec4f, 1},
    colors::Lava.LavaDeviceArray{Vec4f, 1},
    material_parameters::Lava.LavaDeviceArray{Vec4f, 1},
    world_positions::Lava.LavaDeviceArray{Vec4f, 1},
    light_direction::Vec4f,
    light_color::Vec4f,
    light_intensity::Float32,
    ambient::Float32,
    fog_color::Vec4f,
    fog_density::Float32,
)
    vertex_id = Lava.vertex_index()
    position = positions[vertex_id]
    normal = normals[vertex_id]
    base_color = colors[vertex_id]
    material = material_parameters[vertex_id]
    world_position = world_positions[vertex_id]
    Lava.set_position!(position)
    lit_color = _material_response(
        base_color,
        normal,
        light_direction,
        light_color,
        light_intensity,
        ambient,
        material[1],
        material[2],
    )
    Lava.gfx_output(
        0,
        _apply_fog(
            lit_color,
            fog_color,
            sqrt(world_position[1] * world_position[1] + world_position[3] * world_position[3]),
            fog_density,
        ),
    )
    Lava.gfx_output(
        1,
        Vec2f(
            clamp(world_position[1] * 0.02f0 + 0.5f0, 0.0f0, 1.0f0),
            clamp(0.5f0 - world_position[3] * 0.02f0, 0.0f0, 1.0f0),
        ),
    )
    return nothing
end

function _terrain_fragment(texture_enabled::Float32)
    color = Lava.gfx_input(Vec4f, 0)
    uv = Lava.gfx_input(Vec2f, 1)
    red = Lava.sample_texture_2d(UInt32(0), uv[1], uv[2], UInt32(0))
    green = Lava.sample_texture_2d(UInt32(0), uv[1], uv[2], UInt32(1))
    blue = Lava.sample_texture_2d(UInt32(0), uv[1], uv[2], UInt32(2))
    alpha = Lava.sample_texture_2d(UInt32(0), uv[1], uv[2], UInt32(3))
    enabled = clamp(texture_enabled, 0.0f0, 1.0f0)
    Lava.gfx_output(
        0,
        Vec4f(
            color[1] * (1.0f0 - enabled + enabled * red),
            color[2] * (1.0f0 - enabled + enabled * green),
            color[3] * (1.0f0 - enabled + enabled * blue),
            color[4] * (1.0f0 - enabled + enabled * alpha),
        ),
    )
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
        sky_pipeline = GraphicsPipeline(
            ;
            vertex=_sky_vertex,
            fragment=_sky_fragment,
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
            depth=DepthLess(),
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
        mesh_pipeline = GraphicsPipeline(
            ;
            vertex=_mesh_vertex,
            fragment=_terrain_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthLess(),
        )
        texture_pipeline = GraphicsPipeline(
            ;
            vertex=_texture_probe_vertex,
            fragment=_texture_probe_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthOff(),
        )
        depth_pipeline = GraphicsPipeline(
            ;
            vertex=_depth_probe_vertex,
            fragment=_depth_probe_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthLess(),
        )
        created = LavaBackend(
            context,
            context.default_bq,
            probe_pipeline,
            sky_pipeline,
            terrain_pipeline,
            overlay_pipeline,
            mesh_pipeline,
            texture_pipeline,
            depth_pipeline,
            Dict{Tuple{Int, Int, Bool}, Lava.LavaFramebuffer}(),
            nothing,
            nothing,
            nothing,
            nothing,
            nothing,
            UInt64(0),
            UInt64(0),
            UInt64(0),
            UInt64(0),
            false,
            false,
            false,
            false,
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
    width_px * height_px * 4 <= MAX_CAPTURE_BYTES ||
        throw(AdapterError("invalid_dimensions", "capture byte length exceeds the native frame bound"))
    return (Int(width_px), Int(height_px))
end

function _framebuffer!(state::LavaBackend, width_px::Int, height_px::Int, depth::Bool=false)::Lava.LavaFramebuffer
    key = (width_px, height_px, depth)
    return get!(state.framebuffers, key) do
        Lava.LavaFramebuffer(
            width_px,
            height_px;
            ctx=state.context,
            depth=depth,
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
    # Lava readback is indexed as (x, y); the contract stores rows top-to-bottom.
    for row in axes(pixels, 2), column in axes(pixels, 1)
        rgba = _rgba8(pixels[column, row])
        for component in 1:4
            bytes[offset + component - 1] = rgba[component]
        end
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
        depth_attachment=state.depth_compiled,
        texture_sampling=state.texture_compiled,
        readback=state.probe_compiled,
    )
end

function backend_ready(state::LavaBackend=backend())
    properties = _physical_properties(state.context)
    return (
        schema_version="wge.graphics-ready/v1",
        backend_id="lava-vulkan",
        adapter_revision=ADAPTER_REVISION,
        lava_revision=LAVA_REVISION,
        julia_version=string(VERSION),
        vulkan_api_version=string(properties.api_version),
        device_name=state.context.device_name,
        device_uuid=_device_uuid(state.context),
        features=(
            offscreen_raster=state.probe_compiled,
            depth_attachment=state.depth_compiled,
            texture_sampling=state.texture_compiled,
            readback=state.probe_compiled,
            hardware_ray_tracing=state.context.rt_pipeline_properties !== nothing,
        ),
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

function render_depth_probe(state::LavaBackend=backend())
    width, height = (16, 16)
    framebuffer = _framebuffer!(state, width, height, true)
    target = OffscreenTarget(framebuffer)
    draw!(
        state.queue,
        state.depth_pipeline,
        target,
        3;
        args=(Vec4f(0.0f0, 0.0f0, 1.0f0, 1.0f0), 0.3f0),
        clear_color=(0.0f0, 0.0f0, 0.0f0, 1.0f0),
    )
    draw!(
        state.queue,
        state.depth_pipeline,
        target,
        3;
        args=(Vec4f(1.0f0, 0.0f0, 0.0f0, 1.0f0), 0.7f0),
        clear_color=nothing,
        depth_clear=nothing,
    )
    state.draw_calls += 2
    if !state.depth_compiled
        state.depth_compiled = true
        state.pipeline_compilations += 1
    end
    Lava.vk_flush!(state.context)
    pixels = readback_framebuffer(framebuffer)
    center = pixels[cld(size(pixels, 1), 2), cld(size(pixels, 2), 2)]
    return (
        schema="wge.lava-depth-probe/v1",
        width_px=width,
        height_px=height,
        center_rgba=collect(center),
        nearer_fragment_won=center[3] > 0.9f0 && center[1] < 0.1f0,
        depth_attachment=true,
    )
end

function _texture_resources!(state::LavaBackend)
    current = state.texture_resources
    current !== nothing && return current
    texel = (0.2f0, 0.7f0, 0.9f0, 1.0f0)
    data = fill(texel, 4, 4)
    texture = Lava.LavaTexture2D(data; ctx=state.context, filter=:nearest, wrap=:clamp)
    sampler = Lava.LavaSampler(ctx=state.context, filter=:nearest, wrap=:clamp)
    bindings = Lava.bind_textures([texture * sampler])
    created = TextureProbeResources(texture, sampler, bindings)
    state.texture_resources = created
    state.upload_bytes += UInt64(sizeof(texel) * length(data))
    return created
end

function _texture_matrix(bytes::Vector{UInt8}, width::UInt32, height::UInt32)
    width_int = Int(width)
    height_int = Int(height)
    data = Matrix{NTuple{4, Float32}}(undef, height_int, width_int)
    offset = 1
    for row in 1:height_int, column in 1:width_int
        data[row, column] = ntuple(index -> Float32(bytes[offset + index - 1]) / 255.0f0, 4)
        offset += 4
    end
    return data
end

function _material_texture_resources!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
)
    current = state.material_texture_resources
    current !== nothing && current.packet_sha256 == packet.packet_sha256 && return current
    texture_ids = Set(
        texture_id for material in packet.materials for texture_id in material.texture_ids
    )
    length(texture_ids) <= 1 ||
        throw(AdapterError("unsupported_material", "native path currently supports one shared albedo texture"))
    shared_texture_id = isempty(texture_ids) ? nothing : only(texture_ids)
    expected_ids = shared_texture_id === nothing ? String[] : String[shared_texture_id]
    for material in packet.materials
        material.texture_ids == expected_ids ||
            throw(AdapterError(
                "unsupported_material",
                "native path requires every material to use the same albedo texture profile",
            ))
    end

    if isempty(texture_ids)
        bytes = UInt8[255, 255, 255, 255]
        width = UInt32(1)
        height = UInt32(1)
    else
        texture_id = only(texture_ids)
        texture_index = findfirst(texture -> texture.texture_id == texture_id, packet.textures)
        texture_index === nothing &&
            throw(AdapterError("provenance", "material texture is absent from the packet"))
        texture = packet.textures[texture_index]
        texture.color_space in (:srgb, :linear) ||
            throw(AdapterError("unsupported_texture", "native path requires color albedo textures"))
        texture.payload === nothing &&
            throw(AdapterError("unsupported_texture", "native path requires an inline texture payload"))
        bytes = texture.payload::Vector{UInt8}
        width = texture.width_px
        height = texture.height_px
    end
    data = _texture_matrix(bytes, width, height)
    gpu_texture = Lava.LavaTexture2D(data; ctx=state.context, filter=:linear, wrap=:clamp)
    sampler = Lava.LavaSampler(ctx=state.context, filter=:linear, wrap=:clamp)
    bindings = Lava.bind_textures([gpu_texture * sampler])
    created = MaterialTextureResources(packet.packet_sha256, gpu_texture, sampler, bindings)
    state.material_texture_resources = created
    state.upload_bytes += UInt64(length(bytes))
    return created
end

function _material_texture_enabled(material::WGEGraphics.MaterialPacket)::Float32
    length(material.texture_ids) <= 1 ||
        throw(AdapterError("unsupported_material", "native path supports one texture per material"))
    return isempty(material.texture_ids) ? 0.0f0 : 1.0f0
end

function render_texture_probe(state::LavaBackend=backend())
    resource = _texture_resources!(state)
    width, height = (16, 16)
    framebuffer = _framebuffer!(state, width, height, false)
    target = OffscreenTarget(framebuffer)
    draw!(
        state.queue,
        state.texture_pipeline,
        target,
        3;
        descriptor_set_layout=resource.bindings.layout,
        descriptor_set=resource.bindings.set,
        clear_color=(0.0f0, 0.0f0, 0.0f0, 1.0f0),
    )
    state.draw_calls += 1
    if !state.texture_compiled
        state.texture_compiled = true
        state.pipeline_compilations += 1
    end
    Lava.vk_flush!(state.context)
    pixels = readback_framebuffer(framebuffer)
    center = pixels[cld(size(pixels, 1), 2), cld(size(pixels, 2), 2)]
    expected = (0.2f0, 0.7f0, 0.9f0, 1.0f0)
    return (
        schema="wge.lava-texture-probe/v1",
        width_px=width,
        height_px=height,
        center_rgba=collect(center),
        expected_rgba=collect(expected),
        texture_sampled=all(isapprox(center[index], expected[index]; atol=0.05f0) for index in 1:4),
        texture_binding_count=1,
    )
end

function _orthographic_span(projection::WGEGraphics.OrthographicProjection)
    span = projection.span_m
    span > 0.0f0 || throw(AdapterError("invalid_projection", "orthographic span must be positive"))
    return span
end

function _orthographic_span(::WGEGraphics.PerspectiveProjection)
    throw(AdapterError("unsupported_projection", "native terrain path requires orthographic projection"))
end

_orthographic_span(camera::WGEGraphics.CameraPacket) = _orthographic_span(camera.projection)

function _terrain_material(packet::WGEGraphics.GraphicsScenePacket)::WGEGraphics.MaterialPacket
    return _material(packet, packet.terrain.material_id)
end

function _directional_lighting(
    kind::WGEGraphics.DirectionalLightPacket,
    color_rgb::NTuple{3, Float32},
    intensity::Float32,
)
    return (
        direction=Vec4f(kind.direction_xyz..., 0.0f0),
        color=Vec4f(color_rgb..., 1.0f0),
        intensity,
        ambient=0.22f0,
    )
end

function _directional_lighting(
    ::WGEGraphics.PointLightPacket,
    ::NTuple{3, Float32},
    ::Float32,
)
    throw(AdapterError("unsupported_light", "native material path currently requires a directional light"))
end

function _lighting(packet::WGEGraphics.GraphicsScenePacket)
    length(packet.lights) == 1 ||
        throw(AdapterError("unsupported_lighting", "native material path requires exactly one light intent"))
    light = only(packet.lights)
    return _directional_lighting(light.kind, light.color_rgb, light.intensity)
end

function _material(packet::WGEGraphics.GraphicsScenePacket, material_id::String)::WGEGraphics.MaterialPacket
    for material in packet.materials
        if material.material_id == material_id
            material.alpha_mode == :opaque ||
                throw(AdapterError("unsupported_material", "native path requires opaque materials"))
            iszero(material.metallic) ||
                throw(AdapterError("unsupported_material", "native path requires non-metallic materials"))
            return material
        end
    end
    throw(AdapterError("provenance", "mesh references an absent material"))
end

function _rotate_vector(rotation::NTuple{4, Float32}, vector::NTuple{3, Float32})
    x, y, z, w = rotation
    norm_squared = x * x + y * y + z * z + w * w
    norm_squared > eps(Float32) || throw(AdapterError("malformed_packet", "instance rotation is degenerate"))
    inverse_norm = inv(sqrt(norm_squared))
    x *= inverse_norm
    y *= inverse_norm
    z *= inverse_norm
    w *= inverse_norm
    vx, vy, vz = vector
    tx = 2.0f0 * (y * vz - z * vy)
    ty = 2.0f0 * (z * vx - x * vz)
    tz = 2.0f0 * (x * vy - y * vx)
    return (
        vx + w * tx + (y * tz - z * ty),
        vy + w * ty + (z * tx - x * tz),
        vz + w * tz + (x * ty - y * tx),
    )
end

function _transform_position(
    transform::WGEGraphics.TransformPacket,
    position::NTuple{3, Float32},
)
    scaled = ntuple(index -> position[index] * transform.scale_xyz[index], 3)
    rotated = _rotate_vector(transform.rotation_xyzw, scaled)
    return ntuple(index -> rotated[index] + transform.translation_xyz_m[index], 3)
end

function _transform_normal(
    transform::WGEGraphics.TransformPacket,
    normal::NTuple{3, Float32},
)
    return _rotate_vector(transform.rotation_xyzw, normal)
end

function _terrain_resources!(state::LavaBackend, packet::WGEGraphics.GraphicsScenePacket)
    current = state.terrain_resources
    current !== nothing && current.packet_sha256 == packet.packet_sha256 && return current
    heights = Lava.LavaArray{Float32, 1}(packet.terrain.heights_m; bq=state.queue)
    slopes = Lava.LavaArray{Float32, 1}(packet.terrain.slope_grade; bq=state.queue)
    regions = Lava.LavaArray{UInt8, 1}(packet.terrain.region_codes; bq=state.queue)
    state.upload_bytes += UInt64(
        sizeof(Float32) * (length(packet.terrain.heights_m) + length(packet.terrain.slope_grade)) +
        sizeof(UInt8) * length(packet.terrain.region_codes),
    )
    created = TerrainResources(packet.packet_sha256, heights, slopes, regions, Int32(packet.terrain.resolution))
    state.terrain_resources = created
    return created
end

function _project_point(
    camera::WGEGraphics.CameraPacket,
    point::NTuple{3, <:Real},
)::Vec4f
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

function _overlay_color(overlay::WGEGraphics.OverlayPacket)::Vec4f
    return Vec4f(overlay.color_rgba...)
end

function _append_overlay!(
    positions::Vector{Vec4f},
    colors::Vector{Vec4f},
    overlay::WGEGraphics.PointOverlay,
    camera::WGEGraphics.CameraPacket,
)
    center = overlay.position_xyz_m
    radius = overlay.radius_m
    color = _overlay_color(overlay)
    _append_segment!(
        positions,
        colors,
        _project_point(camera, (center[1] - radius, center[2], center[3])),
        _project_point(camera, (center[1] + radius, center[2], center[3])),
        color,
    )
    _append_segment!(
        positions,
        colors,
        _project_point(camera, (center[1], center[2], center[3] - radius)),
        _project_point(camera, (center[1], center[2], center[3] + radius)),
        color,
    )
    return nothing
end

function _append_overlay!(
    positions::Vector{Vec4f},
    colors::Vector{Vec4f},
    overlay::WGEGraphics.CircleOverlay,
    camera::WGEGraphics.CameraPacket,
)
    center = overlay.center_xyz_m
    color = _overlay_color(overlay)
    steps = 16
    for index in 0:(steps - 1)
        first_angle = 2.0f0 * Float32(pi) * Float32(index) / Float32(steps)
        last_angle = 2.0f0 * Float32(pi) * Float32(index + 1) / Float32(steps)
        first = (
            center[1] + overlay.radius_m * cos(first_angle),
            center[2],
            center[3] + overlay.radius_m * sin(first_angle),
        )
        last = (
            center[1] + overlay.radius_m * cos(last_angle),
            center[2],
            center[3] + overlay.radius_m * sin(last_angle),
        )
        _append_segment!(positions, colors, _project_point(camera, first), _project_point(camera, last), color)
    end
    return nothing
end

function _append_overlay!(
    positions::Vector{Vec4f},
    colors::Vector{Vec4f},
    overlay::WGEGraphics.PolylineOverlay,
    camera::WGEGraphics.CameraPacket,
)
    color = _overlay_color(overlay)
    for index in 1:(length(overlay.points_xyz_m) - 1)
        first = _project_point(camera, overlay.points_xyz_m[index])
        last = _project_point(camera, overlay.points_xyz_m[index + 1])
        _append_segment!(positions, colors, first, last, color)
    end
    return nothing
end

function _instance_visible(
    packet::WGEGraphics.GraphicsScenePacket,
    instance::WGEGraphics.InstancePacket,
)
    center = _project_point(packet.camera, instance.transform.translation_xyz_m)
    span = _orthographic_span(packet.camera)
    aspect = Float32(packet.camera.width_px) / Float32(packet.camera.height_px)
    radius = max(instance.transform.scale_xyz...)
    margin_y = radius / (span * 0.5f0)
    margin_x = radius / (span * aspect * 0.5f0)
    return -1.0f0 - margin_x <= center[1] <= 1.0f0 + margin_x &&
           -1.0f0 - margin_y <= center[2] <= 1.0f0 + margin_y
end

function _mesh_visibility(packet::WGEGraphics.GraphicsScenePacket)
    instance_count = length(packet.instances)
    visible_instance_count = count(
        instance -> _instance_visible(packet, instance),
        packet.instances,
    )
    return (
        instance_count=instance_count,
        visible_instance_count=visible_instance_count,
        culled_instance_count=instance_count - visible_instance_count,
    )
end

function _mesh_resources!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    visibility::NamedTuple=_mesh_visibility(packet),
)
    current = state.mesh_resources
    current !== nothing && current.packet_sha256 == packet.packet_sha256 && return current
    positions = Vec4f[]
    normals = Vec4f[]
    colors = Vec4f[]
    material_parameters = Vec4f[]
    world_positions = Vec4f[]
    for instance in packet.instances
        _instance_visible(packet, instance) || continue
        mesh = findfirst(mesh -> mesh.mesh_id == instance.mesh_id, packet.meshes)
        mesh === nothing && throw(AdapterError("provenance", "instance references an absent mesh"))
        mesh_packet = packet.meshes[mesh]
        material = _material(packet, instance.material_id)
        for index in mesh_packet.indices
            vertex = Int(index) + 1
            world_position = _transform_position(instance.transform, mesh_packet.positions_m[vertex])
            world_normal = _transform_normal(instance.transform, mesh_packet.normals[vertex])
            push!(positions, _project_point(packet.camera, world_position))
            push!(normals, Vec4f(world_normal..., 0.0f0))
            push!(colors, Vec4f(material.base_color_rgba...))
            push!(material_parameters, Vec4f(material.metallic, material.roughness, 0.0f0, 0.0f0))
            push!(world_positions, Vec4f(world_position..., 1.0f0))
        end
    end
    isempty(positions) && return nothing
    gpu_positions = Lava.LavaArray{Vec4f, 1}(positions; bq=state.queue)
    gpu_normals = Lava.LavaArray{Vec4f, 1}(normals; bq=state.queue)
    gpu_colors = Lava.LavaArray{Vec4f, 1}(colors; bq=state.queue)
    gpu_material_parameters = Lava.LavaArray{Vec4f, 1}(material_parameters; bq=state.queue)
    gpu_world_positions = Lava.LavaArray{Vec4f, 1}(world_positions; bq=state.queue)
    state.upload_bytes += UInt64(
        sizeof(Vec4f) *
        (length(positions) + length(normals) + length(colors) +
         length(material_parameters) + length(world_positions)),
    )
    created = MeshResources(
        packet.packet_sha256,
        gpu_positions,
        gpu_normals,
        gpu_colors,
        gpu_material_parameters,
        gpu_world_positions,
        length(positions),
        visibility.instance_count,
        visibility.visible_instance_count,
        visibility.culled_instance_count,
    )
    state.mesh_resources = created
    return created
end

function _overlay_resources!(state::LavaBackend, packet::WGEGraphics.GraphicsScenePacket)
    current = state.overlay_resources
    current !== nothing && current.packet_sha256 == packet.packet_sha256 && return current
    positions = Vec4f[]
    colors = Vec4f[]
    for overlay in packet.overlays
        _append_overlay!(positions, colors, overlay, packet.camera)
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

function _scene_measurements(
    pixels::AbstractMatrix{<:NTuple{4, <:Real}},
    packet::WGEGraphics.GraphicsScenePacket,
)
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

function render_scene(
    packet::WGEGraphics.GraphicsScenePacket,
    state::LavaBackend=backend(),
)
    started_ns = time_ns()
    width, height = _validate_dimensions(packet.width_px, packet.height_px)
    resources = _terrain_resources!(state, packet)
    visibility = _mesh_visibility(packet)
    mesh_resources = _mesh_resources!(state, packet, visibility)
    overlay_resources = _overlay_resources!(state, packet)
    framebuffer = _framebuffer!(state, width, height, true)
    target = OffscreenTarget(framebuffer)
    material = _terrain_material(packet)
    texture_resources = _material_texture_resources!(state, packet)
    lighting = _lighting(packet)
    span = _orthographic_span(packet.camera)
    aspect = Float32(width) / Float32(height)
    terrain_vertices = 6 * (packet.terrain.resolution - 1)^2
    draw!(
        state.queue,
        state.sky_pipeline,
        target,
        3;
        args=(
            Vec4f(packet.environment.sky_top_rgb..., 1.0f0),
            Vec4f(packet.environment.sky_horizon_rgb..., 1.0f0),
        ),
        frag_args=(
            Vec4f(packet.environment.fog_color_rgb..., 1.0f0),
            packet.environment.exposure,
        ),
        clear_color=(0.02f0, 0.03f0, 0.05f0, 1.0f0),
    )
    state.draw_calls += 1
    if !state.sky_compiled
        state.sky_compiled = true
        state.pipeline_compilations += 1
    end
    draw!(
        state.queue,
        state.terrain_pipeline,
        target,
        terrain_vertices;
        args=(
            resources.heights,
            resources.slopes,
            resources.regions,
            resources.resolution,
            Float32(packet.terrain.width_m),
            Float32(packet.terrain.length_m),
            span,
            aspect,
            Vec4f(material.base_color_rgba...),
            lighting.direction,
            lighting.color,
            Float32(lighting.intensity),
            lighting.ambient,
            material.metallic,
            material.roughness,
            Vec4f(packet.environment.fog_color_rgb..., 1.0f0),
            packet.environment.fog_density,
        ),
        frag_args=(_material_texture_enabled(material),),
        descriptor_set_layout=texture_resources.bindings.layout,
        descriptor_set=texture_resources.bindings.set,
        clear_color=nothing,
    )
    state.draw_calls += 1
    if !state.terrain_compiled
        state.terrain_compiled = true
        state.pipeline_compilations += 1
    end
    if mesh_resources !== nothing
        draw!(
            state.queue,
            state.mesh_pipeline,
            target,
            mesh_resources.vertex_count;
            args=(
                mesh_resources.positions,
                mesh_resources.normals,
                mesh_resources.colors,
                mesh_resources.material_parameters,
                mesh_resources.world_positions,
                lighting.direction,
                lighting.color,
                Float32(lighting.intensity),
                lighting.ambient,
                Vec4f(packet.environment.fog_color_rgb..., 1.0f0),
                packet.environment.fog_density,
            ),
            frag_args=(_material_texture_enabled(material),),
            descriptor_set_layout=texture_resources.bindings.layout,
            descriptor_set=texture_resources.bindings.set,
            clear_color=nothing,
            depth_clear=nothing,
        )
        state.draw_calls += 1
        if !state.mesh_compiled
            state.mesh_compiled = true
            state.pipeline_compilations += 1
        end
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
            instance_count=visibility.instance_count,
            visible_instance_count=visibility.visible_instance_count,
            culled_instance_count=visibility.culled_instance_count,
            terrain_vertex_count=terrain_vertices,
            mesh_vertex_count=mesh_resources === nothing ? 0 : mesh_resources.vertex_count,
            frame_time_us=Int(cld(time_ns() - started_ns, UInt64(1_000))),
        ),
    )
end

end
