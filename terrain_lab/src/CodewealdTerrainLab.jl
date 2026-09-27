module CodewealdTerrainLab

using JSON3
using SHA

export analyze_heightfield, solve_landform_placements, write_analysis, write_placement_plan
export EROSION_REQUEST_SCHEMA, EROSION_RESULT_SCHEMA
export ErosionProfile, ErodeRequest, ThermalErosionRequest, FluxFieldRequest
export ErosionOutcome, ErosionReport, ErosionWorkerError
export erode_heightfield, thermal_erosion, flux_field, valley_cross_section

include("Erosion.jl")

const ANALYSIS_SCHEMA = "codeweald.terrain-analysis/v1"
const REGION_PROTECTED_RELIEF = UInt8(1 << 0)
const REGION_TRAVERSABLE_LANDFORM = UInt8(1 << 1)
const REGION_LANE = UInt8(1 << 2)
const REGION_HYDROLOGY = UInt8(1 << 3)
const REGION_LANDMARK_PAD = UInt8(1 << 4)
const PLACEMENT_SCHEMA = "codeweald.placement-plan/v1"

mutable struct _DeterministicRng
    state::UInt64
end

function _next!(rng::_DeterministicRng)
    value = rng.state
    value ⊻= value << 13
    value ⊻= value >> 7
    value ⊻= value << 17
    rng.state = value == 0 ? UInt64(0x9e3779b97f4a7c15) : value
    return rng.state
end

_unit(rng::_DeterministicRng) = Float64(_next!(rng) >> 11) * 0x1.0p-53

function _feature_seed(generation_seed::Integer, feature_id::AbstractString)
    digest = sha256("$generation_seed:$feature_id")
    value = zero(UInt64)
    for byte in digest[1:8]
        value = (value << 8) | UInt64(byte)
    end
    return value == 0 ? UInt64(1) : value
end

function _read_object(path::AbstractString)
    value = JSON3.read(read(path, String))
    value isa JSON3.Object || error("$path must contain a JSON object")
    return value
end

function _read_heightfield(path::AbstractString, resolution::Int)
    bytes = read(path)
    expected = resolution * resolution * sizeof(Float32)
    length(bytes) == expected ||
        error("$path has $(length(bytes)) bytes; expected $expected")
    values = collect(reinterpret(Float32, bytes))
    all(isfinite, values) || error("$path contains non-finite heights")
    # Python writes rows in C order. Julia reshapes columns first, so transpose
    # the intermediate matrix to recover [row, column] terrain coordinates.
    return permutedims(reshape(values, resolution, resolution))
end

function _read_mask(path::AbstractString, resolution::Int)
    values = read(path)
    expected = resolution * resolution
    length(values) == expected ||
        error("$path has $(length(values)) bytes; expected $expected")
    return permutedims(reshape(values .!= 0x00, resolution, resolution))
end

function _read_regions(path::AbstractString, resolution::Int)
    values = read(path)
    expected = resolution * resolution
    length(values) == expected ||
        error("$path has $(length(values)) bytes; expected $expected")
    return permutedims(reshape(values, resolution, resolution))
end

function _quantile(sorted_values::Vector{Float64}, probability::Float64)
    isempty(sorted_values) && return 0.0
    position = clamp(probability, 0.0, 1.0) * (length(sorted_values) - 1) + 1
    lower = floor(Int, position)
    upper = ceil(Int, position)
    amount = position - lower
    return sorted_values[lower] * (1.0 - amount) + sorted_values[upper] * amount
end

