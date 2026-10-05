# Bench parent: spawns N fresh processes per variant, aggregates medians and
# spread. F-OPT.2 fix: only cross-process medians with reported spread are
# admissible evidence on this rig.
# Usage: julia --startup-file=no bench2.jl <label> <module_path> <N> [subdiv] [h]
using Printf

label  = ARGS[1]
modpath = ARGS[2]
N      = parse(Int, ARGS[3])
subdiv = length(ARGS) >= 5 ? parse(Int, ARGS[4]) : 6
h      = length(ARGS) >= 6 ? parse(Float64, ARGS[5]) : 0.28

builds = Float64[]; clips = Float64[]; allocs = Float64[]
hashes = String[]
for i in 1:N
    out = read(`julia --startup-file=no bench_child.jl $(label)_$i $modpath $subdiv $h`, String)
    for line in split(out, "\n")
        parts = split(strip(line), ",")
        # RESULT,<label>,<build_ms>,<clip_ms>,<alloc_mb>,<hash16>
        if length(parts) == 6 && parts[1] == "RESULT"
            push!(builds, parse(Float64, parts[3]))
            push!(clips,  parse(Float64, parts[4]))
            push!(allocs, parse(Float64, parts[5]))
            push!(hashes, parts[6])
        end
    end
end
isempty(builds) && error("no results collected")
length(unique(hashes)) == 1 || error("HASH DISAGREEMENT across children: $(unique(hashes))")
med(v) = (s = sort(v); s[ceil(Int, end/2)])
rel(v) = (maximum(v) - minimum(v)) / med(v) * 100
@printf("MEDIAN,%s,N=%d,build=%.2fms(+-%.0f%%),clip=%.2fms(+-%.0f%%),alloc=%.1fMB(+-%.0f%%),hash=%s\n",
        label, N,
        med(builds), rel(builds),
        med(clips),  rel(clips),
        med(allocs), rel(allocs),
        hashes[1])
