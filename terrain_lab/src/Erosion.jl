const EROSION_REQUEST_SCHEMA = "codeweald.erosion-request/v1"
const EROSION_RESULT_SCHEMA = "codeweald.erosion-result/v1"

const _EROSION_NEIGHBORS = (
    (1, 0, 1.0),
    (-1, 0, 1.0),
    (0, 1, 1.0),
    (0, -1, 1.0),
    (1, 1, sqrt(2.0)),
    (1, -1, sqrt(2.0)),
    (-1, 1, sqrt(2.0)),
    (-1, -1, sqrt(2.0)),
)

struct ErosionWorkerError <: Exception
    code::String
    message::String
end

Base.showerror(io::IO, error::ErosionWorkerError) = print(io, error.message)

struct ErosionProfile
    key::String
    iterations::Int
    stream_power::Float64
    glacial_strength::Float64
    ice_line::Float64
    lateral_widening::Float64
    talus_degrees::Float64
    cirque_strength::Float64
    planation::Float64
end

struct ErodeRequest
    height::Matrix{Float64}
    cell_m::Float64
    profile::ErosionProfile
    protect::Union{Nothing,BitMatrix}
end

struct ThermalErosionRequest
    height::Matrix{Float64}
    cell_m::Float64
    talus_degrees::Float64
    rate::Float64
end

struct FluxFieldRequest
    height::Matrix{Float64}
    source::Union{Nothing,Matrix{Float64}}
    exponent::Float64
end

struct ErosionReport
    profile::String
    iterations::Int
    ice_line_m::Float64
    lateral_widening_cells::Int
    mean_lowering_m::Float64
    maximum_lowering_m::Float64
    maximum_raising_m::Float64
    glacial_share::Float64
    relief_before_m::Float64
    relief_after_m::Float64
end

struct ErosionOutcome
    height::Matrix{Float64}
    report::ErosionReport
end

function _erosion_require(condition::Bool, code::String, message::String)
    condition || throw(ErosionWorkerError(code, message))
    return nothing
end

function _validate_erosion_height(height::Matrix{Float64}; minimum_dimension::Int=1)
    rows, columns = size(height)
    _erosion_require(
        rows >= minimum_dimension && columns >= minimum_dimension,
        "invalid_shape",
        "heightfield dimensions are too small for this operation",
    )
    _erosion_require(all(isfinite, height), "non_finite_height", "heightfield contains non-finite values")
    return nothing
end

function _validate_profile(profile::ErosionProfile)
    values = (
        profile.stream_power,
        profile.glacial_strength,
        profile.ice_line,
        profile.lateral_widening,
        profile.talus_degrees,
        profile.cirque_strength,
        profile.planation,
    )
    _erosion_require(all(isfinite, values), "invalid_profile", "profile values must be finite")
    _erosion_require(profile.iterations > 0, "invalid_profile", "iterations must be positive")
    _erosion_require(0.0 <= profile.ice_line <= 1.0, "invalid_profile", "ice_line must be between zero and one")
    _erosion_require(profile.stream_power >= 0.0, "invalid_profile", "stream_power must not be negative")
    _erosion_require(profile.glacial_strength >= 0.0, "invalid_profile", "glacial_strength must not be negative")
    _erosion_require(profile.lateral_widening >= 0.0, "invalid_profile", "lateral_widening must not be negative")
    _erosion_require(profile.talus_degrees >= 0.0 && profile.talus_degrees < 90.0, "invalid_profile", "talus_degrees must be in [0, 90)")
    _erosion_require(profile.cirque_strength >= 0.0, "invalid_profile", "cirque_strength must not be negative")
    _erosion_require(0.0 <= profile.planation <= 1.0, "invalid_profile", "planation must be between zero and one")
    return nothing
end

function _erosion_shift(values::Matrix{Float64}, dr::Int, dc::Int)
    rows, columns = size(values)
    shifted = similar(values)
    for row in 1:rows, column in 1:columns
        source_row = clamp(row - dr, 1, rows)
        source_column = clamp(column - dc, 1, columns)
        shifted[row, column] = values[source_row, source_column]
    end
    return shifted