"""
    _summary(grades; steep_grade, world_edge_count=nothing)

`steep_edge_fraction` is a share of this region's own edges, which makes it a
description of the region but **not a gate-able quantity**. Its denominator is
the region's size, and region size is a design variable: when the massif
inversion raised protected relief from 12% of the world to 50%, the walkable
set halved while the steep ground inside it did not move at all -- identical
valley floor, identical crags, same heights -- and the fraction rose from
passing to failing. The gate then asked for the ridge flanks to be smoothed to
correct an accounting change, which is a metric arguing against the art
direction it exists to protect (D16/D25).

So when `world_edge_count` is supplied, the summary also reports
`steep_edge_world_fraction`: the same steep edges over every edge in the world.
That denominator is fixed by resolution alone, so the number moves only when
the terrain moves, and it stays comparable between worlds with different
mountain/valley splits.
"""
function _summary(
    grades::Vector{Float64};
    steep_grade::Float64,
    world_edge_count::Union{Nothing,Int}=nothing,
)
    sort!(grades)
    sample_count = length(grades)
    steep_count = Base.count(>(steep_grade), grades)
    summary = Dict(
        "edge_count" => sample_count,
        "p50_grade" => round(_quantile(grades, 0.50), digits=6),
        "p95_grade" => round(_quantile(grades, 0.95), digits=6),
        "p99_grade" => round(_quantile(grades, 0.99), digits=6),
        "p999_grade" => round(_quantile(grades, 0.999), digits=6),
        "maximum_grade" => round(isempty(grades) ? 0.0 : grades[end], digits=6),
        "steep_grade_threshold" => steep_grade,
        "steep_edge_count" => steep_count,
        "steep_edge_fraction" => round(
            sample_count == 0 ? 0.0 : steep_count / sample_count,
            digits=8,
        ),
    )
    if world_edge_count !== nothing
        summary["world_edge_count"] = world_edge_count
        summary["steep_edge_world_fraction"] = round(
            world_edge_count == 0 ? 0.0 : steep_count / world_edge_count,
            digits=8,
        )
    end
    return summary
end

function _edge_region(left::UInt8, right::UInt8)
    combined = left | right
    combined & REGION_LANE != 0 && return "lane"
    combined & REGION_LANDMARK_PAD != 0 && return "landmark_pad"
    combined & REGION_HYDROLOGY != 0 && return "hydrology"
    combined & REGION_PROTECTED_RELIEF != 0 && return "protected_relief"
    combined & REGION_TRAVERSABLE_LANDFORM != 0 && return "traversable_landform"
    return "background"
end

function _grade_regions(
    height::Matrix{Float32},
    regions::Matrix{UInt8},
    spacing_x::Float64,
    spacing_z::Float64,
)
    grades = Dict(
        name => Float64[] for name in (
            "lane",
            "landmark_pad",
            "hydrology",
            "protected_relief",
            "traversable_landform",
            "background",
        )
    )
    rows, columns = size(height)
    size(regions) == size(height) || error("heightfield and region dimensions differ")
    for row in 1:rows
        for column in 1:(columns - 1)
            grade = abs(Float64(height[row, column + 1] - height[row, column])) / spacing_x
            name = _edge_region(regions[row, column], regions[row, column + 1])
            push!(grades[name], grade)
        end
    end
    for row in 1:(rows - 1)
        for column in 1:columns
            grade = abs(Float64(height[row + 1, column] - height[row, column])) / spacing_z
            name = _edge_region(regions[row, column], regions[row + 1, column])
            push!(grades[name], grade)
        end
    end
    return grades
end

function _sample_height(
    height::Matrix{Float32},
    width::Float64,
    world_length::Float64,
    x::Float64,
    z::Float64,
)
    rows, columns = size(height)
    u = clamp(x / width + 0.5, 0.0, 1.0) * (columns - 1)
    v = clamp(0.5 - z / world_length, 0.0, 1.0) * (rows - 1)
    column0, row0 = floor(Int, u) + 1, floor(Int, v) + 1
    column1, row1 = min(column0 + 1, columns), min(row0 + 1, rows)
    tx, tz = u - floor(u), v - floor(v)
    return (
        Float64(height[row0, column0]) * (1.0 - tx) * (1.0 - tz) +
        Float64(height[row0, column1]) * tx * (1.0 - tz) +
        Float64(height[row1, column0]) * (1.0 - tx) * tz +
        Float64(height[row1, column1]) * tx * tz
    )
end

function _point_in_polygon(x::Float64, z::Float64, polygon)
    inside = false
    count = length(polygon)
    count >= 3 || return false
    previous = count
    for current in 1:count
        xi, zi = Float64(polygon[current][1]), Float64(polygon[current][2])
        xj, zj = Float64(polygon[previous][1]), Float64(polygon[previous][2])
        if ((zi > z) != (zj > z)) &&
           (x < (xj - xi) * (z - zi) / (zj - zi) + xi)
            inside = !inside
        end
        previous = current
    end
    return inside
end

