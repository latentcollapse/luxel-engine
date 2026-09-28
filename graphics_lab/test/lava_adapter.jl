using Base64
using SHA
using Test

include(joinpath(@__DIR__, "..", "src", "LavaAdapter.jl"))
using .LavaAdapter

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
end
