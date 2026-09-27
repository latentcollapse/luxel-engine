#!/usr/bin/env julia

using CodewealdTerrainLab
using JSON3
using SHA

const _REQUEST_COMMON = Set([
    "schema",
    "operation",
    "shape",
    "height_path",
    "height_sha256",
])

function _fail(code::String, message::String)
    throw(ErosionWorkerError(code, message))
end

function _exact_keys(value, expected::Set{String}, label::String)
    value isa JSON3.Object || _fail("malformed_request", "$label must be an object")
    actual = Set(String(key) for key in keys(value))
    actual == expected || _fail("malformed_request", "$label fields do not match the closed schema")
    return nothing
end

function _string(value, label::String)
    value isa AbstractString || _fail("malformed_request", "$label must be a string")
    return String(value)
end

function _integer(value, label::String)
    if value isa Integer && !(value isa Bool)
        return Int(value)
    elseif value isa AbstractFloat && isfinite(value) && isinteger(value)
        return Int(value)
    end
    _fail("malformed_request", "$label must be an integer")
end

function _float(value, label::String)
    value isa Real && !(value isa Bool) || _fail("malformed_request", "$label must be numeric")
    result = Float64(value)
    isfinite(result) || _fail("malformed_request", "$label must be finite")
    return result
end

function _shape(value)
    value isa JSON3.Array && length(value) == 2 ||
        _fail("invalid_shape", "shape must contain exactly two dimensions")
    rows, columns = _integer(value[1], "shape[1]"), _integer(value[2], "shape[2]")
    rows > 0 && columns > 0 || _fail("invalid_shape", "shape dimensions must be positive")
    return rows, columns
end

function _read_le_f64(path::String, rows::Int, columns::Int, expected_sha::String, label::String)
    bytes = try
        read(path)
    catch error
        _fail("missing_input", "$label could not be read: $(sprint(showerror, error))")
    end
    expected_bytes = rows * columns * sizeof(Float64)
    length(bytes) == expected_bytes ||
        _fail("invalid_input_length", "$label byte length does not match shape")
    bytes2hex(sha256(bytes)) == expected_sha ||
        _fail("input_digest_mismatch", "$label digest does not match request")
    native_words = reinterpret(UInt64, bytes)
    words = Base.ENDIAN_BOM == 0x04030201 ? native_words : ltoh.(native_words)
    values = reinterpret(Float64, words)
    all(isfinite, values) || _fail("non_finite_input", "$label contains non-finite values")
    return permutedims(reshape(values, columns, rows))
end

function _read_mask(path::String, rows::Int, columns::Int, expected_sha::String)
    bytes = try
        read(path)
    catch error
        _fail("missing_input", "protection mask could not be read: $(sprint(showerror, error))")
    end
    length(bytes) == rows * columns ||
        _fail("invalid_input_length", "protection mask byte length does not match shape")
    bytes2hex(sha256(bytes)) == expected_sha ||
        _fail("input_digest_mismatch", "protection mask digest does not match request")
    return BitMatrix(permutedims(reshape(bytes .!= 0x00, columns, rows)))
end

function _write_le_f64(path::String, matrix::Matrix{Float64})
    values = vec(permutedims(matrix))
    native_words = reinterpret(UInt64, values)
    words = Base.ENDIAN_BOM == 0x04030201 ? native_words : htol.(native_words)
    bytes = collect(reinterpret(UInt8, words))
    open(path, "w") do stream
        write(stream, bytes)
    end
    return bytes
end

function _profile(value)
    fields = Set([
        "key",
        "iterations",
        "stream_power",
        "glacial_strength",
        "ice_line",
        "lateral_widening",
        "talus_degrees",
        "cirque_strength",
        "planation",
    ])
    _exact_keys(value, fields, "profile")
    return ErosionProfile(
        _string(value["key"], "profile.key"),
        _integer(value["iterations"], "profile.iterations"),
        _float(value["stream_power"], "profile.stream_power"),
        _float(value["glacial_strength"], "profile.glacial_strength"),
        _float(value["ice_line"], "profile.ice_line"),
        _float(value["lateral_widening"], "profile.lateral_widening"),
        _float(value["talus_degrees"], "profile.talus_degrees"),
        _float(value["cirque_strength"], "profile.cirque_strength"),
        _float(value["planation"], "profile.planation"),
    )
end

function _report_object(report::ErosionReport)
    return Dict(
        "profile" => report.profile,
        "iterations" => report.iterations,
        "ice_line_m" => report.ice_line_m,
        "lateral_widening_cells" => report.lateral_widening_cells,
        "mean_lowering_m" => report.mean_lowering_m,
        "maximum_lowering_m" => report.maximum_lowering_m,
        "maximum_raising_m" => report.maximum_raising_m,
        "glacial_share" => report.glacial_share,
        "relief_before_m" => report.relief_before_m,
        "relief_after_m" => report.relief_after_m,
    )