function _polygon_frame(polygon)
    xs = [Float64(point[1]) for point in polygon]
    zs = [Float64(point[2]) for point in polygon]
    center_x, center_z = sum(xs) / length(xs), sum(zs) / length(zs)
    dx, dz = xs .- center_x, zs .- center_z
    sxx = sum(dx .* dx) / length(dx)
    szz = sum(dz .* dz) / length(dz)
    sxz = sum(dx .* dz) / length(dx)
    angle = 0.5 * atan(2.0 * sxz, sxx - szz)
    axis_x, axis_z = cos(angle), sin(angle)
    if axis_x < 0.0 || (abs(axis_x) < 1e-9 && axis_z < 0.0)
        axis_x, axis_z = -axis_x, -axis_z
    end
    longs = dx .* axis_x .+ dz .* axis_z
    crosses = -dx .* axis_z .+ dz .* axis_x
    return (
        center_x=center_x,
        center_z=center_z,
        axis_x=axis_x,
        axis_z=axis_z,
        min_long=minimum(longs),
        max_long=maximum(longs),
        min_cross=minimum(crosses),
        max_cross=maximum(crosses),
        min_x=minimum(xs),
        max_x=maximum(xs),
        min_z=minimum(zs),
        max_z=maximum(zs),
    )
end

function _landform_assignment(asset_plan, feature_id::AbstractString)
    for assignment in asset_plan.assignments
        String(assignment.feature_id) == feature_id || continue
        String(assignment.role) == "landform_dressing" || continue
        return assignment
    end
    return nothing
end

