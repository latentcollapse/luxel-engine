#!/usr/bin/env julia

using JSON3
using SHA
using WGEGraphics

const WORKER_SCHEMA = "wge.graphics-worker/v1"
const LAVA_REVISION = "11c7e31bdf62408d22bf379e9e59510f69d2103e"

function read_frame(io::IO)
    header = read(io, 4)
    isempty(header) && return nothing
    length(header) == 4 || error("truncated frame header")
    length_bytes = (Int(header[1]) << 24) | (Int(header[2]) << 16) | (Int(header[3]) << 8) | Int(header[4])
    0 <= length_bytes <= 16_777_216 || error("frame length is outside the worker bound")
    payload = read(io, length_bytes)
    length(payload) == length_bytes || error("truncated frame payload")
    return String(payload)
end

function write_frame(io::IO, payload::AbstractString)
    bytes = Vector{UInt8}(payload)
    length(bytes) <= typemax(UInt32) || error("response is too large")
    n = UInt32(length(bytes))
    write(io, UInt8[(n >> 24) & 0xff, (n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff])
    write(io, bytes)
    flush(io)
    return nothing
end

function failure(code::String, detail::String)
    return JSON3.write((schema=WORKER_SCHEMA, kind="failed", code=code, detail=detail))
end

function handle(payload::AbstractString)::String
    value = try
        JSON3.read(payload)
    catch error
        return failure("malformed_request", "JSON decode failed: $(sprint(showerror, error))")
    end
    value isa JSON3.Object || return failure("malformed_request", "request must be an object")
    Set(String(key) for key in keys(value)) == Set(("op", "packet")) ||
        return failure("malformed_request", "request fields are closed")
    String(value["op"]) == "validate_packet" ||
        return failure("unsupported_operation", "only validate_packet is available in the protocol checkpoint")
    try
        packet = validate_scene_packet(JSON3.write(value["packet"]))
        summary = packet_summary(packet)
        return JSON3.write((
            schema=WORKER_SCHEMA,
            kind="packet_validated",
            packet_sha256=summary.packet_sha256,
            world_artifact_id=summary.world_artifact_id,
            spatial_fields_sha256=summary.spatial_fields_sha256,
            resolution=summary.resolution,
            terrain_samples=summary.terrain_samples,
            capture_id=summary.capture_id,
            width_px=summary.width_px,
            height_px=summary.height_px,
            overlay_count=summary.overlay_count,
            lava_revision=LAVA_REVISION,
        ))
    catch error
        if error isa ProtocolError
            return failure(error.code, error.detail)
        end
        return failure("worker_error", sprint(showerror, error))
    end
end

function main()
    script_sha256 = bytes2hex(sha256(read(PROGRAM_FILE)))
    write_frame(stdout, JSON3.write((
        schema=WORKER_SCHEMA,
        kind="ready",
        profile="protocol_only",
        script_sha256=script_sha256,
        lava_revision=LAVA_REVISION,
        julia_version=string(VERSION),
    )))
    while true
        frame = read_frame(stdin)
        frame === nothing && break
        write_frame(stdout, handle(frame))
    end
end

if abspath(PROGRAM_FILE) == abspath(@__FILE__)
    main()
end
