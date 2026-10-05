# Spiral G bisect probe — startup -> single point render -> readback.
using Vulkan
using Printf
using SHA

const W = UInt32(1024)
const H = UInt32(1024)

step(name) = println(stderr, "[probe] $name"); flush(stderr)

step("instance")
instance = Instance([], []; application_info = ApplicationInfo(v"0.0.1", v"0.0.1", v"1.3";
    application_name = "probe", engine_name = "TetCage"))
step("enumerate")
pdevices = unwrap(enumerate_physical_devices(instance))
pdevice = pdevices[1]
props = unwrap(get_physical_device_properties(pdevice))
step("device $(props.device_name)")
queue_family = find_queue_family(pdevice, QUEUE_GRAPHICS_BIT)
device = Device(pdevice, [DeviceQueueCreateInfo(queue_family, [1.0])], [], [])
queue = get_device_queue(device, queue_family, UInt32(0))
mp = unwrap(get_physical_device_memory_properties(pdevice))

step("glslc")
vs_path = joinpath(tempdir(), "probe.vert")
open(vs_path, "w") do f
    write(f, """
#version 450
layout(location = 0) in vec3 in_pos;
layout(location = 0) flat out vec2 out_fb;
layout(push_constant) uniform PC { float u_fpx; float u_d; float u_w; float u_h; } pc;
void main() {
    float zc = max(pc.u_d - in_pos.z, 0.05);
    float clip_x = in_pos.x * pc.u_fpx * 2.0 / pc.u_w;
    float clip_y = in_pos.y * pc.u_fpx * 2.0 / pc.u_h;
    gl_Position = vec4(clip_x, clip_y, 0.0, zc);
    out_fb = vec2((clip_x / zc * 0.5 + 0.5) * pc.u_w, (clip_y / zc * 0.5 + 0.5) * pc.u_h);
}
""")
end
run(pipeline(`glslc -O -o $(vs_path * ".spv") $vs_path`; stdout = devnull, stderr = devnull))
vs_bytes = read(vs_path * ".spv")
vs_code = UInt32.(reinterpret(UInt32, vs_bytes))
step("spirv $(length(vs_code)) words")

step("image")
image = unwrap(create_image(device, ImageCreateInfo(
    IMAGE_TYPE_2D, FORMAT_R32G32_SFLOAT, Extent3D(W, H, UInt32(1)),
    UInt32(1), UInt32(1), SAMPLE_COUNT_1_BIT, IMAGE_TILING_OPTIMAL,
    IMAGE_USAGE_COLOR_ATTACHMENT_BIT | IMAGE_USAGE_TRANSFER_SRC_BIT,
    SHARING_MODE_EXCLUSIVE, UInt32[], IMAGE_LAYOUT_UNDEFINED)))
img_reqs = unwrap(get_image_memory_requirements(device, image))
function find_mt(type_bits::UInt32, want::UInt32)
    for i in 0:(length(mp.memory_types)-1)
        (type_bits & (1 << i)) != 0 && (UInt32(mp.memory_types[i+1].property_flags) & want) == want && return UInt32(i)
    end
    error("no mem type")
end
img_mem = unwrap(allocate_memory(device, MemoryAllocateInfo(img_reqs.size,
    find_mt(img_reqs.memory_type_bits, UInt32(MEMORY_PROPERTY_DEVICE_LOCAL_BIT)))))
unwrap(bind_image_memory(device, image, img_mem, UInt64(0)))
image_view = unwrap(create_image_view(device, ImageViewCreateInfo(
    image, IMAGE_VIEW_TYPE_2D, FORMAT_R32G32_SFLOAT,
    ComponentMapping(COMPONENT_SWIZZLE_IDENTITY, COMPONENT_SWIZZLE_IDENTITY,
                     COMPONENT_SWIZZLE_IDENTITY, COMPONENT_SWIZZLE_IDENTITY),
    ImageSubresourceRange(IMAGE_ASPECT_COLOR_BIT, UInt32(0), UInt32(1), UInt32(0), UInt32(1)))))