end

function flux_field(
    height::Matrix{Float64};
    source::Union{Nothing,Matrix{Float64}}=nothing,
    exponent::Float64=1.15,
)
    _validate_erosion_height(height)
    _erosion_require(isfinite(exponent) && exponent > 0.0, "invalid_exponent", "flux exponent must be finite and positive")
    if source !== nothing
        _erosion_require(size(source) == size(height), "invalid_source", "source and heightfield dimensions differ")
        _erosion_require(all(isfinite, source), "invalid_source", "source contains non-finite values")
        _erosion_require(all(value -> value >= 0.0, source), "invalid_source", "source contains negative values")
    end

    rows, columns = size(height)
    flux = source === nothing ? ones(Float64, rows, columns) : max.(source, 0.0)
    weights = Matrix{Float64}[]
    total = zeros(Float64, rows, columns)
    for (dr, dc, distance) in _EROSION_NEIGHBORS
        difference = max.((height .- _erosion_shift(height, dr, dc)) ./ distance, 0.0)
        weighted = difference .^ exponent
        push!(weights, weighted)
        total .+= weighted
    end
    safe = ifelse.(total .> 0.0, total, 1.0)
    shares = [weight ./ safe for weight in weights]

    # The Python implementation sorts the row-major flattened field and visits
    # high cells first. Explicit row-major indices preserve the graph traversal
    # independently of Julia's column-major matrix storage.
    order = collect(0:(rows * columns - 1))
    sort!(
        order;
        by=index -> (
            height[div(index, columns) + 1, mod(index, columns) + 1],
            -index,
        ),
        rev=true,
        alg=Base.Sort.MergeSort,
    )
    for index in order
        row, column = div(index, columns) + 1, mod(index, columns) + 1
        carried = flux[row, column]
        carried <= 0.0 && continue
        for (offset, (dr, dc, _distance)) in enumerate(_EROSION_NEIGHBORS)
            portion = shares[offset][row, column]
            portion <= 0.0 && continue
            destination_row, destination_column = row - dr, column - dc
            if 1 <= destination_row <= rows && 1 <= destination_column <= columns
                flux[destination_row, destination_column] += carried * portion
            end
        end
    end
    return flux
end

function thermal_erosion(
    height::Matrix{Float64};
    cell_m::Float64,
    talus_degrees::Float64,
    rate::Float64=0.35,
)
    _validate_erosion_height(height)
    _erosion_require(isfinite(cell_m) && cell_m > 0.0, "invalid_cell_size", "cell_m must be finite and positive")
    _erosion_require(
        isfinite(talus_degrees) && 0.0 <= talus_degrees < 90.0,
        "invalid_talus_angle",
        "talus_degrees must be finite and in [0, 90)",
    )
    _erosion_require(isfinite(rate) && rate >= 0.0, "invalid_rate", "thermal rate must be finite and non-negative")

    limit = tan(talus_degrees * (pi / 180.0)) * cell_m
    moved = zeros(Float64, size(height))
    excesses = Matrix{Float64}[]
    total = zeros(Float64, size(height))
    largest = zeros(Float64, size(height))
    for (dr, dc, distance) in _EROSION_NEIGHBORS
        difference = height .- _erosion_shift(height, dr, dc)
        excess = max.(difference .- limit .* distance, 0.0)
        push!(excesses, excess)
        total .+= excess
        largest .= max.(largest, excess)
    end
    safe = ifelse.(total .> 0.0, total, 1.0)
    amount = rate * 0.5 .* largest
    for (index, (dr, dc, _distance)) in enumerate(_EROSION_NEIGHBORS)
        share = amount .* (excesses[index] ./ safe)
        moved .-= share
        moved .+= _erosion_shift(share, -dr, -dc)
    end
    return height .+ moved
end

function _ice_source(height::Matrix{Float64}, ice_line_m::Float64)
    return max.(height .- ice_line_m, 0.0)
