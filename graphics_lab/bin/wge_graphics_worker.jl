#!/usr/bin/env julia

using JSON3
using SHA
using WGEGraphics

include(joinpath(@__DIR__, "..", "src", "LavaAdapter.jl"))
using .LavaAdapter

const WORKER_SCHEMA = "wge.graphics-worker/v1"
const LAVA_REVISION = "11c7e31bdf62408d22bf379e9e59510f69d2103e"
const MAX_WORKER_FRAME_BYTES = 64 * 1024 * 1024

function read_frame(io::IO)::Union{Nothing, String}
    header = read(io, 4)
    isempty(header) && return nothing
    length(header) == 4 || error("truncated frame header")
    length_bytes = (Int(header[1]) << 24) | (Int(header[2]) << 16) | (Int(header[3]) << 8) | Int(header[4])
    0 <= length_bytes <= MAX_WORKER_FRAME_BYTES || error("frame length is outside the worker bound")
    payload = read(io, length_bytes)
    length(payload) == length_bytes || error("truncated frame payload")
    return String(payload)
end

function write_frame(io::IO, payload::AbstractString)
    bytes = Vector{UInt8}(payload)
    length(bytes) <= MAX_WORKER_FRAME_BYTES || error("response is outside the worker bound")
    length(bytes) <= typemax(UInt32) || error("response length overflows frame header")
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
    raw_operation = get(value, "op", nothing)
    raw_operation isa AbstractString || return failure("malformed_request", "op must be a string")
    operation = String(raw_operation)
    try
        if operation == "validate_packet"
            Set(String(key) for key in keys(value)) == Set(("op", "packet")) ||
                return failure("malformed_request", "validate_packet request fields are closed")
            packet = validate_scene_packet(value["packet"])
            summary = packet_summary(packet)
            return JSON3.write((
                schema=WORKER_SCHEMA,
                kind="packet_shape_checked",
                authority="producer_only",
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
        elseif operation == "probe_backend"
            allowed = Set(("op", "width_px", "height_px"))
            request_keys = Set(String(key) for key in keys(value))
            request_keys ⊆ allowed || return failure("malformed_request", "probe_backend request fields are closed")
            width = _request_dimension(value, "width_px", 16)
            height = _request_dimension(value, "height_px", 16)
            width === nothing && return failure("malformed_request", "width_px must be a positive integer")
            height === nothing && return failure("malformed_request", "height_px must be a positive integer")
            state = LavaAdapter.backend()
            render = LavaAdapter.render_probe(width, height, state)
            capabilities = LavaAdapter.backend_probe(state)
            return JSON3.write((
                schema=WORKER_SCHEMA,
                kind="backend_probed",
                capabilities=capabilities,
                render=render,
            ))
        elseif operation == "render_packet"
            Set(String(key) for key in keys(value)) ==
                Set(("op", "packet", "expected_packet_sha256")) ||
                return failure("malformed_request", "render_packet request fields are closed")
            packet = validate_scene_packet(value["packet"])
            expected_packet_sha256 = WGEGraphics._string(
                value["expected_packet_sha256"],
                "expected_packet_sha256",
            )
            expected_packet_sha256 == packet.packet_sha256 ||
                return failure(
                    "provenance",
                    "Rust packet identity does not match the worker packet",
                )
            frame = LavaAdapter.render_scene(packet)
            return JSON3.write((schema=WORKER_SCHEMA, kind="frame_rendered", frame=frame))
        elseif operation == "open_window"
            allowed = Set(("op", "width_px", "height_px", "vsync"))
            request_keys = Set(String(key) for key in keys(value))
            request_keys ⊆ allowed ||
                return failure("malformed_request", "open_window request fields are closed")
            width = _request_dimension(value, "width_px", 1280)
            height = _request_dimension(value, "height_px", 720)
            width === nothing && return failure("malformed_request", "width_px must be a positive integer")
            height === nothing && return failure("malformed_request", "height_px must be a positive integer")
            vsync = haskey(value, "vsync") ? value["vsync"] : true
            vsync isa Bool || return failure("malformed_request", "vsync must be a boolean")
            state = LavaAdapter.backend()
            opened = LavaAdapter.open_window_session(state, width, height; vsync=vsync)
            return JSON3.write((
                schema=WORKER_SCHEMA,
                kind="window_opened",
                window_width_px=opened.window_width_px,
                window_height_px=opened.window_height_px,
                vsync=opened.vsync,
            ))
        elseif operation == "render_window"
            allowed = Set(("op", "packet", "camera_override", "frame_count", "expected_packet_sha256"))
            request_keys = Set(String(key) for key in keys(value))
            request_keys ⊆ allowed ||
                return failure("malformed_request", "render_window request fields are closed")
            packet = validate_scene_packet(value["packet"])
            expected_packet_sha256 = WGEGraphics._string(
                value["expected_packet_sha256"],
                "expected_packet_sha256",
            )
            expected_packet_sha256 == packet.packet_sha256 ||
                return failure(
                    "provenance",
                    "Rust packet identity does not match the worker packet",
                )
            camera_override = nothing
            if haskey(value, "camera_override") && value["camera_override"] isa JSON3.Object
                camera_override = WGEGraphics._parse_camera(value["camera_override"])
            end
            frame_count_raw = get(value, "frame_count", nothing)
            frame_count_raw isa Integer ||
                return failure("malformed_request", "frame_count must be a positive integer")
            frame_count = Int(frame_count_raw)
            1 <= frame_count <= 1024 ||
                return failure("malformed_request", "frame_count must be in 1..1024")
            state = LavaAdapter.backend()
            result = LavaAdapter.render_window_frames!(
                state,
                packet,
                camera_override,
                frame_count,
            )
            return JSON3.write((
                schema=WORKER_SCHEMA,
                kind="window_frames_presented",
                frames_presented=result.frames_presented,
                frame_times_us=result.frame_times_us,
                window_presented_frames=result.window_presented_frames,
            ))
        elseif operation == "close_window"
            Set(String(key) for key in keys(value)) == Set(("op",)) ||
                return failure("malformed_request", "close_window request fields are closed")
            state = LavaAdapter.backend()
            closed = LavaAdapter.close_window_session!(state)
            return JSON3.write((
                schema=WORKER_SCHEMA,
                kind="window_closed",
                closed=closed.closed,
                presented_frames=Int(closed.presented_frames),
            ))
        elseif operation == "probe_capabilities"
            Set(String(key) for key in keys(value)) == Set(("op",)) ||
                return failure("malformed_request", "probe_capabilities request fields are closed")
            state = LavaAdapter.backend()
            color = LavaAdapter.render_probe(16, 16, state)
            depth = LavaAdapter.render_depth_probe(state)
            texture = LavaAdapter.render_texture_probe(state)
            capabilities = LavaAdapter.backend_probe(state)
            return JSON3.write((
                schema=WORKER_SCHEMA,
                kind="capabilities_probed",
                capabilities=capabilities,
                ready=LavaAdapter.backend_ready(state),
                color=color,
                depth=depth,
                texture=texture,
            ))
        else
            return failure("unsupported_operation", "operation is not available in the graphics worker")
        end
    catch error
        if error isa ProtocolError || error isa AdapterError
            return failure(error.code, error.detail)
        end
        return failure("worker_error", sprint(showerror, error))
    end
end

function _request_dimension(value::JSON3.Object, key::String, default::Int)::Union{Nothing, Int}
    haskey(value, key) || return default
    candidate = value[key]
    if candidate isa Integer
        integer = Int(candidate)
        return 1 <= integer <= 8192 ? integer : nothing
    elseif candidate isa AbstractFloat && isfinite(candidate) && isinteger(candidate)
        integer = Int(candidate)
        return 1 <= integer <= 8192 ? integer : nothing
    end
    return nothing
end

function main()::Nothing
    script_sha256 = "sha256:" * bytes2hex(sha256(read(PROGRAM_FILE)))
    write_frame(stdout, JSON3.write((
        schema=WORKER_SCHEMA,
        kind="ready",
        profile="protocol_and_lava_lazy",
        script_sha256=script_sha256,
        lava_revision=LAVA_REVISION,
        adapter_revision=LavaAdapter.ADAPTER_REVISION,
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
