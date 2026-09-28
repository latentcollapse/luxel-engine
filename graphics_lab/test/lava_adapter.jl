using Base64
using SHA
using Test
using WGEGraphics

include(joinpath(@__DIR__, "..", "src", "LavaAdapter.jl"))
using .LavaAdapter

@testset "RGBA payload layout" begin
    bytes = UInt8[
        1, 2, 3, 4,
        5, 6, 7, 8,
        9, 10, 11, 12,
        13, 14, 15, 16,
        17, 18, 19, 20,
        21, 22, 23, 24,
    ]
    matrix = LavaAdapter._texture_matrix(bytes, UInt32(3), UInt32(2))
    @test size(matrix) == (2, 3)
    @test matrix[1, 1] == (1.0f0 / 255.0f0, 2.0f0 / 255.0f0, 3.0f0 / 255.0f0, 4.0f0 / 255.0f0)
    @test matrix[2, 3] == (21.0f0 / 255.0f0, 22.0f0 / 255.0f0, 23.0f0 / 255.0f0, 24.0f0 / 255.0f0)
end

@testset "camera projection lowering" begin
    orthographic = WGEGraphics.CameraPacket(
        "orthographic",
        WGEGraphics.OrthographicProjection(20.0f0),
        (0.0f0, 10.0f0, 0.0f0),
        (0.0f0, -1.0f0, 0.0f0),
        (0.0f0, 0.0f0, -1.0f0),
        0.1f0,
        100.0f0,
        UInt32(320),
        UInt32(240),
    )
    orthographic_frame = LavaAdapter._camera_frame(orthographic)
    orthographic_center = LavaAdapter._project_point(orthographic_frame, (0.0f0, 0.0f0, 0.0f0))
    @test orthographic_frame.mode == 0.0f0
    @test orthographic_center[1] ≈ 0.0f0
    @test orthographic_center[2] ≈ 0.0f0
    @test 0.0f0 < orthographic_center[3] < 1.0f0

    perspective = WGEGraphics.CameraPacket(
        "perspective",
        WGEGraphics.PerspectiveProjection(60.0f0),
        (0.0f0, 10.0f0, 0.0f0),
        (0.0f0, -1.0f0, 0.0f0),
        (0.0f0, 0.0f0, -1.0f0),
        0.1f0,
        100.0f0,
        UInt32(320),
        UInt32(240),
    )
    perspective_frame = LavaAdapter._camera_frame(perspective)
    perspective_center = LavaAdapter._project_point(perspective_frame, (0.0f0, 0.0f0, 0.0f0))
    @test perspective_frame.mode == 1.0f0
    @test perspective_center[1] ≈ 0.0f0
    @test perspective_center[2] ≈ 0.0f0
    @test 0.0f0 < perspective_center[3] < 1.0f0
end

@testset "persistent Lava adapter" begin
    state = LavaAdapter.backend()
    @test state === LavaAdapter.backend()

    capabilities = LavaAdapter.backend_probe(state)
    @test capabilities.schema == "wge.lava-backend-probe/v1"
    @test capabilities.persistent_context
    @test !isempty(capabilities.device_uuid)
    @test capabilities.hardware_ray_tracing

    first = LavaAdapter.render_probe(24, 12, state)
    @test first.matrix_size == (24, 12)
    @test first.center_rgba == Float32[0.0, 1.0, 0.0, 1.0]
    @test first.green_pixels > 0
    first_bytes = base64decode(first.capture_base64)
    @test length(first_bytes) == 24 * 12 * 4
    @test first.capture_sha256 == "sha256:" * bytes2hex(sha256(first_bytes))
    @test first.telemetry.draw_calls == 1
    @test first.telemetry.pipeline_compilations == 1

    second = LavaAdapter.render_probe(8, 6, state)
    @test second.matrix_size == (8, 6)
    @test second.center_rgba == Float32[0.0, 1.0, 0.0, 1.0]
    @test second.telemetry.draw_calls == 2
    @test second.telemetry.pipeline_compilations == 1
    @test length(base64decode(second.capture_base64)) == 8 * 6 * 4

    depth = LavaAdapter.render_depth_probe(state)
    @test depth.nearer_fragment_won
    @test depth.center_rgba[3] > 0.9f0

    texture = LavaAdapter.render_texture_probe(state)
    @test texture.texture_sampled
    @test texture.center_rgba ≈ texture.expected_rgba atol=0.05f0

    final_capabilities = LavaAdapter.backend_probe(state)
    @test final_capabilities.offscreen_raster
    @test final_capabilities.readback
    @test final_capabilities.depth_attachment
    @test final_capabilities.texture_sampling
end