"""
Produce concrete, terrain-grounded landform transforms from typed composition
contracts. Godot consumes these transforms; it must not reroll placement.
"""
function solve_landform_placements(
    zone_spec_path::AbstractString,
    asset_plan_path::AbstractString,
    heightfield_path::AbstractString,
    terrain_manifest_path::AbstractString;
    zone_spec_sha256::AbstractString,
    asset_plan_sha256::AbstractString,
)
    zone_spec = _read_object(zone_spec_path)
    asset_plan = _read_object(asset_plan_path)
    manifest = _read_object(terrain_manifest_path)
    resolution = Int(manifest.resolution)
    width = Float64(manifest.world_bounds_m.width)
    world_length = Float64(manifest.world_bounds_m.length)
    height = _read_heightfield(heightfield_path, resolution)
    generation_seed = Int(zone_spec.generation_seed)
    placements = Dict{String,Any}[]
    composition_counts = Dict{String,Int}()
    requires_dressing = false

    for feature in zone_spec.features
        hasproperty(feature, :category) || continue
        String(feature.category) == "landform" || continue
        hasproperty(feature, :generation) || continue
        generation = feature.generation
        hasproperty(generation, :composition) || continue
        feature_id = String(feature.id)
        assignment = _landform_assignment(asset_plan, feature_id)
        assignment === nothing && error("no landform asset assignment for $feature_id")
        polygon = feature.geometry.points
        frame = _polygon_frame(polygon)
        composition = generation.composition
        if hasproperty(composition, :dressing) &&
           String(composition.dressing) == "none"
            continue
        end
        requires_dressing = true
        spine_count = Int(composition.spine_count)
        elevation_bias = Float64(composition.elevation_bias)
        along_jitter = Float64(composition.along_jitter)
        cross_jitter = Float64(composition.cross_jitter)
        target_count = Int(assignment.instance_count)
        minimum_spacing = Float64(assignment.minimum_spacing_m)
        scales = assignment.scale_m
        assets = assignment.assets
        isempty(assets) && error("no resolved landform assets for $feature_id")
        rng = _DeterministicRng(_feature_seed(generation_seed, feature_id))
        cross_span = max(frame.max_cross - frame.min_cross, 1e-6)
        long_span = max(frame.max_long - frame.min_long, 1e-6)
        spine_offsets = [
            spine_count == 1 ? 0.0 :
            frame.min_cross + cross_span * (index - 0.5) / spine_count
            for index in 1:spine_count
        ]
        candidates = NamedTuple[]
        candidate_target = max(target_count * 300, 4000)
        attempts = 0
        while length(candidates) < candidate_target && attempts < candidate_target * 5
            attempts += 1
            x = frame.min_x + (frame.max_x - frame.min_x) * _unit(rng)
            z = frame.min_z + (frame.max_z - frame.min_z) * _unit(rng)
            _point_in_polygon(x, z, polygon) || continue
            relative_x, relative_z = x - frame.center_x, z - frame.center_z
            long = relative_x * frame.axis_x + relative_z * frame.axis_z
            cross = -relative_x * frame.axis_z + relative_z * frame.axis_x
            nearest_spine = minimum(abs(cross - offset) for offset in spine_offsets)
            ridge_sigma = max(cross_span / (spine_count * 4.5), minimum_spacing)
            ridge_affinity = exp(-0.5 * (nearest_spine / ridge_sigma)^2)
            phase = (long - frame.min_long) / long_span
            procession = 0.72 + 0.28 * abs(sin(phase * π * max(3, target_count ÷ spine_count)))
            ground = _sample_height(height, width, world_length, x, z)
            # Elevation remains a preference rather than a hard cutoff. Hard
            # cutoffs made narrow authored polygons unable to satisfy their
            # exact placement contract.
            elevation_score = 1.0 / (1.0 + exp(-ground * 0.18))
            score = (
                ridge_affinity * (1.0 - elevation_bias) +
                elevation_score * elevation_bias
            ) * procession
            score += (_unit(rng) - 0.5) * (along_jitter + cross_jitter) * 0.12
            push!(candidates, (score=score, x=x, z=z, ground=ground))
        end
        sort!(candidates, by=candidate -> candidate.score, rev=true)
        selected = NamedTuple[]
        for candidate in candidates
            all(
                hypot(candidate.x - prior.x, candidate.z - prior.z) >= minimum_spacing
                for prior in selected
            ) || continue
            push!(selected, candidate)
            length(selected) == target_count && break
        end
        length(selected) == target_count || error(
            "$feature_id placement contract requested $target_count but solver found $(length(selected))"
        )
        for (index, candidate) in enumerate(selected)
            asset = assets[mod1(index, length(assets))]
            scale = Float64(scales[1]) + (Float64(scales[2]) - Float64(scales[1])) * _unit(rng)
            push!(
                placements,
                Dict(
                    "id" => "$(feature_id)-$(lpad(index, 4, '0'))",
                    "feature_id" => feature_id,
                    "layer_id" => "primary",
                    "asset_sha256" => String(asset.sha256),
                    "position_m" => [
                        round(candidate.x, digits=6),
                        round(candidate.ground, digits=6),
                        round(candidate.z, digits=6),
                    ],
                    "ground_height_m" => round(candidate.ground, digits=6),
                    "yaw_degrees" => round(_unit(rng) * 360.0, digits=6),
                    "scale" => round(scale, digits=8),
                    "composition_pattern" => String(composition.pattern),
                ),
            )
        end
        composition_counts[String(composition.pattern)] = (
            get(composition_counts, String(composition.pattern), 0) + target_count
        )
    end
    isempty(placements) && requires_dressing &&
        error("ZoneSpec requested landform dressing but solver produced no placements")
    return Dict(
        "schema_version" => PLACEMENT_SCHEMA,
        "zone_spec_sha256" => zone_spec_sha256,
        "asset_plan_sha256" => asset_plan_sha256,
        "solver" => Dict(
            "name" => "CodewealdTerrainLab",
            "version" => "0.1.0",
            "generation_seed" => generation_seed,
            "scope" => "landform_dressing",
        ),
        "empty_reason" => (
            isempty(placements) ? "all_landforms_are_terrain_native" : nothing
        ),
        "composition_counts" => composition_counts,
        "placements" => placements,
    )
end