step("renderpass")
rp = unwrap(create_render_pass(device, RenderPassCreateInfo(
    [AttachmentDescription(FORMAT_R32G32_SFLOAT, SAMPLE_COUNT_1_BIT,
        ATTACHMENT_LOAD_OP_CLEAR, ATTACHMENT_STORE_OP_STORE,
        ATTACHMENT_LOAD_OP_DONT_CARE, ATTACHMENT_STORE_OP_DONT_CARE,
        IMAGE_LAYOUT_UNDEFINED, IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL)],
    [SubpassDescription(PIPELINE_BIND_POINT_GRAPHICS, AttachmentReference[],
        [AttachmentReference(UInt32(0), IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL)], UInt32[])],
    [SubpassDependency(UInt32(SUBPASS_EXTERNAL), UInt32(0);
        src_stage_mask = PIPELINE_STAGE_LATE_FRAGMENT_TESTS_BIT,
        dst_stage_mask = PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
        src_access_mask = ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
        dst_access_mask = ACCESS_COLOR_ATTACHMENT_READ_BIT | ACCESS_COLOR_ATTACHMENT_WRITE_BIT)])))
framebuffer = unwrap(create_framebuffer(device, FramebufferCreateInfo(rp, [image_view], W, H, UInt32(1))))

step("pipeline")
push_ranges = [PushConstantRange(SHADER_STAGE_VERTEX_BIT, UInt32(0), UInt32(16))]
layout = unwrap(create_pipeline_layout(device, PipelineLayoutCreateInfo([], push_ranges)))
vs_mod = unwrap(create_shader_module(device, ShaderModuleCreateInfo(UInt(length(vs_code)) * 4, vs_code)))
fs_path = joinpath(tempdir(), "probe.frag")
open(fs_path, "w") do f
    write(f, """
#version 450
layout(location = 0) flat in vec2 in_fb;
layout(location = 0) out vec2 out_color;
void main() { out_color = in_fb; }
""")
end
run(pipeline(`glslc -O -o $(fs_path * ".spv") $fs_path`; stdout = devnull, stderr = devnull))
fs_code = UInt32.(reinterpret(UInt32, read(fs_path * ".spv")))
fs_mod = unwrap(create_shader_module(device, ShaderModuleCreateInfo(UInt(length(fs_code)) * 4, fs_code)))
stages = [PipelineShaderStageCreateInfo(SHADER_STAGE_VERTEX_BIT, vs_mod, "main"),
          PipelineShaderStageCreateInfo(SHADER_STAGE_FRAGMENT_BIT, fs_mod, "main")]
vertex_input = PipelineVertexInputStateCreateInfo(
    [VertexInputBindingDescription(UInt32(0), UInt32(12), VERTEX_INPUT_RATE_VERTEX)],
    [VertexInputAttributeDescription(UInt32(0), UInt32(0), FORMAT_R32G32B32_SFLOAT, UInt32(0))])
pipeline = first(first(unwrap(create_graphics_pipelines(device,
    [GraphicsPipelineCreateInfo(stages,
        PipelineRasterizationStateCreateInfo(false, false, POLYGON_MODE_FILL,
            FRONT_FACE_COUNTER_CLOCKWISE, false, 0f0, 0f0, 0f0, 1f0),
        layout, 0, -1;
        vertex_input_state = vertex_input,
        input_assembly_state = PipelineInputAssemblyStateCreateInfo(PRIMITIVE_TOPOLOGY_POINT_LIST, false),
        viewport_state = PipelineViewportStateCreateInfo(;
            viewports = [Viewport(0.0, 0.0, Float32(W), Float32(H), 0.0, 1.0)],
            scissors = [Rect2D(Offset2D(0, 0), Extent2D(W, H))]),
        multisample_state = PipelineMultisampleStateCreateInfo(SAMPLE_COUNT_1_BIT, false, 0f0, false, false),
        color_blend_state = PipelineColorBlendStateCreateInfo(false, LOGIC_OP_COPY,
            [PipelineColorBlendAttachmentState(false, BLEND_FACTOR_ZERO, BLEND_FACTOR_ZERO,
                BLEND_OP_ADD, BLEND_FACTOR_ZERO, BLEND_FACTOR_ZERO, BLEND_OP_ADD;
                color_write_mask = COLOR_COMPONENT_R_BIT | COLOR_COMPONENT_G_BIT)],
            (0f0, 0f0, 0f0, 0f0)),
        render_pass = rp)]))))
