#!/usr/bin/env julia
# CONVERGE-3 R-1 measured acceptance: the shape of the ruin wall end's top
# outline as the camera sees it.
#
# The wall-end triangles of a rendered packet (world space) are projected
# through the packet's camera; per screen column the topmost silhouette point
# and its world position are kept. Points on near-vertical silhouette segments
# (the block's side edges, steeper than `max_slope`) are dropped; what is left
# is the top outline. Reported: its length, its relief (height range) and its
# RMS deviation from the best straight line. An intact wall reads straight and
# flat; a broken one does not.
#
# Gate (restated 2026-10-05, see WGE_CONVERGE3_CONTRACTS.md R-1): relief >= 1.0 m
# and RMS from a line >= 0.2 m; the intact converge2 wall end is the control
# and must fail both.
#
# Usage:
#   julia --project=graphics_lab tools/ruin_outline_measure.jl PACKET.json [CONTROL_PACKET.json]
# where PACKET is a campaign2 close-view graphics_scene_packet.json.
# (Developed in a Palette session; copied here by hand, see Palette seam S-6.)

using JSON3
using LinearAlgebra

function rotate(q, v)
    x, y, z, w = q
    u = [x, y, z]
    t = 2 .* cross(u, v)
    return v .+ w .* t .+ cross(u, t)
end

"""World-space triangles of the ruin's wall end (not its fallen blocks)."""
function wall_end_triangles(body)
    meshes = Dict(m["mesh_id"] => m for m in body["meshes"])
    tris = Vector{Vector{Vector{Float64}}}()
    for i in body["instances"]
        startswith(i["instance_id"], "kit-ruin") || continue
        occursin("thick_end", i["mesh_id"]) && !occursin("fallen", i["mesh_id"]) || continue
        m = meshes[i["mesh_id"]]; t = i["transform"]
        world = [rotate(Float64.(t["rotation_xyzw"]), Float64.(v) .* Float64.(t["scale_xyz"])) .+ Float64.(t["translation_xyz_m"])
                 for v in m["positions_m"]]
        idx = Int.(m["indices"])
        for k in 1:3:length(idx)
            push!(tris, [world[idx[k]+1], world[idx[k+1]+1], world[idx[k+2]+1]])
        end
    end
    return tris
end

"""Topmost silhouette point per screen column: (columns, heights, world points)."""
function top_profile(body)
    cam = body["camera"]; tris = wall_end_triangles(body)
    Wp, Hp = cam["width_px"], cam["height_px"]
    P0 = Float64.(cam["position_xyz_m"]); Fw = normalize(Float64.(cam["forward_xyz"]))
    Rt = normalize(cross(Fw, Float64.(cam["up_xyz"]))); Up = cross(Rt, Fw)
    th = tand(cam["projection"]["fov_y_degrees"] / 2); asp = Wp / Hp
    proj(p) = (q = p .- P0; z = dot(q, Fw); ((dot(q, Rt) / (z * th * asp) + 1) / 2 * Wp, (1 - dot(q, Up) / (z * th)) / 2 * Hp))
    top_y = fill(Inf, Wp); top_p = [fill(NaN, 3) for _ in 1:Wp]
    for w in tris
        s = [proj(v) for v in w]; xs = first.(s)
        for c in max(0, floor(Int, minimum(xs))):min(Wp - 1, ceil(Int, maximum(xs)))
            x = c + 0.5
            for (i, j) in ((1, 2), (2, 3), (3, 1))
                (xi, yi), (xj, yj) = s[i], s[j]
                (xi - x) * (xj - x) <= 0 && xi != xj || continue
                t = (x - xi) / (xj - xi); y = yi + t * (yj - yi)
                y < top_y[c+1] && (top_y[c+1] = y; top_p[c+1] = w[i] .+ t .* (w[j] .- w[i]))
            end
        end
    end
    cols = findall(isfinite, top_y)
    return (h = [top_p[c][2] for c in cols], p = top_p[cols])
end

function outline_shape(prof; max_slope = 2.5)
    p, h = prof.p, prof.h; n = length(h)
    s = zeros(n)
    for k in 2:n
        s[k] = s[k-1] + norm(p[k][[1, 3]] .- p[k-1][[1, 3]])
    end
    keep = [k for k in 2:n-1 if abs(h[k+1] - h[k-1]) / max(s[k+1] - s[k-1], 1e-6) < max_slope]
    hs, ss = h[keep], s[keep]
    A = hcat(ones(length(ss)), ss)
    resid = hs .- A * (A \ hs)
    return (points = length(keep), length_m = ss[end] - ss[1], relief_m = maximum(hs) - minimum(hs),
            rms_from_line_m = sqrt(sum(resid .^ 2) / length(resid)))
end

passes(shape) = shape.relief_m >= 1.0 && shape.rms_from_line_m >= 0.2

function main(args)
    isempty(args) && error("usage: julia --project=graphics_lab tools/ruin_outline_measure.jl PACKET.json [CONTROL_PACKET.json]")
    report = Dict{String, Any}()
    for (label, path) in zip(("candidate", "control"), args)
        shape = outline_shape(top_profile(JSON3.read(read(path, String))["body"]))
        report[label] = Dict("relief_m" => round(shape.relief_m, digits = 3), "rms_from_line_m" => round(shape.rms_from_line_m, digits = 3),
                             "length_m" => round(shape.length_m, digits = 2), "points" => shape.points, "passes" => passes(shape))
    end
    ok = report["candidate"]["passes"] && (!haskey(report, "control") || !report["control"]["passes"])
    report["gate"] = ok ? "met" : "not met"
    JSON3.pretty(stdout, report); println()
    return ok
end

if abspath(PROGRAM_FILE) == @__FILE__
    exit(main(ARGS) ? 0 : 1)
end