function _hydrology_summary(
    height::Matrix{Float32},
    width::Float64,
    world_length::Float64,
    centerline;
    sample_spacing::Float64,
    uphill_tolerance_m::Float64,
)
    channel_profile = hasproperty(centerline, :channel_profile) ?
        String(centerline.channel_profile) : "incised_stream"
    authored = [
        (Float64(point[1]), Float64(point[2]))
        for point in centerline.points
    ]
    length(authored) >= 2 || return Dict(
        "id" => String(centerline.id),
        "channel_profile" => channel_profile,
        "sample_count" => 0,
        "uphill_step_fraction" => 1.0,
        "maximum_uphill_step_m" => 0.0,
        "maximum_uphill_grade" => 0.0,
    )
    samples = Tuple{Float64,Float64}[]
    for (start, finish) in zip(authored[1:end-1], authored[2:end])
        dx, dz = finish[1] - start[1], finish[2] - start[2]
        distance = hypot(dx, dz)
        steps = max(1, ceil(Int, distance / sample_spacing))
        for index in 0:(steps - 1)
            amount = index / steps
            push!(samples, (start[1] + dx * amount, start[2] + dz * amount))
        end
    end
    push!(samples, authored[end])
    heights = [
        _sample_height(height, width, world_length, point[1], point[2])
        for point in samples
    ]
    # Authored polylines do not require a direction flag. Infer downstream as
    # the lower endpoint, then measure every local reversal in that direction.
    reversed_direction = heights[end] > heights[1]
    if reversed_direction
        reverse!(samples)
        reverse!(heights)
    end
    uphill_steps = Float64[]
    uphill_grades = Float64[]
    for index in 1:(length(samples) - 1)
        delta = heights[index + 1] - heights[index]
        distance = hypot(
            samples[index + 1][1] - samples[index][1],
            samples[index + 1][2] - samples[index][2],
        )
        if delta > uphill_tolerance_m
            push!(uphill_steps, delta)
            push!(uphill_grades, delta / max(distance, 1e-6))
        end
    end
    edge_count = max(0, length(samples) - 1)
    return Dict(
        "id" => String(centerline.id),
        "channel_profile" => channel_profile,
        "sample_count" => length(samples),
        "inferred_downstream_endpoint" => reversed_direction ? "first" : "last",
        "start_height_m" => round(heights[1], digits=5),
        "end_height_m" => round(heights[end], digits=5),
        "height_range_m" => round(maximum(heights) - minimum(heights), digits=5),
        "uphill_step_fraction" => round(
            edge_count == 0 ? 0.0 : length(uphill_steps) / edge_count,
            digits=6,
        ),
        "maximum_uphill_step_m" => round(
            isempty(uphill_steps) ? 0.0 : maximum(uphill_steps),
            digits=6,
        ),
        "maximum_uphill_grade" => round(
            isempty(uphill_grades) ? 0.0 : maximum(uphill_grades),
            digits=6,
        ),
    )
end