step("pipeline ok")

command_pool = unwrap(create_command_pool(device, CommandPoolCreateInfo(queue_family)))
cb = first(unwrap(allocate_command_buffers(device,
    CommandBufferAllocateInfo(command_pool, COMMAND_BUFFER_LEVEL_PRIMARY, 1))))
fence = Fence(device)

step("buffers")
# 3 test points near known cells: (511.3, 512.7), (600.2, 400.8), (300.5, 700.9)
pts = [(0.5 * W - 0.7, 0.5 * H + 0.7, 0.0), (0.5 * W + 100.2, 0.5 * H - 100.8, 0.0),
       (0.5 * W - 200.5, 0.5 * H + 200.9, 0.0)]
vbo_size = 3 * 12
vbo = unwrap(create_buffer(device, BufferCreateInfo(UInt64(vbo_size),
    BUFFER_USAGE_VERTEX_BUFFER_BIT, SHARING_MODE_EXCLUSIVE, UInt32[])))
vreqs = unwrap(get_buffer_memory_requirements(device, vbo))
vmem = unwrap(allocate_memory(device, MemoryAllocateInfo(vreqs.size,
    find_mt(vreqs.memory_type_bits,
        UInt32(MEMORY_PROPERTY_HOST_VISIBLE_BIT) | UInt32(MEMORY_PROPERTY_HOST_COHERENT_BIT)))))
unwrap(bind_buffer_memory(device, vbo, vmem, UInt64(0)))
vptr = unwrap(map_memory(device, vmem, UInt64(0), vreqs.size))
vdata = reinterpret(Float32, unsafe_wrap(Array, Ptr{UInt8}(vptr), vbo_size; own = false))
# invert analytic: fb_x = W/2 + f*x/d => x = (fb_x - W/2)*d/f
for (i, (fx, fy, _)) in enumerate(pts)
    o = 3 * (i - 1) + 1
    vdata[o] = Float32((fx - 0.5 * W) * 5.0 / 500.0)
    vdata[o+1] = Float32((fy - 0.5 * H) * 5.0 / 500.0)
    vdata[o+2] = 0.0f0
end
unwrap(unmap_memory(device, vmem))

ibo_size = 3 * 4
ibo = unwrap(create_buffer(device, BufferCreateInfo(UInt64(ibo_size),
    BUFFER_USAGE_INDEX_BUFFER_BIT, SHARING_MODE_EXCLUSIVE, UInt32[])))
ireqs = unwrap(get_buffer_memory_requirements(device, ibo))
imem = unwrap(allocate_memory(device, MemoryAllocateInfo(ireqs.size,
    find_mt(ireqs.memory_type_bits,
        UInt32(MEMORY_PROPERTY_HOST_VISIBLE_BIT) | UInt32(MEMORY_PROPERTY_HOST_COHERENT_BIT)))))
unwrap(bind_buffer_memory(device, ibo, imem, UInt64(0)))
iptr = unwrap(map_memory(device, imem, UInt64(0), ireqs.size))
idata = reinterpret(UInt32, unsafe_wrap(Array, Ptr{UInt8}(iptr), ibo_size; own = false))
idata[1] = 0; idata[2] = 1; idata[3] = 2
unwrap(unmap_memory(device, imem))

