using JSON3
using Test
using WGEGraphics

include(joinpath(@__DIR__, "..", "bin", "wge_graphics_worker.jl"))

@testset "graphics worker protocol boundary" begin
    ready = JSON3.read(
        JSON3.write((
            schema="wge.graphics-worker/v1",
            kind="ready",
            profile="protocol_only",
            lava_revision=LAVA_REVISION,
        )),
    )
    @test ready["kind"] == "ready"
    @test LAVA_REVISION == "11c7e31bdf62408d22bf379e9e59510f69d2103e"
    failed = JSON3.read(handle("{\"op\":\"wrong\",\"packet\":{}}"))
    @test failed["kind"] == "failed"
    @test failed["code"] == "unsupported_operation"
    malformed = JSON3.read(handle("not-json"))
    @test malformed["code"] == "malformed_request"
    malformed_op = JSON3.read(handle(JSON3.write((op=42,))))
    @test malformed_op["code"] == "malformed_request"
    malformed_dimension = JSON3.read(handle(JSON3.write((op="probe_backend", width_px="wide"))))
    @test malformed_dimension["code"] == "malformed_request"
end

@testset "protocol packet parser rejects authority-shaped fakes" begin
    fake = JSON3.read(
        handle(
            JSON3.write((
                op="validate_packet",
                packet=(body=(schema_version="wge.graphics-scene-packet/v4",), packet_sha256="pass"),
            )),
        ),
    )
    @test fake["kind"] == "failed"
    @test fake["code"] in ("malformed_packet", "unsupported_schema")
end