end

function _morphology(values::Matrix{Float64}, radius::Int, operation::Symbol)
    result = copy(values)
    for _ in 1:max(radius, 0)
        merged = copy(result)
        for (dr, dc, _distance) in _EROSION_NEIGHBORS[1:4]
            shifted = _erosion_shift(result, dr, dc)
            if operation === :minimum
                merged .= min.(merged, shifted)
            elseif operation === :maximum
                merged .= max.(merged, shifted)
            else
                throw(ErosionWorkerError("unsupported_operation", "unknown morphology operation"))
            end
        end
        result = merged
    end
    return result
end

function _widen(values::Matrix{Float64}, radius::Int)
    radius < 1 && return values
    rows, columns = size(values)
    window = 2 * radius + 1
    padded = Matrix{Float64}(undef, rows + 2 * radius, columns + 2 * radius)
    for row in axes(padded, 1), column in axes(padded, 2)
        padded[row, column] = values[
            clamp(row - radius, 1, rows),
            clamp(column - radius, 1, columns),
        ]
    end

    cumulative_rows = cumsum(padded; dims=1)
    row_sums = Matrix{Float64}(undef, rows, columns + 2 * radius)
    for row in 1:rows, column in axes(row_sums, 2)
        upper = row + window - 1
        lower_sum = row == 1 ? 0.0 : cumulative_rows[row - 1, column]
        row_sums[row, column] = cumulative_rows[upper, column] - lower_sum
    end

    cumulative_columns = cumsum(row_sums; dims=2)
    result = Matrix{Float64}(undef, rows, columns)
    for row in 1:rows, column in 1:columns
        right = column + window - 1
        left_sum = column == 1 ? 0.0 : cumulative_columns[row, column - 1]
        result[row, column] =
            (cumulative_columns[row, right] - left_sum) / Float64(window * window)
    end
    return result
end

function _erosion_gradient(height::Matrix{Float64}, cell_m::Float64)
    rows, columns = size(height)
    gradient_rows = Matrix{Float64}(undef, rows, columns)
    gradient_columns = Matrix{Float64}(undef, rows, columns)
    for row in 1:rows, column in 1:columns
        gradient_rows[row, column] = if row == 1
            (height[2, column] - height[1, column]) / cell_m
        elseif row == rows
            (height[rows, column] - height[rows - 1, column]) / cell_m
        else
            (height[row + 1, column] - height[row - 1, column]) / (2.0 * cell_m)
        end
        gradient_columns[row, column] = if column == 1
            (height[row, 2] - height[row, 1]) / cell_m
        elseif column == columns
            (height[row, columns] - height[row, columns - 1]) / cell_m
        else
            (height[row, column + 1] - height[row, column - 1]) / (2.0 * cell_m)
        end
    end
    return gradient_rows, gradient_columns
end