rbo_size = Int(W) * Int(H) * 8
rbo = unwrap(create_buffer(device, BufferCreateInfo(UInt64(rbo_size),
    BUFFER_USAGE_TRANSFER_DST_BIT, SHARING_MODE_EXCLUSIVE, UInt32[])))
rreqs = unwrap(get_buffer_memory_requirements(device, rbo))
rmem = unwrap(allocate_memory(device, MemoryAllocateInfo(rreqs.size,
    find_mt(rreqs.memory_type_bits,
        UInt32(MEMORY_PROPERTY_HOST_VISIBLE_BIT) | UInt32(MEMORY_PROPERTY_HOST_COHERENT_BIT)))))
unwrap(bind_buffer_memory(device, rbo, rmem, UInt64(0)))
step("buffers ok")

pc_data = Float32[500.0, 5.0, W, H]
pc_ptr = Ptr{Cvoid}(pointer(pc_data))

step("record + submit")
unwrap(begin_command_buffer(cb, CommandBufferBeginInfo()))
GC.@preserve pc_data begin
    cmd_begin_render_pass(cb,
        RenderPassBeginInfo(rp, framebuffer, Rect2D(Offset2D(0, 0), Extent2D(W, H)),
            [ClearValue(ClearColorValue((NaN32, NaN32, 0f0, 0f0)))]),
        SUBPASS_CONTENTS_INLINE)
    cmd_bind_pipeline(cb, PIPELINE_BIND_POINT_GRAPHICS, pipeline)
    cmd_bind_vertex_buffers(cb, [vbo], [UInt64(0)])
    cmd_bind_index_buffer(cb, ibo, UInt64(0), INDEX_TYPE_UINT32)
    cmd_push_constants(cb, layout, SHADER_STAGE_VERTEX_BIT, UInt32(0), UInt32(16), pc_ptr)
    cmd_draw_indexed(cb, 3, 1, 0, 0, 0)
    cmd_end_render_pass(cb)
end
cmd_pipeline_barrier(cb, [], [],
    [ImageMemoryBarrier(ACCESS_COLOR_ATTACHMENT_WRITE_BIT, ACCESS_TRANSFER_READ_BIT,
        IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        UInt32(queue_family), UInt32(queue_family), image,
        ImageSubresourceRange(IMAGE_ASPECT_COLOR_BIT, UInt32(0), UInt32(1), UInt32(0), UInt32(1)))];
    src_stage_mask = PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
    dst_stage_mask = PIPELINE_STAGE_TRANSFER_BIT)
cmd_copy_image_to_buffer(cb, image, IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, rbo,
    [BufferImageCopy(UInt64(0), UInt32(0), UInt32(0),
        ImageSubresourceLayers(IMAGE_ASPECT_COLOR_BIT, UInt32(0), UInt32(0), UInt32(1)),
        Offset3D(0, 0, 0), Extent3D(W, H, UInt32(1)))])
unwrap(end_command_buffer(cb))
unwrap(queue_submit(queue, [SubmitInfo([], [], [cb], [])], fence = fence))
step("wait")
unwrap(wait_for_fences(device, [fence], true, UInt64(1_000_000_000)))
step("readback")

rptr = unwrap(map_memory(device, rmem, UInt64(0), rreqs.size))
rdata = copy(reinterpret(Float32, unsafe_wrap(Array, Ptr{UInt8}(rptr), rbo_size; own = false)))
unwrap(unmap_memory(device, rmem))

for (i, (fx, fy, _)) in enumerate(pts)
    cx = floor(Int, fx); cy = floor(Int, fy)
    o = 2 * (cy * Int(W) + cx) + 1
    vx, vy = rdata[o], rdata[o+1]
    @printf(stderr, "[probe] point %d: predicted cell (%d,%d) payload (%.4f, %.4f) expected (%.4f, %.4f)\n",
            i, cx, cy, vx, vy, fx, fy)
end
flush(stderr)
println("PROBE COMPLETE")
