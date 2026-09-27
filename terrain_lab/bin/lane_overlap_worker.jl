#!/usr/bin/env julia
# Pinned lane-overlap worker. Reads length-prefixed JSON jobs from stdin.
# The executable vocabulary is this file. Job payloads cannot add operations.

using JSON3
using SHA

struct AxisAligned end

function overlap_1d(a0::Int, a1::Int, b0::Int, b1::Int)
    max(a0, b0), min(a1, b1)
end

"""Axis-aligned overlap. Other domains would be additional methods, not author payloads."""
function measure(::AxisAligned, lane, footprint)
    x0, x1 = overlap_1d(lane.x0, lane.x1, footprint.x0, footprint.x1)
    y0, y1 = overlap_1d(lane.y0, lane.y1, footprint.y0, footprint.y1)
    width = max(0, x1 - x0)
    height = max(0, y1 - y0)
    area = width * height
    if area == 0
        return (intersects=false, x0=0, x1=0, y0=0, y1=0, area=0)
    end
    return (intersects=true, x0=x0, x1=x1, y0=y0, y1=y1, area=area)
end

function read_frame(io::IO)
    header = read(io, 4)
    if isempty(header)
        return nothing
    end
    length(header) == 4 || error("truncated header")
    n = (Int(header[1]) << 24) | (Int(header[2]) << 16) | (Int(header[3]) << 8) | Int(header[4])
    0 <= n <= 1_000_000 || error("frame length out of range")
    payload = read(io, n)
    length(payload) == n || error("truncated frame")
    return payload
end

function write_frame(io::IO, payload::AbstractString)
    bytes = Vector{UInt8}(payload)
    n = length(bytes)
    header = UInt8[
        UInt8((n >> 24) & 0xff),
        UInt8((n >> 16) & 0xff),
        UInt8((n >> 8) & 0xff),
        UInt8(n & 0xff),
    ]
    write(io, header)
    write(io, bytes)
    flush(io)
    return nothing
end

function as_int(value)
    if value isa Integer
        return Int(value)
    elseif value isa AbstractFloat && isinteger(value) && isfinite(value)
        return Int(value)
    end
    error("coordinate is not an integer")
end

function keyset(obj)
    Set(String(k) for k in keys(obj))
end

function rect(obj)
    keyset(obj) == Set(["x0", "x1", "y0", "y1"]) || error("rectangle fields are closed")
    x0 = as_int(obj["x0"])
    x1 = as_int(obj["x1"])
    y0 = as_int(obj["y0"])
    y1 = as_int(obj["y1"])
    x1 > x0 && y1 > y0 || error("rectangle is degenerate")
    return (x0=x0, x1=x1, y0=y0, y1=y1)
end

const FORBIDDEN = Set([
    "ccall", "code", "eval", "gate_passed", "include", "receipt", "register", "source",
])

function canonical_measurement(measured)
    flag = measured.intersects ? "true" : "false"
    "{\"intersects\":$(flag),\"overlap\":{\"x0\":$(measured.x0),\"x1\":$(measured.x1),\"y0\":$(measured.y0),\"y1\":$(measured.y1)},\"overlap_area\":$(measured.area),\"schema\":\"wge.lane-overlap/v0\"}"
end

function failure(code, detail)
    safe = replace(detail, "\\" => "\\\\")
    safe = replace(safe, "\"" => "\\\"")
    "{\"class\":\"OperatorFailure\",\"code\":\"$(code)\",\"detail\":\"$(safe)\"}"
end

function segment(obj)
    keyset(obj) == Set(["x0", "x1", "y0", "y1"]) || error("path fields are closed")
    x0 = as_int(obj["x0"])
    x1 = as_int(obj["x1"])
    y0 = as_int(obj["y0"])
    y1 = as_int(obj["y1"])
    return (x0=x0, x1=x1, y0=y0, y1=y1)
end

struct Manhattan end

function measure(::Manhattan, path)
    return abs(path.x1 - path.x0) + abs(path.y1 - path.y0)
end

function canonical_length(len::Int)
    "{\"length\":$(len),\"schema\":\"wge.path-length/v0\"}"
end

function handle(payload::AbstractString)
    value = try
        JSON3.read(payload)
    catch err
        return failure("malformed_job", sprint(showerror, err))
    end
    value isa JSON3.Object || return failure("malformed_job", "job must be an object")
    names = keyset(value)
    !isempty(intersect(names, FORBIDDEN)) && return failure("rejected_payload", "job carries a forbidden field")
    haskey(value, "op") || return failure("malformed_job", "missing op")
    op = value["op"]
    op isa AbstractString || return failure("malformed_job", "op must be a string")
    if op == "lane_overlap"
        names == Set(["footprint", "lane", "op"]) || return failure("rejected_payload", "lane_overlap accepts only op, lane, footprint")
        lane = try
            rect(value["lane"])
        catch err
            return failure("malformed_job", sprint(showerror, err))
        end
        footprint = try
            rect(value["footprint"])
        catch err
            return failure("malformed_job", sprint(showerror, err))
        end
        return canonical_measurement(measure(AxisAligned(), lane, footprint))
    elseif op == "path_length"
        names == Set(["op", "path"]) || return failure("rejected_payload", "path_length accepts only op, path")
        path = try
            segment(value["path"])
        catch err
            return failure("malformed_job", sprint(showerror, err))
        end
        return canonical_length(measure(Manhattan(), path))
    end
    return failure("unsupported_operation", "pinned operations are lane_overlap and path_length")
end

function main()
    script = String(read(PROGRAM_FILE))
    digest = bytes2hex(sha256(script))
    write_frame(stdout, "{\"op\":\"ready\",\"script_sha256\":\"$(digest)\"}")
    while true
        frame = read_frame(stdin)
        frame === nothing && break
        write_frame(stdout, handle(String(frame)))
    end
end

main()
