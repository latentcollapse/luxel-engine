#!/usr/bin/env julia

using JSON3
using SHA

const REQUEST_SCHEMA = "wge.julia-world-fields-request/v1"
const RESPONSE_SCHEMA = "wge.julia-world-fields-response/v1"

function _exact_keys(value, expected::Set{String}, label::String)
    value isa JSON3.Object || error("$label must be a JSON object")
    Set(String(key) for key in keys(value)) == expected ||
        error("$label does not match its closed schema")
end

function _string(value, label::String)
    value isa AbstractString || error("$label must be a string")
    result = String(value)
    isempty(result) && error("$label must not be empty")
    result
end

function _integer(value, label::String)
    if value isa Integer && !(value isa Bool)
        return Int(value)
    elseif value isa AbstractFloat && isfinite(value) && isinteger(value)
        return Int(value)
    end
    error("$label must be an integer")
end

function _u64(value, label::String)
    value isa Integer && !(value isa Bool) || error("$label must be an unsigned integer")
    value >= 0 || error("$label must be an unsigned integer")
    UInt64(value)
end

function _float(value, label::String)
    value isa Real && !(value isa Bool) || error("$label must be numeric")
    result = Float64(value)
    isfinite(result) || error("$label must be finite")
    result
end

function _array(value, label::String)
    value isa JSON3.Array || error("$label must be an array")
    value
end

function _point(value, label::String)
    point = _array(value, label)
    length(point) == 2 || error("$label must contain two coordinates")
    return (_float(point[1], "$label[1]"), _float(point[2], "$label[2]"))
end

function _point_in_polygon(x::Float64, z::Float64, polygon::Vector{Tuple{Float64,Float64}})
    inside = false
    previous = length(polygon)
    for current in eachindex(polygon)
        xi, zi = polygon[current]
        xj, zj = polygon[previous]
        if ((zi > z) != (zj > z)) &&
           (x < (xj - xi) * (z - zi) / (zj - zi) + xi)
            inside = !inside
        end
        previous = current
    end
    inside
end

function _region_at(x::Float64, z::Float64, regions)
    for region in regions
        _point_in_polygon(x, z, region.polygon) && return region.code
    end
    UInt8(0)
end

function _append_le_f64!(bytes::Vector{UInt8}, value::Float64)
    word = htol(reinterpret(UInt64, value))
    append!(bytes, reinterpret(UInt8, [word]))
    nothing
end

function _f64_array_bytes(values::Vector{Float64})
    bytes = UInt8[]
    for value in values
        _append_le_f64!(bytes, value)
    end
    return bytes
end

function _spatial_digest(resolution::Int, height_bytes::Vector{UInt8}, grade_bytes::Vector{UInt8}, code_bytes::Vector{UInt8})
    bytes = UInt8[]
    append!(bytes, codeunits("wge.julia-spatial-fields/v1\0"))
    word = htol(UInt64(resolution))
    append!(bytes, reinterpret(UInt8, [word]))
    append!(bytes, height_bytes)
    append!(bytes, grade_bytes)
    append!(bytes, code_bytes)
    return "sha256:" * bytes2hex(sha256(bytes))
end

