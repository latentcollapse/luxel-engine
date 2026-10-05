# TetLab Spiral A1 — mesh I/O, deterministic. Stdlib only.
# Wavefront OBJ subset: v lines, f lines (1-based, / separators tolerated).
# We only ever read OBJs we wrote; writer emits canonical form.
module TetMeshIO

using SHA

export Mesh, write_obj, read_obj, mesh_sha256, tri_count, vert_count

struct Mesh
    V::Vector{NTuple{3,Float64}}   # vertices
    T::Vector{NTuple{3,Int}}       # triangles, 0-based indices into V
end

vert_count(m::Mesh) = length(m.V)
tri_count(m::Mesh)  = length(m.T)

# Canonical 18-digit fixed-point rendering -> parse-identical across runs.
function _fx(x::Float64)
    isfinite(x) || error("non-finite coordinate in OBJ write")
    s = sign(x)
    a = abs(x)
    ip = floor(Int, a)
    fp = round(Int, (a - ip) * 1e12)
    if fp == 1_000_000_000_000
        ip += 1
        fp = 0
    end
    return (s < 0 ? "-" : "") * string(ip) * "." * lpad(string(fp), 12, '0')
end

write_obj(path::String, m::Mesh) = open(path, "w") do io
    for v in m.V
        println(io, "v ", _fx(v[1]), " ", _fx(v[2]), " ", _fx(v[3]))
    end
    for t in m.T
        println(io, "f ", t[1] + 1, " ", t[2] + 1, " ", t[3] + 1)
    end
end

function read_obj(path::String)
    V = NTuple{3,Float64}[]
    T = NTuple{3,Int}[]
    for line in eachline(path)
        line = strip(line)
        if startswith(line, "v ")
            p = split(line)
            push!(V, (parse(Float64, p[2]), parse(Float64, p[3]), parse(Float64, p[4])))
        elseif startswith(line, "f ")
            p = split(line)
            idx = [parse(Int, first(split(tok, '/'))) for tok in p[2:end]]
            for k in 2:length(idx) - 1  # fan triangulation (we emit triangles only)
                push!(T, (idx[1] - 1, idx[k] - 1, idx[k + 1] - 1))
            end
        end
    end
    return Mesh(V, T)
end

# Canonical 64-char hex digest of the canonical OBJ text (not file bytes:
# CRLF/transport must not change mesh identity).
function mesh_sha256(path::String)
    io = IOBuffer()
    for line in eachline(path)
        println(io, strip(line))
    end
    return bytes2hex(sha256(take!(io)))
end

# Zero-padded sortable key (SOP: sortlex ≠ sortnum).
key6(i::Int)    = lpad(string(i), 6, '0')
key6f(x::Int)   = lpad(string(x), 6, '0')
key6i64(i::Int64) = lpad(string(i), 20, '0')

end # module
