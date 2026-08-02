#!/usr/bin/env julia

using CodewealdTerrainLab

function argument(name::String)
    index = findfirst(==(name), ARGS)
    index === nothing && error("missing $name")
    index < length(ARGS) || error("missing value after $name")
    return ARGS[index + 1]
end

plan = solve_landform_placements(
    argument("--zone-spec"),
    argument("--asset-plan"),
    argument("--heightfield"),
    argument("--manifest");
    zone_spec_sha256=argument("--zone-spec-sha256"),
    asset_plan_sha256=argument("--asset-plan-sha256"),
)
write_placement_plan(argument("--output"), plan)
println("Solved $(length(plan["placements"])) deterministic landform placements")