"""
Analyze a canonical Float32 little-endian heightfield. The protected mask marks
authored non-traversable relief, allowing steep Alpine silhouettes to be
reported separately from terrain the player can reasonably reach.
"""
function analyze_heightfield(
    heightfield_path::AbstractString,
    protected_mask_path::AbstractString,
    semantic_region_mask_path::AbstractString,
    manifest_path::AbstractString;
    maximum_accessible_grade::Float64=12.0,
    steep_grade::Float64=2.0,
    maximum_accessible_steep_fraction::Float64=0.01,
    maximum_accessible_steep_world_fraction::Float64=0.009,
    maximum_hydrology_uphill_fraction::Float64=0.08,
    maximum_hydrology_uphill_step_m::Float64=0.35,
)
    manifest = _read_object(manifest_path)
    resolution = Int(manifest.resolution)
    resolution >= 3 || error("terrain resolution must be at least 3")
    width = Float64(manifest.world_bounds_m.width)
    world_length = Float64(manifest.world_bounds_m.length)
    width > 0.0 && world_length > 0.0 ||
        error("terrain world bounds must be positive")
    height = _read_heightfield(heightfield_path, resolution)
    protected = _read_mask(protected_mask_path, resolution)
    regions = _read_regions(semantic_region_mask_path, resolution)
    protected == ((regions .& REGION_PROTECTED_RELIEF) .!= 0x00) ||
        error("protected mask does not match semantic protected-relief bits")
    grades = _grade_regions(
        height,
        regions,
        width / (resolution - 1),
        world_length / (resolution - 1),
    )
    accessible = reduce(
        vcat,
        (
            grades["lane"],
            grades["landmark_pad"],
            grades["traversable_landform"],
            grades["background"],
        ),
    )
    intentional = grades["protected_relief"]
    # Every edge in the world, whatever region claims it. Fixed by resolution,
    # so it does not move when the mountain/valley split does.
    world_edge_count = sum(length(values) for values in Base.values(grades); init=0)
    accessible_summary = _summary(
        accessible; steep_grade=steep_grade, world_edge_count=world_edge_count
    )
    intentional_summary = _summary(
        intentional; steep_grade=steep_grade, world_edge_count=world_edge_count
    )
    region_summaries = Dict(
        name => _summary(
            values; steep_grade=steep_grade, world_edge_count=world_edge_count
        )
        for (name, values) in grades
    )
    hydrology = hasproperty(manifest, :hydrology_centerlines) ? [
        _hydrology_summary(
            height,
            width,
            world_length,
            centerline;
            sample_spacing=max(width, world_length) / (resolution - 1) * 4.0,
            uphill_tolerance_m=0.05,
        )
        for centerline in manifest.hydrology_centerlines
    ] : Any[]
    failures = String[]
    if accessible_summary["maximum_grade"] > maximum_accessible_grade
        push!(
            failures,
            "accessible maximum grade $(accessible_summary["maximum_grade"]) exceeds $maximum_accessible_grade",
        )
    end
    # Gated against the world, not against the walkable set. See `_summary`:
    # the walkable set is a design variable, so gating on a share of it makes
    # the threshold move whenever the mountain/valley split moves, and asks the
    # author to flatten real terrain to correct an accounting change.
    #
    # 0.009 preserves the absolute allowance the previous rule granted -- 1% of
    # a world that was then ~87% walkable -- so this is the same strictness
    # expressed in a denominator that holds still.
    if accessible_summary["steep_edge_world_fraction"] >
       maximum_accessible_steep_world_fraction
        push!(
            failures,
            "accessible steep-edge world fraction $(accessible_summary["steep_edge_world_fraction"]) exceeds $maximum_accessible_steep_world_fraction",
        )
    end
    for stream in hydrology
        # A wetland rill is a map-space trace of saturated or standing water,
        # not a promise that the entire polyline is one continuously flowing
        # bed. Incised and surface channels retain the strict downhill gate.
        requires_downhill_flow = stream["channel_profile"] != "wetland_rill"
        if requires_downhill_flow &&
           stream["uphill_step_fraction"] > maximum_hydrology_uphill_fraction
            push!(
                failures,
                "hydrology $(stream["id"]) uphill-step fraction $(stream["uphill_step_fraction"]) exceeds $maximum_hydrology_uphill_fraction",
            )
        end
        if requires_downhill_flow &&
           stream["maximum_uphill_step_m"] > maximum_hydrology_uphill_step_m
            push!(
                failures,
                "hydrology $(stream["id"]) maximum uphill step $(stream["maximum_uphill_step_m"]) m exceeds $maximum_hydrology_uphill_step_m m",
            )
        end
    end
    return Dict(
        "schema_version" => ANALYSIS_SCHEMA,
        "status" => isempty(failures) ? "passed" : "failed",
        "zone_id" => String(manifest.zone_id),
        "zone_spec_sha256" => String(manifest.zone_spec_sha256),
        "resolution" => resolution,
        "world_bounds_m" => Dict("width" => width, "length" => world_length),
        "heightfield_sha256" => bytes2hex(sha256(read(heightfield_path))),
        "protected_relief_mask_sha256" => bytes2hex(sha256(read(protected_mask_path))),
        "semantic_region_mask_sha256" => bytes2hex(sha256(read(semantic_region_mask_path))),
        "protected_relief_fraction" => round(count(protected) / length(protected), digits=8),
        "policy" => Dict(
            "maximum_accessible_grade" => maximum_accessible_grade,
            "steep_grade" => steep_grade,
            # Reported for continuity with worlds analysed before the gate moved
            # to a world-relative denominator; no longer the gated quantity.
            "maximum_accessible_steep_fraction" => maximum_accessible_steep_fraction,
            "maximum_accessible_steep_world_fraction" =>
                maximum_accessible_steep_world_fraction,
            "maximum_hydrology_uphill_fraction" => maximum_hydrology_uphill_fraction,
            "maximum_hydrology_uphill_step_m" => maximum_hydrology_uphill_step_m,
            "wetland_rill_flow_policy" => "standing-or-braided; downhill continuity not required",
        ),
        "accessible" => accessible_summary,
        "intentional_relief" => intentional_summary,
        "regions" => region_summaries,
        "hydrology" => hydrology,
        "failures" => failures,
    )
end

function write_analysis(output_path::AbstractString, report)
    open(output_path, "w") do destination
        JSON3.pretty(destination, report)
        write(destination, '\n')
    end
    return report
end

function write_placement_plan(output_path::AbstractString, plan)
    open(output_path, "w") do destination
        JSON3.pretty(destination, plan)
        write(destination, '\n')
    end
    return plan
end

end