function erode_heightfield(request::ErodeRequest)::ErosionOutcome
    height = request.height
    profile = request.profile
    _validate_erosion_height(height; minimum_dimension=2)
    _validate_profile(profile)
    _erosion_require(
        isfinite(request.cell_m) && request.cell_m > 0.0,
        "invalid_cell_size",
        "cell_m must be finite and positive",
    )
    if request.protect !== nothing
        _erosion_require(
            size(request.protect) == size(height),
            "invalid_protection_mask",
            "protection mask and heightfield dimensions differ",
        )
    end

    original = copy(height)
    working = copy(original)
    floor_value, ceiling = extrema(original)
    relief = max(ceiling - floor_value, 1e-6)
    sorted_heights = sort!(collect(vec(original)))
    ice_line_m = _quantile(sorted_heights, profile.ice_line)
    side = max(size(height)...)
    widening = max(2, round(Int, profile.lateral_widening * side / 20.0))
    step_budget = relief * 0.30 / max(profile.iterations, 1)
    planation = clamp(profile.planation, 0.0, 0.999)
    glacial_rate = profile.glacial_strength *
        (1.0 - (1.0 - planation)^(1.0 / max(profile.iterations, 1)))

    glacial_total = 0.0
    fluvial_total = 0.0
    for _ in 1:profile.iterations
        gradient_rows, gradient_columns = _erosion_gradient(working, request.cell_m)
        slope = hypot.(gradient_rows, gradient_columns)

        water = flux_field(working)
        water_normalised = water ./ max(maximum(water), 1e-9)
        stream = profile.stream_power * step_budget .*
            (water_normalised .^ 0.45) .* (clamp.(slope, 0.0, 3.0) .^ 0.9)

        ice = flux_field(working; source=_ice_source(working, ice_line_m))
        ice_peak = maximum(ice)
        ice_normalised = ice_peak > 0.0 ? ice ./ ice_peak : ice
        corridor = Float64.(
            _morphology(Float64.(ice_normalised .> 0.02), widening, :maximum)
        )
        corridor = _widen(corridor, max(2, div(widening, 3)))
        thalweg = _morphology(working, widening + 4, :minimum)
        above_floor = max.(working .- thalweg, 0.0)
        glacier = glacial_rate .* corridor .* above_floor

        headward = clamp.((working .- ice_line_m) ./ (relief * 0.5), 0.0, 1.0)
        cirque = profile.cirque_strength * step_budget * 0.9 .*
            headward .* clamp.(ice_normalised .* 4.0, 0.0, 1.0) .*
            (1.0 .- ice_normalised)
        cirque = _widen(cirque, max(1, widening + 1))

        removed = stream .+ glacier .+ cirque
        if request.protect !== nothing
            removed[request.protect] .= 0.0
        end
        working = max.(working .- removed, floor_value - 2.0)
        glacial_total += sum(glacier)
        fluvial_total += sum(stream)

        working = thermal_erosion(
            working;
            cell_m=request.cell_m,
            talus_degrees=profile.talus_degrees,
        )
        if request.protect !== nothing
            working[request.protect] .= original[request.protect]
        end
    end

    for _ in 1:8
        working = thermal_erosion(
            working;
            cell_m=request.cell_m,
            talus_degrees=profile.talus_degrees,
            rate=0.5,
        )
        if request.protect !== nothing
            working[request.protect] .= original[request.protect]
        end
    end

    working = max.(working, original .- relief * 0.15)
    if request.protect !== nothing
        working[request.protect] .= original[request.protect]
    end

    delta = working .- original
    report = ErosionReport(
        profile.key,
        profile.iterations,
        round(ice_line_m; digits=2),
        widening,
        round(-sum(delta) / length(delta); digits=4),
        round(-minimum(delta); digits=3),
        round(maximum(delta); digits=3),
        round(glacial_total / max(glacial_total + fluvial_total, 1e-9); digits=4),
        round(relief; digits=2),
        round(maximum(working) - minimum(working); digits=2),
    )
    return ErosionOutcome(working, report)
end

function thermal_erosion(request::ThermalErosionRequest)
    return thermal_erosion(
        request.height;
        cell_m=request.cell_m,
        talus_degrees=request.talus_degrees,
        rate=request.rate,
    )
end

function flux_field(request::FluxFieldRequest)
    return flux_field(request.height; source=request.source, exponent=request.exponent)
end

function valley_cross_section(height::Matrix{Float64}, row::Int)
    _validate_erosion_height(height)
    _erosion_require(1 <= row <= size(height, 1), "invalid_row", "row index is outside the heightfield")
    line = @view height[row, :]
    low, high = extrema(line)
    if high - low < 1e-6
        return (form_ratio=0.0, shape="flat", relief_m=0.0)
    end
    near_floor = count(value -> value <= low + (high - low) * 0.25, line)
    near_rim = count(value -> value <= low + (high - low) * 0.75, line)
    ratio = near_floor / max(near_rim, 1e-9)
    return (
        form_ratio=round(ratio; digits=4),
        shape=ratio >= 0.48 ? "u" : "v",
        relief_m=round(high - low; digits=3),
    )
end
