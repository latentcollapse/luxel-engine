using CUDA
# EXACT copy of E's deform_corners! body style
function deform_corners!(ox, oy, oz, cx, cy, cz, n, s, fam, A, phi)
    i = (blockIdx().x - 1) * blockDim().x + threadIdx.x
    if i <= n
        x = cx[i]; y = cy[i]; z = cz[i]
        if fam == 1
            t = clamp(y / s, 0f0, 1f0)
            lean = A * s * t * t * sin(phi + 2.1f0 * x / s + 1.3f0 * z / s)
            ox[i] = x + lean; oy[i] = y; oz[i] = z
        elseif fam == 2
            θ = A * (y / s)
            c = cos(θ); sn = sin(θ)
            ox[i] = c * x - sn * y; oy[i] = sn * x + c * y; oz[i] = z
        else
            ox[i] = x; oy[i] = y; oz[i] = z + A * s * tanh(3f0 * y / s)
        end
    end
    return nothing
end
# my ÷/div + Int32-index additions, one at a time
function probe_div!(px, cx, idx, nV, ntot)
    i = (blockIdx().x - 1) * blockDim().x + threadIdx.x
    if i <= ntot
        g = div(i - 1, nV)
        j = i - g * nV
        i4 = 4 * (j - 1)
        a = idx[i4+1]
        px[i] = cx[a]
    end
    return nothing
end
thr = 256
nC = 4400
dox = CUDA.zeros(Float32, nC); dcy = CUDA.zeros(Float32, nC); dcz = CUDA.zeros(Float32, nC)
dcx = CUDA.ones(Float32, nC)
@cuda threads=thr blocks=cld(nC, thr) deform_corners!(dox, dcy, dcz, dcx, dcy, dcz, nC, 1f0, Int32(3), 0.4f0, 0.7f0)
CUDA.synchronize(); println("E-kernel: OK")
nV = 100; ntot = 200
px = CUDA.zeros(Float32, ntot); idx = CUDA.ones(Int32, 4*nV)
@cuda threads=thr blocks=cld(ntot, thr) probe_div!(px, dcx, idx, nV, ntot)
CUDA.synchronize(); println("probe_div (div + Int32 idx): OK")
