module LavaAdapter

using Base64
using GeometryBasics: Vec4f
using Lava
using SHA
using Vulkan

export AdapterError,
    LavaBackend,
    backend,
    backend_probe,
    render_probe

const ADAPTER_REVISION = "wge.lava-adapter/v1"
const LAVA_REVISION = "11c7e31bdf62408d22bf379e9e59510f69d2103e"
const VULKAN_REVISION = "03b4ca2351477ccbb8ee378f512da50f7eec7bac"
const VULKAN_CORE_REVISION = "1d02829e8fa92da430d879db4dd7bf564a872035"

struct AdapterError <: Exception
    code::String
    detail::String
end

Base.showerror(io::IO, error::AdapterError) = print(io, error.code, ": ", error.detail)

mutable struct LavaBackend{C, Q, P}
    context::C
    queue::Q
    probe_pipeline::P
    framebuffers::Dict{Tuple{Int, Int}, Lava.LavaFramebuffer}
    draw_calls::UInt64
    readback_bytes::UInt64
    pipeline_compilations::UInt64
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

function backend()::LavaBackend
    lock(BACKEND_LOCK) do
        current = BACKEND_REF[]
        current isa LavaBackend && return current

        context = Lava.vk_context()
        pipeline = GraphicsPipeline(
            ;
            vertex=_probe_vertex,
            fragment=_probe_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthOff(),
        )
        created = LavaBackend(
            context,
            context.default_bq,
            pipeline,
            Dict{Tuple{Int, Int}, Lava.LavaFramebuffer}(),
            UInt64(0),
            UInt64(0),
            UInt64(0),
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
        offscreen_raster=false,
        depth_attachment=false,
        texture_sampling=false,
        readback=false,
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
    state.pipeline_compilations == 0 && (state.pipeline_compilations = 1)
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

end