function _decode_request(line::String)
    request = JSON3.read(line)
    _exact_keys(
        request,
        Set([
            "schema_version", "layout_sha256", "world_id", "width_m", "length_m",
            "resolution", "seed", "terrain", "regions",
        ]),
        "request",
    )
    _string(request.schema_version, "schema_version") == REQUEST_SCHEMA ||
        error("unsupported request schema")
    layout_sha256 = _string(request.layout_sha256, "layout_sha256")
    occursin(r"^sha256:[0-9a-f]{64}$", layout_sha256) || error("layout_sha256 is malformed")
    world_id = _string(request.world_id, "world_id")
    width = _float(request.width_m, "width_m")
    length_m = _float(request.length_m, "length_m")
    width > 0 && length_m > 0 || error("world dimensions must be positive")
    resolution = _integer(request.resolution, "resolution")
    17 <= resolution <= 257 && isodd(resolution) || error("resolution is outside the supported range")
    seed = _u64(request.seed, "seed")

    terrain = request.terrain
    _exact_keys(terrain, Set(["base_elevation_m", "noise_amplitude_m", "features"]), "terrain")
    base = _float(terrain.base_elevation_m, "terrain.base_elevation_m")
    noise_amplitude = _float(terrain.noise_amplitude_m, "terrain.noise_amplitude_m")
    noise_amplitude >= 0 || error("noise amplitude must not be negative")
    features = NamedTuple[]
    seen_features = Set{String}()
    for (index, feature) in enumerate(_array(terrain.features, "terrain.features"))
        label = "terrain.features[$index]"
        _exact_keys(
            feature,
            Set(["feature_id", "center_xz_m", "radius_x_m", "radius_z_m", "elevation_m"]),
            label,
        )
        id = _string(feature.feature_id, "$label.feature_id")
        id in seen_features && error("duplicate terrain feature id $id")
        push!(seen_features, id)
        center = _point(feature.center_xz_m, "$label.center_xz_m")
        radius_x = _float(feature.radius_x_m, "$label.radius_x_m")
        radius_z = _float(feature.radius_z_m, "$label.radius_z_m")
        radius_x > 0 && radius_z > 0 || error("terrain feature radii must be positive")
        push!(features, (
            id=id,
            x=center[1],
            z=center[2],
            rx=radius_x,
            rz=radius_z,
            elevation=_float(feature.elevation_m, "$label.elevation_m"),
        ))
    end

    regions = NamedTuple[]
    seen_ids = Set{String}()
    seen_codes = Set{UInt8}()
    seen_priorities = Set{Int}()
    for (index, region) in enumerate(_array(request.regions, "regions"))
        label = "regions[$index]"
        _exact_keys(
            region,
            Set(["region_id", "code", "priority", "blocks_traversal", "polygon_xz_m"]),
            label,
        )
        id = _string(region.region_id, "$label.region_id")
        code_value = _integer(region.code, "$label.code")
        1 <= code_value <= 255 || error("region code must be 1 through 255")
        code = UInt8(code_value)
        priority = _integer(region.priority, "$label.priority")
        region.blocks_traversal isa Bool || error("$label.blocks_traversal must be boolean")
        (id in seen_ids || code in seen_codes || priority in seen_priorities) &&
            error("regions must have unique ids, codes, and priorities")
        push!(seen_ids, id)
        push!(seen_codes, code)
        push!(seen_priorities, priority)
        polygon_values = _array(region.polygon_xz_m, "$label.polygon_xz_m")
        length(polygon_values) >= 3 || error("region polygon needs at least three points")
        polygon = [_point(point, "$label.polygon_xz_m[$point_index]") for (point_index, point) in enumerate(polygon_values)]
        push!(regions, (
            id=id,
            code=code,
            priority=priority,
            blocked=region.blocks_traversal,
            polygon=polygon,
        ))
    end
    sort!(regions, by=region -> region.priority)
    return (
        layout_sha256=layout_sha256,
        world_id=world_id,
        width=width,
        length=length_m,
        resolution=resolution,
        seed=seed,
        base=base,
        noise_amplitude=noise_amplitude,
        features=features,
        regions=regions,
    )
end

function _generate(request, request_sha256::String)
    resolution = request.resolution
    count = resolution * resolution
    heights = Vector{Float64}(undef, count)
    region_codes = Vector{UInt8}(undef, count)
    phase = Float64(request.seed % UInt64(1_000_003)) / 1_000_003.0 * 2π
    for row in 1:resolution
        z = request.length / 2 - (row - 1) * request.length / (resolution - 1)
        for column in 1:resolution
            x = -request.width / 2 + (column - 1) * request.width / (resolution - 1)
            height = request.base
            for feature in request.features
                dx = (x - feature.x) / feature.rx
                dz = (z - feature.z) / feature.rz
                height += feature.elevation * exp(-0.5 * (dx * dx + dz * dz))
            end
            noise = sin(x * 0.173 + phase) * cos(z * 0.137 - phase * 0.37)
            noise += 0.25 * sin((x + z) * 0.071 + phase * 0.51)
            heights[(row - 1) * resolution + column] = height + request.noise_amplitude * noise
            region_codes[(row - 1) * resolution + column] = _region_at(x, z, request.regions)
        end
    end

    spacing_x = request.width / (resolution - 1)
    spacing_z = request.length / (resolution - 1)
    grades = zeros(Float64, count)
    for row in 1:resolution
        for column in 1:resolution
            index = (row - 1) * resolution + column
            height = heights[index]
            grade = 0.0
            if column > 1
                grade = max(grade, abs(height - heights[index - 1]) / spacing_x)
            end
            if column < resolution
                grade = max(grade, abs(height - heights[index + 1]) / spacing_x)
            end
            if row > 1
                grade = max(grade, abs(height - heights[index - resolution]) / spacing_z)
            end
            if row < resolution
                grade = max(grade, abs(height - heights[index + resolution]) / spacing_z)
            end
            grades[index] = grade
        end
    end

    height_bytes = _f64_array_bytes(heights)
    grade_bytes = _f64_array_bytes(grades)
    code_bytes = collect(region_codes)
    return (
        schema_version=RESPONSE_SCHEMA,
        request_sha256=request_sha256,
        world_id=request.world_id,
        resolution=resolution,
        width_m=request.width,
        length_m=request.length,
        heights_f64_le_hex=bytes2hex(height_bytes),
        heights_sha256="sha256:" * bytes2hex(sha256(height_bytes)),
        slope_grade_f64_le_hex=bytes2hex(grade_bytes),
        slope_grade_sha256="sha256:" * bytes2hex(sha256(grade_bytes)),
        region_codes_hex=bytes2hex(code_bytes),
        region_codes_sha256="sha256:" * bytes2hex(sha256(code_bytes)),
        spatial_sha256=_spatial_digest(resolution, height_bytes, grade_bytes, code_bytes),
    )
end

function main()
    line = readline(stdin)
    eof(stdin) || error("worker accepts exactly one request frame")
    request = _decode_request(line)
    request_sha256 = "sha256:" * bytes2hex(sha256(codeunits(line)))
    println(JSON3.write(_generate(request, request_sha256)))
end

main()
