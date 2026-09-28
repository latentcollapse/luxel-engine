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

const REQUIRED_OPTIONS = Set([
    "zone-spec",
    "asset-plan",
    "heightfield",
    "manifest",
    "zone-spec-sha256",
    "asset-plan-sha256",
    "output",
])

function main(values=ARGS)
    options = arguments(values)
    unknown = setdiff(Set(keys(options)), REQUIRED_OPTIONS)
    isempty(unknown) || error("unknown option(s): $(join(sort!(collect(unknown)), ", "))")
    all(haskey(options, name) for name in REQUIRED_OPTIONS) ||
        error("all placement solver options are required")
    plan = solve_landform_placements(
        options["zone-spec"],
        options["asset-plan"],
        options["heightfield"],
        options["manifest"];
        zone_spec_sha256=options["zone-spec-sha256"],
        asset_plan_sha256=options["asset-plan-sha256"],
    )
    write_placement_plan(options["output"], plan)
    println("Solved $(length(plan["placements"])) deterministic landform placements")
end

if abspath(PROGRAM_FILE) == abspath(@__FILE__)
    main()
end
