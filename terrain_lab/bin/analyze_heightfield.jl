#!/usr/bin/env julia

using CodewealdTerrainLab

function arguments(values)
    result = Dict{String,String}()
    index = 1
    while index <= length(values)
        key = values[index]
        startswith(key, "--") || error("unexpected argument $key")
        index < length(values) || error("$key requires a value")
        result[key[3:end]] = values[index + 1]
        index += 2
    end
    return result
end

options = arguments(ARGS)
for required in ("heightfield", "protected-mask", "semantic-region-mask", "manifest", "output")
    haskey(options, required) || error("--$required is required")
end

report = analyze_heightfield(
    options["heightfield"],
    options["protected-mask"],
    options["semantic-region-mask"],
    options["manifest"];
    maximum_accessible_grade=parse(
        Float64, get(options, "maximum-accessible-grade", "12.0")
    ),
    steep_grade=parse(Float64, get(options, "steep-grade", "2.0")),
    maximum_accessible_steep_fraction=parse(
        Float64, get(options, "maximum-accessible-steep-fraction", "0.01")
    ),
    maximum_hydrology_uphill_fraction=parse(
        Float64, get(options, "maximum-hydrology-uphill-fraction", "0.08")
    ),
    maximum_hydrology_uphill_step_m=parse(
        Float64, get(options, "maximum-hydrology-uphill-step-m", "0.35")
    ),
)
write_analysis(options["output"], report)
println(
    "Terrain analysis $(report["status"]): $(report["zone_id"]) " *
    "accessible p99=$(report["accessible"]["p99_grade"]) " *
    "max=$(report["accessible"]["maximum_grade"])",
)
exit(report["status"] == "passed" ? 0 : 1)