end

function _base_response(operation::String, rows::Int, columns::Int)
    return Dict(
        "schema" => EROSION_RESULT_SCHEMA,
        "status" => "ok",
        "operation" => operation,
        "shape" => [rows, columns],
    )
end

function _handle(value)
    value isa JSON3.Object || _fail("malformed_request", "request must be an object")
    _string(get(value, "schema", nothing), "schema") == EROSION_REQUEST_SCHEMA ||
        _fail("unsupported_schema", "request schema is not supported")
    operation = _string(get(value, "operation", nothing), "operation")
    operation in ("erode", "thermal_erosion", "flux_field", "valley_cross_section") ||
        _fail("unsupported_operation", "worker operation is not supported")

    extras = if operation == "erode"
        Set([
            "output_path",
            "cell_m",
            "protect_path",
            "protect_sha256",
            "profile",
        ])
    elseif operation == "thermal_erosion"
        Set(["output_path", "cell_m", "talus_degrees", "rate"])
    elseif operation == "flux_field"
        Set(["output_path", "source_path", "source_sha256", "exponent"])
    else
        Set(["row_index"])
    end
    _exact_keys(value, union(_REQUEST_COMMON, extras), "request")
    rows, columns = _shape(value["shape"])
    height = _read_le_f64(
        _string(value["height_path"], "height_path"),
        rows,
        columns,
        _string(value["height_sha256"], "height_sha256"),
        "heightfield",
    )

    if operation == "erode"
        protect = nothing
        mask_path, mask_sha = value["protect_path"], value["protect_sha256"]
        if mask_path === nothing
            mask_sha === nothing || _fail("malformed_request", "mask digest requires a mask path")
        else
            mask_sha isa AbstractString || _fail("malformed_request", "mask digest must be a string")
            protect = _read_mask(
                _string(mask_path, "protect_path"),
                rows,
                columns,
                String(mask_sha),
            )
        end
        request = ErodeRequest(
            height,
            _float(value["cell_m"], "cell_m"),
            _profile(value["profile"]),
            protect,
        )
        outcome = erode_heightfield(request)
        output_path = _string(value["output_path"], "output_path")
        bytes = _write_le_f64(output_path, outcome.height)
        response = _base_response(operation, rows, columns)
        response["dtype"] = "float64-le"
        response["result_sha256"] = bytes2hex(sha256(bytes))
        response["report"] = _report_object(outcome.report)
        return response
    elseif operation == "thermal_erosion"
        request = ThermalErosionRequest(
            height,
            _float(value["cell_m"], "cell_m"),
            _float(value["talus_degrees"], "talus_degrees"),
            _float(value["rate"], "rate"),
        )
        output = thermal_erosion(request)
        bytes = _write_le_f64(_string(value["output_path"], "output_path"), output)
        response = _base_response(operation, rows, columns)
        response["dtype"] = "float64-le"
        response["result_sha256"] = bytes2hex(sha256(bytes))
        return response
    elseif operation == "flux_field"
        source_path, source_sha = value["source_path"], value["source_sha256"]
        source = if source_path === nothing
            source_sha === nothing || _fail("malformed_request", "source digest requires a source path")
            nothing
        else
            source_sha isa AbstractString || _fail("malformed_request", "source digest must be a string")
            _read_le_f64(
                _string(source_path, "source_path"),
                rows,
                columns,
                String(source_sha),
                "source field",
            )
        end
        request = FluxFieldRequest(height, source, _float(value["exponent"], "exponent"))
        output = flux_field(request)
        bytes = _write_le_f64(_string(value["output_path"], "output_path"), output)
        response = _base_response(operation, rows, columns)
        response["dtype"] = "float64-le"
        response["result_sha256"] = bytes2hex(sha256(bytes))
        return response
    end

    row_index = _integer(value["row_index"], "row_index")
    0 <= row_index < rows || _fail("invalid_row", "row_index is outside the heightfield")
    measured = valley_cross_section(height, row_index + 1)
    response = _base_response(operation, rows, columns)
    response["measurement"] = Dict(
        "form_ratio" => measured.form_ratio,
        "shape" => measured.shape,
        "relief_m" => measured.relief_m,
    )
    return response
end

function main()
    response = try
        _handle(JSON3.read(read(stdin, String)))
    catch error
        code = error isa ErosionWorkerError ? error.code : "invalid_request"
        Dict(
            "schema" => EROSION_RESULT_SCHEMA,
            "status" => "error",
            "error" => Dict("code" => code, "message" => sprint(showerror, error)),
        )
    end
    println(JSON3.write(response))
    return get(response, "status", "error") == "ok" ? 0 : 2
end

exit(main())
