#!/usr/bin/env julia

using CodewealdTerrainLab

function arguments(values)
    result = Dict{String,String}()
    index = 1
    while index <= length(values)
        key = values[index]
        startswith(key, "--") || error("unexpected argument $key")
        index < length(values) || error("$key requires a value")
        name = key[3:end]
        isempty(name) && error("option name must not be empty")
        haskey(result, name) && error("duplicate option $key")
        result[name] = values[index + 1]
        index += 2
    end
    return result
end

const ALLOWED_OPTIONS = Set([
    "heightfield",
    "protected-mask",
    "semantic-region-mask",
    "manifest",
    "output",
    "maximum-accessible-grade",
    "steep-grade",
    "maximum-accessible-steep-fraction",
    "maximum-accessible-steep-world-fraction",
    "maximum-hydrology-uphill-fraction",
    "maximum-hydrology-uphill-step-m",
])

function main(values=ARGS)
    options = arguments(values)
    unknown = setdiff(Set(keys(options)), ALLOWED_OPTIONS)
    isempty(unknown) || error("unknown option(s): $(join(sort!(collect(unknown)), ", "))")
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
        maximum_accessible_steep_world_fraction=parse(
            Float64, get(options, "maximum-accessible-steep-world-fraction", "0.009")
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
end

if abspath(PROGRAM_FILE) == abspath(@__FILE__)
    main()
end
