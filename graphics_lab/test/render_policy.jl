using Test
using JSON3
using GeometryBasics: Vec2f, Vec4f
using LuxelGraphics

include(joinpath(@__DIR__, "..", "src", "LavaAdapter.jl"))
using .LavaAdapter

# ---------------------------------------------------------------------------
# RenderPolicy — Julia side (graphical parity sprint §4)
#
# The load-bearing assertions here are:
#   1. an ABSENT policy yields the frozen-baseline defaults, and
#   2. the dither actually breaks the banding it was added to break.
# (1) is what keeps the baseline captures byte-identical; (2) is what stops
# the policy from being a decorative schema that no one can observe working.
# ---------------------------------------------------------------------------

@testset "render policy defaults reproduce the frozen baseline" begin
    policy = LuxelGraphics.RenderPolicy()
    @test policy.dither.amplitude_milli_lsb == 0
    @test policy.sampler.anisotropy == 1
    @test policy.terrain_surface.uv_repeat_scale_milli == 1000
    @test policy.terrain_surface.wrap_repeat == false
    @test policy.terrain_surface.macro_variation_bp == 0
    # 7500/1000 must reproduce the adapter's historical `0.25 + 0.75 * pcf`
    # with a one-texel tap spread. This is the load-bearing default: it is
    # what makes the shadow rework byte-identical when no policy is present.
    @test policy.shadow.darkness_bp == 7500
    @test policy.shadow.filter_radius_milli == 1000
    @test policy.bloom.intensity_bp == 0
    @test policy.vignette.strength_bp == 0
    @test policy.grade.gamma_bp == LuxelGraphics.POLICY_SCALE
    @test policy.grade.saturation_bp == LuxelGraphics.POLICY_SCALE
    @test policy.grade.lift_rgb_bp == (0, 0, 0)
    @test policy.grade.gain_rgb_bp == ntuple(_ -> LuxelGraphics.POLICY_SCALE, 3)
end

@testset "resolve policy lowering is the single audit point" begin
    lowered = LavaAdapter._resolve_policy(LuxelGraphics.RenderPolicy())
    @test lowered.grade_lift_rgb[1] == 0.0f0
    @test lowered.grade_gain_rgb[1] == 1.0f0
    @test lowered.grade_gamma_saturation[1] == 1.0f0
    @test lowered.grade_gamma_saturation[2] == 1.0f0
    @test lowered.bloom[2] == 0.0f0        # bloom off
    @test lowered.vignette[1] == 0.0f0     # vignette off
    @test lowered.dither[1] == 0.0f0       # dither off

    styled = LuxelGraphics.RenderPolicy(
        LuxelGraphics.GradePolicy((120, -60, 240), 9800, (10400, 10200, 10600), 9200),
        LuxelGraphics.BloomPolicy(8200, 1400),
        LuxelGraphics.VignettePolicy(2200, 6800, 3500),
        LuxelGraphics.DitherPolicy(1000),
        LuxelGraphics.TerrainSurfacePolicy(8000, true, 900, 55),
        LuxelGraphics.SamplerPolicy(8),
        LuxelGraphics.ShadowPolicy(9600, 2400),
    )
    low = LavaAdapter._resolve_policy(styled)
    @test low.grade_lift_rgb[1] ≈ 0.012f0 atol = 1.0f-6
    @test low.grade_gain_rgb[3] ≈ 1.06f0 atol = 1.0f-6
    @test low.grade_gamma_saturation[1] ≈ 0.98f0 atol = 1.0f-6
    @test low.grade_gamma_saturation[2] ≈ 0.92f0 atol = 1.0f-6
    @test low.bloom[1] ≈ 0.82f0 atol = 1.0f-6
    @test low.bloom[2] ≈ 0.14f0 atol = 1.0f-6
    @test low.vignette[1] ≈ 0.22f0 atol = 1.0f-6
    @test low.dither[1] ≈ 1.0f0 atol = 1.0f-6   # milli_lsb 1000 -> 1.0 LSB

    shadow_low = LavaAdapter._shadow_uniform(styled)
    @test shadow_low[1] ≈ 0.96f0 atol = 1.0f-6
    @test shadow_low[2] ≈ 2.4f0 atol = 1.0f-6   # milli 2400 -> 2.4 texels

    # Default lowering must be EXACTLY the historical arithmetic, not merely
    # close: 1.0f0 - 7500/10000 == 0.25f0 and 1000/1000 == 1.0f0.
    default_shadow = LavaAdapter._shadow_uniform(LuxelGraphics.RenderPolicy())
    @test default_shadow[1] === 0.75f0
    @test default_shadow[2] === 1.0f0
end

@testset "dither breaks the banding it exists to break" begin
    # A SHALLOW display-space ramp: 64 pixels spanning only ~4 display codes.
    # This is the real sky condition the baseline scan measured (959/959
    # identical adjacent pixels). A full 256-code ramp would NOT band — every
    # code maps to its own byte — so testing that shape would prove nothing.
    width = 64
    codes = 4
    linears = [
        LavaAdapter._srgb_to_linear(Float32(i) * (codes - 1) / ((width - 1) * 255))
        for i in 1:width
    ]

    plain = UInt8[
        LavaAdapter._rgba8((v, v, v, 1.0f0), 0.0f0, x, 0)[1] for (x, v) in enumerate(linears)
    ]
    dithered = UInt8[
        LavaAdapter._rgba8((v, v, v, 1.0f0), 1.0f0, x, 0)[1] for (x, v) in enumerate(linears)
    ]

    plain_repeats = count(i -> plain[i] == plain[i + 1], 1:(width - 1))
    dither_repeats = count(i -> dithered[i] == dithered[i + 1], 1:(width - 1))

    # The undithered ramp must exhibit the defect, or this gate is vacuous.
    @test plain_repeats > width * 0.6
    # The claim is RUN-LENGTH REDUCTION: adjacent identical pixels are the
    # banding, so the dither must decorrelate neighbours. A fixed ratio is not
    # claimed because the achievable reduction depends on how shallow the
    # ramp is; what must hold is a large, unambiguous reduction.
    @test dither_repeats < plain_repeats * 0.75
    # The dither must break banding WITHOUT changing tone: same endpoints and
    # a mean within half an LSB of the undithered mean. This is the property
    # that stops "fix banding" from silently meaning "add noise".
    @test minimum(dithered) >= minimum(plain) - 1
    @test maximum(dithered) <= maximum(plain) + 1
    @test abs(sum(Float64.(dithered)) / width - sum(Float64.(plain)) / width) <= 0.5
    # It must actually have changed pixels, not merely reordered them.
    @test count(i -> dithered[i] != plain[i], 1:width) > width * 0.2
    # No pixel may move by more than the amplitude allows (1 LSB).
    @test maximum(abs(Int(dithered[i]) - Int(plain[i])) for i in 1:width) <= 1
end

@testset "bayer pattern is deterministic, bounded, and centred" begin
    values = [LavaAdapter._bayer8x8(Int32(x), Int32(y)) for y in 0:7, x in 0:7]
    @test length(values) == 64
    @test minimum(values) >= -0.5f0
    @test maximum(values) < 0.5f0
    # The same coordinates must give the same value in any process/run: the
    # certified frame is a pure function of the packet, never of a counter.
    @test LavaAdapter._bayer8x8(Int32(3), Int32(5)) == LavaAdapter._bayer8x8(Int32(3), Int32(5))
    # And it must tile: an 8x8 period, so the pattern has no visible macro-grid.
    @test LavaAdapter._bayer8x8(Int32(0), Int32(0)) == LavaAdapter._bayer8x8(Int32(8), Int32(0))
end

@testset "dither never touches alpha" begin
    # artifact_rate counts non-opaque pixels; dithering alpha would make that
    # measurement meaningless.
    plain = LavaAdapter._rgba8((0.5f0, 0.5f0, 0.5f0, 0.5f0), 0.0f0, 0, 0)
    dithered = LavaAdapter._rgba8((0.5f0, 0.5f0, 0.5f0, 0.5f0), 2.0f0, 3, 5)
    @test dithered[4] == plain[4]
    @test dithered[1] != plain[1]
end

@testset "grade identity is a true identity" begin
    identity_lift = Vec4f(0.0f0, 0.0f0, 0.0f0, 0.0f0)
    identity_gain = Vec4f(1.0f0, 1.0f0, 1.0f0, 0.0f0)
    identity_gamma = Vec4f(1.0f0, 1.0f0, 0.0f0, 0.0f0)
    for value in (0.0f0, 0.05f0, 0.5f0, 0.93f0, 1.0f0)
        color = Vec4f(value, value, value, 1.0f0)
        graded = LavaAdapter._apply_grade(color, identity_lift, identity_gain, identity_gamma)
        @test graded[1] ≈ value atol = 1.0f-6
        @test graded[4] ≈ 1.0f0 atol = 1.0f-6
    end
end

@testset "grade identity is BIT-exact, not merely close" begin
    # `x ^ 1.0f0` goes through powf and is not guaranteed to return `x`
    # unchanged. Applying the default grade through it moved 6 bytes of the
    # frozen baseline capture by 1 LSB each, which broke the promise that an
    # absent policy reproduces the baseline exactly. Identity must therefore be
    # the SAME BIT PATTERN, checked over every one of the 256 quantised codes
    # rather than at a handful of round numbers.
    identity_lift = Vec4f(0.0f0, 0.0f0, 0.0f0, 0.0f0)
    identity_gain = Vec4f(1.0f0, 1.0f0, 1.0f0, 0.0f0)
    identity_gamma = Vec4f(1.0f0, 1.0f0, 0.0f0, 0.0f0)
    mismatches = 0
    for code in 0:255
        v = LavaAdapter._srgb_to_linear(Float32(code) / 255.0f0)
        color = Vec4f(v, v, v, 1.0f0)
        graded = LavaAdapter._apply_grade(color, identity_lift, identity_gain, identity_gamma)
        if reinterpret(UInt32, graded[1]) != reinterpret(UInt32, v)
            mismatches += 1
        end
    end
    @test mismatches == 0
end

@testset "grade saturation 0 is monochrome, 1 is identity" begin
    color = Vec4f(0.8f0, 0.4f0, 0.2f0, 1.0f0)
    lift = Vec4f(0.0f0, 0.0f0, 0.0f0, 0.0f0)
    gain = Vec4f(1.0f0, 1.0f0, 1.0f0, 0.0f0)
    mono = LavaAdapter._apply_grade(color, lift, gain, Vec4f(1.0f0, 0.0f0, 0.0f0, 0.0f0))
    @test mono[1] ≈ mono[2] ≈ mono[3]
    identity = LavaAdapter._apply_grade(color, lift, gain, Vec4f(1.0f0, 1.0f0, 0.0f0, 0.0f0))
    @test identity[1] ≈ 0.8f0 atol = 1.0f-6
    @test identity[2] ≈ 0.4f0 atol = 1.0f-6
end

@testset "vignette darkens edges and leaves the centre" begin
    color = Vec4f(0.5f0, 0.5f0, 0.5f0, 1.0f0)
    engaged = Vec4f(0.5f0, 0.6f0, 0.35f0, 0.0f0)  # strength, radius, softness
    centre = LavaAdapter._apply_vignette(color, engaged, Vec2f(0.5f0, 0.5f0))
    corner = LavaAdapter._apply_vignette(color, engaged, Vec2f(0.0f0, 0.0f0))
    @test centre[1] ≈ 0.5f0 atol = 1.0f-6          # centre untouched
    @test corner[1] < centre[1]                    # corner darkened
    # Off must be exactly off.
    off = LavaAdapter._apply_vignette(color, Vec4f(0.0f0, 0.6f0, 0.35f0, 0.0f0), Vec2f(0.0f0, 0.0f0))
    @test off[1] == color[1]
end

@testset "bloom is off by default and only samples when engaged" begin
    # `_apply_bloom` calls a Lava GPU intrinsic, so it cannot be executed on the
    # host (doing so segfaults inside the shader JIT). What IS host-testable is
    # the short-circuit predicate, and the default that keeps it false. Bloom's
    # actual appearance is verified by the fixed-camera A/B capture, not here.
    @test !LavaAdapter._bloom_engaged(LavaAdapter._resolve_policy(LuxelGraphics.RenderPolicy()).bloom)
    @test LavaAdapter._bloom_engaged(
        LavaAdapter._resolve_policy(LuxelGraphics.RenderPolicy(
            LuxelGraphics.GradePolicy(),
            LuxelGraphics.BloomPolicy(8200, 1400),
            LuxelGraphics.VignettePolicy(),
            LuxelGraphics.DitherPolicy(),
            LuxelGraphics.TerrainSurfacePolicy(),
            LuxelGraphics.SamplerPolicy(),
            LuxelGraphics.ShadowPolicy(),
        )).bloom,
    )
    # The threshold knee is pure arithmetic and must be host-testable.
    @test LavaAdapter._bloom_knee(0.5f0, 0.82f0) == 0.0f0
    @test LavaAdapter._bloom_knee(0.95f0, 0.82f0) ≈ 0.13f0 atol = 1.0f-6
end

@testset "policy parsing rejects out-of-range values" begin
    # Bounds are duplicated in Julia on purpose: a receiver must be able to
    # check a packet it did not produce, so validation cannot live only in the
    # producer.
    # `_parse_render_policy` takes the packet BODY and looks up
    # "render_policy" inside it, exactly as the packet validator does.
    policy_json(overrides) = JSON3.read(
        JSON3.write(Dict{String,Any}("render_policy" => overrides)),
    )

    @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(
        policy_json(Dict("dither" => Dict("amplitude_milli_lsb" => 9000))),
    )
    @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(
        policy_json(Dict("sampler" => Dict("anisotropy" => 0))),
    )
    @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(
        policy_json(Dict("terrain_surface" => Dict(
            "uv_repeat_scale_milli" => 0, "wrap_repeat" => false,
            "macro_variation_bp" => 0, "macro_frequency_milli" => 40,
        ))),
    )
    @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(
        policy_json(Dict("vignette" => Dict(
            "strength_bp" => 2000, "radius_bp" => 10000, "softness_bp" => 0,
        ))),
    )
    @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(
        policy_json(Dict("shadow" => Dict("darkness_bp" => 10001, "filter_radius_milli" => 1000))),
    )
    @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(
        policy_json(Dict("shadow" => Dict("darkness_bp" => 7500, "filter_radius_milli" => 99))),
    )
    @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(
        policy_json(Dict("grade" => Dict(
            "lift_r_bp" => -5000, "lift_g_bp" => 0, "lift_b_bp" => 0,
            "gamma_bp" => 10000,
            "gain_r_bp" => 10000, "gain_g_bp" => 10000, "gain_b_bp" => 10000,
            "saturation_bp" => 10000,
        ))),
    )

    # And a legal policy must parse.
    ok = LuxelGraphics._parse_render_policy(policy_json(Dict(
        "dither" => Dict("amplitude_milli_lsb" => 1000),
        "sampler" => Dict("anisotropy" => 8),
        "shadow" => Dict("darkness_bp" => 9600, "filter_radius_milli" => 2400),
    )))
    @test ok.dither.amplitude_milli_lsb == 1000
    @test ok.sampler.anisotropy == 8
    @test ok.shadow.darkness_bp == 9600
    @test ok.shadow.filter_radius_milli == 2400
    # Omitted sections take declared defaults, not errors.
    @test ok.grade.gamma_bp == LuxelGraphics.POLICY_SCALE
end

@testset "anisotropy refusal is driven by MEASURED capability" begin
    # The refusal must not be a hardcoded excuse. Given three different probed
    # capability profiles it must produce three DIFFERENT diagnoses, because
    # "the hardware cannot", "we could not tell", and "the device creation must
    # change" are different problems with different fixes.
    capable = LavaAdapter.GpuCapabilityProfile(
        "NVIDIA GeForce RTX 5060", 16.0f0, true, true, 1.0f0, UInt32(64), true)
    unsupported = LavaAdapter.GpuCapabilityProfile(
        "Some Old GPU", 1.0f0, false, false, 1.0f0, UInt32(64), true)
    unknown = LavaAdapter.UNKNOWN_CAPABILITY_PROFILE

    capable_detail = LavaAdapter._anisotropy_refusal_detail(Int32(8), capable)
    unsupported_detail = LavaAdapter._anisotropy_refusal_detail(Int32(8), unsupported)
    unknown_detail = LavaAdapter._anisotropy_refusal_detail(Int32(8), unknown)

    @test capable_detail != unsupported_detail
    @test capable_detail != unknown_detail
    @test unsupported_detail != unknown_detail

    # Hardware-capable: names the concrete measured limit AND the upstream fix.
    @test occursin("maxSamplerAnisotropy=16.0", capable_detail)
    @test occursin("SUPPORTS", capable_detail)
    # Hardware-limited: says so, and does NOT pretend it is a config problem.
    @test occursin("hardware limit", unsupported_detail)
    @test !occursin("pinned Lava revision", unsupported_detail)
    # Probe failed: says we do not know, rather than guessing.
    @test occursin("cannot tell", unknown_detail)

    # Every one of them must echo the requested value, so the packet that
    # failed can be identified from the error alone.
    for detail in (capable_detail, unsupported_detail, unknown_detail)
        @test occursin("anisotropy=8", detail)
    end
end

@testset "timestamp capability requires the limit that actually gates it" begin
    # Regression guard for F-1b: the old check tested only timestamp_period and
    # timestamp_valid_bits, and so reported GPU timing available on a device
    # where `timestampComputeAndGraphics` was false — meaning every gpu_*_us
    # would have been a decorative None even if the certification boundary did
    # not strip it.
    supports = LavaAdapter.GpuCapabilityProfile(
        "ok", 16.0f0, true, true, 1.0f0, UInt32(64), true)
    no_limit = LavaAdapter.GpuCapabilityProfile(
        "no-limit", 16.0f0, true, false, 1.0f0, UInt32(64), true)
    no_period = LavaAdapter.GpuCapabilityProfile(
        "no-period", 16.0f0, true, true, 0.0f0, UInt32(64), true)
    no_bits = LavaAdapter.GpuCapabilityProfile(
        "no-bits", 16.0f0, true, true, 1.0f0, UInt32(0), true)

    # Each of the three conditions must independently be sufficient to refuse.
    # Modeled as the same conjunction the probe applies.
    capable(p) = p.probe_complete && p.timestamp_compute_and_graphics &&
                 p.timestamp_period > 0.0f0 && p.timestamp_valid_bits > 0
    @test capable(supports)
    @test !capable(no_limit)
    @test !capable(no_period)
    @test !capable(no_bits)
    @test !capable(LavaAdapter.UNKNOWN_CAPABILITY_PROFILE)
end

@testset "terrain repeat wrap does NOT leak onto mesh surfaces (sprint F-4)" begin
    # THE load-bearing test for this change. Before the sampler-per-surface
    # split, both surfaces shared ONE sampler keyed by material id, so asking
    # terrain to tile silently changed mesh UV addressing at uv == 1.0 — the
    # exact contamination that made terrain tiling unimplementable.
    tiled = LuxelGraphics.RenderPolicy(
        LuxelGraphics.GradePolicy(), LuxelGraphics.BloomPolicy(), LuxelGraphics.VignettePolicy(),
        LuxelGraphics.DitherPolicy(), LuxelGraphics.TerrainSurfacePolicy(8000, true, 0, 40),
        LuxelGraphics.SamplerPolicy(), LuxelGraphics.ShadowPolicy(),
    )
    default = LuxelGraphics.RenderPolicy()

    terrain_spec = LavaAdapter._surface_sampler_spec(tiled, :terrain)
    mesh_spec = LavaAdapter._surface_sampler_spec(tiled, :mesh)

    @test terrain_spec.wrap == :repeat
    @test mesh_spec.wrap == :clamp
    @test terrain_spec != mesh_spec
    # And the two must be distinguishable as dict keys, which is what makes two
    # cached samplers possible for one material.
    d = Dict{LavaAdapter.SamplerSpec,Int}(terrain_spec => 1, mesh_spec => 2)
    @test length(d) == 2

    # Default policy stays clamped on BOTH surfaces, which is what keeps the
    # frozen baseline byte-identical.
    @test LavaAdapter._surface_sampler_spec(default, :terrain).wrap == :clamp
    @test LavaAdapter._surface_sampler_spec(default, :mesh).wrap == :clamp
    @test LavaAdapter._surface_sampler_spec(default, :terrain) ==
          LavaAdapter._surface_sampler_spec(default, :mesh)

    # An unknown surface kind must be refused, not silently treated as a mesh.
    @test_throws LavaAdapter.AdapterError LavaAdapter._surface_sampler_spec(tiled, :sky)

    # The repeat scale at its default is exactly 1.0f0, so the vertex-stage
    # multiplication is bit-exact identity. This is the byte-identity argument.
    @test Float32(default.terrain_surface.uv_repeat_scale_milli) / 1000.0f0 === 1.0f0
    @test Float32(tiled.terrain_surface.uv_repeat_scale_milli) / 1000.0f0 === 8.0f0
end

@testset "unsupported policy axes FAIL CLOSED instead of rendering unchanged" begin
    # The anti-decorative-schema rule: a policy the renderer cannot honour must
    # be refused loudly. If any of these ever returns normally, the receipt will
    # claim a policy took effect when it did not — which is exactly the failure
    # this sprint exists to prevent.
    @test LavaAdapter._assert_render_policy_supported(
        LuxelGraphics.RenderPolicy(), LavaAdapter.UNKNOWN_CAPABILITY_PROFILE,
    ) === nothing
    @test LavaAdapter._assert_render_policy_supported(LuxelGraphics.RenderPolicy(
        LuxelGraphics.GradePolicy(), LuxelGraphics.BloomPolicy(), LuxelGraphics.VignettePolicy(),
        LuxelGraphics.DitherPolicy(1000), LuxelGraphics.TerrainSurfacePolicy(),
        LuxelGraphics.SamplerPolicy(), LuxelGraphics.ShadowPolicy(9600, 2400),
    ), LavaAdapter.UNKNOWN_CAPABILITY_PROFILE) === nothing

    aniso_policy = LuxelGraphics.RenderPolicy(
        LuxelGraphics.GradePolicy(), LuxelGraphics.BloomPolicy(), LuxelGraphics.VignettePolicy(),
        LuxelGraphics.DitherPolicy(), LuxelGraphics.TerrainSurfacePolicy(),
        LuxelGraphics.SamplerPolicy(8), LuxelGraphics.ShadowPolicy(),
    )
    # Wrapped in a thunk: calling it eagerly would throw outside the assertion.
    # The profile is the hardware-capable one, so the refusal must be the
    # "device creation must change" diagnosis.
    capable = LavaAdapter.GpuCapabilityProfile(
        "NVIDIA GeForce RTX 5060", 16.0f0, true, true, 1.0f0, UInt32(64), true)
    aniso_error = try
        LavaAdapter._assert_render_policy_supported(aniso_policy, capable)
        nothing
    catch e
        e
    end
    @test aniso_error isa LavaAdapter.AdapterError
    # The refusal must name the axis AND the reason, or it is not actionable.
    @test occursin("anisotropy", aniso_error.detail)
    @test occursin("maxSamplerAnisotropy=16.0", aniso_error.detail)

    terrain_policy = LuxelGraphics.RenderPolicy(
        LuxelGraphics.GradePolicy(), LuxelGraphics.BloomPolicy(), LuxelGraphics.VignettePolicy(),
        LuxelGraphics.DitherPolicy(), LuxelGraphics.TerrainSurfacePolicy(8000, true, 700, 40),
        LuxelGraphics.SamplerPolicy(), LuxelGraphics.ShadowPolicy(),
    )
    terrain_error = try
        LavaAdapter._assert_render_policy_supported(terrain_policy, capable)
        nothing
    catch e
        e
    end
    @test terrain_error isa LavaAdapter.AdapterError
    @test occursin("macro_variation_bp", terrain_error.detail)

    # The axes that ARE implemented must NOT be refused any more. This is the
    # regression guard for F-4: a tiling policy that still threw would mean the
    # sampler split did not land.
    tiling_ok = LuxelGraphics.RenderPolicy(
        LuxelGraphics.GradePolicy(), LuxelGraphics.BloomPolicy(), LuxelGraphics.VignettePolicy(),
        LuxelGraphics.DitherPolicy(), LuxelGraphics.TerrainSurfacePolicy(8000, true, 0, 40),
        LuxelGraphics.SamplerPolicy(), LuxelGraphics.ShadowPolicy(),
    )
    @test LavaAdapter._assert_render_policy_supported(tiling_ok, capable) === nothing

    # Non-boolean wrap_repeat must be refused rather than coerced: Julia's
    # `Bool(1)` is `true`, which would let the receiver disagree with Rust.
    bad_wrap = try
        LuxelGraphics._parse_render_policy(JSON3.read(JSON3.write(Dict{String,Any}(
            "render_policy" => Dict("terrain_surface" => Dict(
                "uv_repeat_scale_milli" => 8000, "wrap_repeat" => 1,
                "macro_variation_bp" => 0, "macro_frequency_milli" => 40,
            )),
        ))))
        nothing
    catch e
        e
    end
    @test bad_wrap isa LuxelGraphics.ProtocolError
    @test occursin("not a JSON boolean", bad_wrap.detail)
end

@testset "an absent policy section is not an error" begin
    empty_body = JSON3.read("{}")
    policy = LuxelGraphics._parse_render_policy(empty_body)
    @test policy.dither.amplitude_milli_lsb == 0
    @test policy.shadow.darkness_bp == 7500
    @test policy == LuxelGraphics.RenderPolicy()
end

# ---------------------------------------------------------------------------
# CONVERGE-0 axes: mesh repeat wrap, view-relative shadow fit, view sky.
# ---------------------------------------------------------------------------

@testset "unknown render policy keys fail closed (CONVERGE-0)" begin
    policy_json(overrides) = JSON3.read(JSON3.write(Dict{String,Any}("render_policy" => overrides)))
    # An axis this worker does not know must be refused, not ignored.
    @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(
        policy_json(Dict("volumetrics" => Dict("density_bp" => 100))),
    )
    # So must an unknown field inside a known axis.
    @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(
        policy_json(Dict("dither" => Dict("amplitude_milli_lsb" => 1000, "pattern" => "blue"))),
    )
    @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(
        policy_json(Dict("sky" => Dict("sun_disc_radius_milli_deg" => 650))),
    )
    @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(
        policy_json(Dict("shadow_fit" => Dict("view_distance_m" => 7))),
    )
    @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(
        policy_json(Dict("mesh_surface" => Dict("wrap_repeat" => 1))),
    )
    parsed = LuxelGraphics._parse_render_policy(policy_json(Dict(
        "mesh_surface" => Dict("wrap_repeat" => true),
        "shadow_fit" => Dict("view_distance_m" => 60),
        "sky" => Dict("sun_disc_radius_milli_deg" => 650, "sun_disc_gain_bp" => 120000, "sun_glow_gain_bp" => 2400),
    )))
    @test parsed.mesh_surface.wrap_repeat
    @test parsed.shadow_fit.view_distance_m == 60
    @test parsed.sky.sun_disc_gain_bp == 120000
    # Absent axes keep their absent meaning.
    absent = LuxelGraphics.RenderPolicy()
    @test absent.shadow_fit === nothing
    @test absent.sky === nothing
    @test absent.mesh_surface.wrap_repeat == false
end

@testset "mesh repeat wrap is its own axis (CONVERGE-0)" begin
    wrapped = LuxelGraphics.RenderPolicy(
        LuxelGraphics.GradePolicy(), LuxelGraphics.BloomPolicy(), LuxelGraphics.VignettePolicy(),
        LuxelGraphics.DitherPolicy(), LuxelGraphics.TerrainSurfacePolicy(),
        LuxelGraphics.SamplerPolicy(), LuxelGraphics.ShadowPolicy(),
        LuxelGraphics.MeshSurfacePolicy(true), nothing, nothing,
    )
    @test LavaAdapter._surface_sampler_spec(wrapped, :mesh).wrap == :repeat
    # Mesh wrap must not leak onto terrain either (the F-4 split, reversed).
    @test LavaAdapter._surface_sampler_spec(wrapped, :terrain).wrap == :clamp
end

@testset "view-fitted shadow frame bounds the frustum slice (CONVERGE-0)" begin
    camera = LuxelGraphics.CameraPacket(
        "fit-camera",
        LuxelGraphics.PerspectiveProjection(50.0f0),
        (54.5f0, 26.0f0, 49.0f0),
        (-0.6f0, -0.2f0, -0.77f0),
        (0.0f0, 1.0f0, 0.0f0),
        0.1f0,
        1500.0f0,
        UInt32(960),
        UInt32(640),
    )
    light = LavaAdapter._normalize_vector(Vec4f(1.6407f0, -1.0f0, -1.381f0, 0.0f0))
    light_direction, right, up = LavaAdapter._basis_vectors(
        light, LavaAdapter._shadow_up_hint(light), "test light basis")
    frame = LavaAdapter._view_fitted_shadow_frame(camera, light_direction, right, up, 60.0f0)
    @test frame.mode == 0.0f0
    half_span = frame.projection[1]
    # Every frustum-slice corner must project inside the orthographic extent.
    view = LavaAdapter._camera_frame(camera)
    for depth in (view.projection[3], 60.0f0), sx in (-1.0f0, 1.0f0), sy in (-1.0f0, 1.0f0)
        corner = Vec4f(
            (view.position .+ view.forward .* depth .+ view.right .* (sx * view.projection[1] * depth) .+ view.up .* (sy * view.projection[2] * depth))[1:3]...,
            1.0f0,
        )
        ndc = LavaAdapter._project_world(
            corner, frame.position, frame.right, frame.up, frame.forward, frame.projection, frame.mode)
        @test abs(ndc[1]) <= 1.0f0
        @test abs(ndc[2]) <= 1.0f0
        @test 0.0f0 <= ndc[3] <= 1.0f0
    end
    # Resolution is independent of world size: ~4 texels/m at 60 m.
    texels_per_metre = 512.0f0 / (2.0f0 * half_span)
    @test texels_per_metre > 3.5f0
    # And the fit refuses a distance that does not reach past the near plane.
    @test_throws LavaAdapter.AdapterError LavaAdapter._view_fitted_shadow_frame(
        camera, light_direction, right, up, 0.05f0)
end

# ---------------------------------------------------------------------------
# N-4 layered terrain: schema, uniform lowering, and the macro policy rule.
# ---------------------------------------------------------------------------

@testset "terrain layer schema fails closed (N-4)" begin
    materials = [
        LuxelGraphics.MaterialPacket(
            id, (1.0f0, 1.0f0, 1.0f0, 1.0f0), 0.0f0, 1.0f0, 0.0f0, 0.5f0, :opaque,
            String[], nothing, nothing, nothing, nothing, 1.0f0, 1.0f0, (0.0f0, 0.0f0, 0.0f0),
        ) for id in ("ground-mat", "rock-mat")
    ]
    layer(id, material, coverage) = coverage === nothing ?
        Dict("layer_id" => id, "material_id" => material, "metres_per_repeat_milli" => 2000) :
        Dict("layer_id" => id, "material_id" => material, "metres_per_repeat_milli" => 3000, "coverage" => coverage)
    doc(layers; extra=Dict()) = JSON3.read(JSON3.write(merge(Dict(
        "set_id" => "s", "set_sha256" => "sha256:" * repeat("a", 64), "macro_texture_id" => "m", "layers" => layers,
    ), extra)))
    good = doc([layer("ground", "ground-mat", nothing), layer("rock", "rock-mat", Dict("slope_bp" => [450, 850]))])
    parsed = LuxelGraphics._parse_terrain_layers(good, materials)
    @test length(parsed.layers) == 2
    @test parsed.layers[2].coverage.slope_bp == (450, 850)
    @test parsed.layers[2].coverage.height_mm === nothing
    bad = [
        doc([layer("ground", "ground-mat", nothing)]),                                          # one layer
        doc([layer("ground", "ground-mat", Dict("slope_bp" => [1, 2])), layer("rock", "rock-mat", Dict("slope_bp" => [450, 850]))]),  # base covered
        doc([layer("ground", "ground-mat", nothing), layer("rock", "rock-mat", Dict())]),      # empty coverage
        doc([layer("ground", "ground-mat", nothing), layer("rock", "rock-mat", Dict("slope_bp" => [450, 450]))]),  # zero-width ramp
        doc([layer("ground", "ground-mat", nothing), layer("rock", "missing-mat", Dict("slope_bp" => [450, 850]))]),  # unknown material
        doc([layer("ground", "ground-mat", nothing), layer("rock", "rock-mat", Dict("slope_bp" => [450, 850]))]; extra=Dict("tint" => 1)),  # unknown key
    ]
    for case in bad
        @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_terrain_layers(case, materials)
    end
end

@testset "layered terrain uniforms encode absent terms and padding (N-4)" begin
    ground = LuxelGraphics.TerrainLayerPacket("ground", "g", UInt32(2000), nothing)
    rock = LuxelGraphics.TerrainLayerPacket(
        "rock", "r", UInt32(3000), LuxelGraphics.TerrainLayerCoverage((Int32(450), Int32(850)), nothing, nothing))
    policy = LuxelGraphics.TerrainSurfacePolicy(1000, true, 2000, 15)
    u = LavaAdapter._terrain_layer_uniforms([ground, rock], Float32[1.0, 0.8], policy)
    @test u[1][1] ≈ 0.5f0          # 1 / 2 m
    @test u[3][1] ≈ 1.0f0 / 3.0f0  # 1 / 3 m
    @test u[4][4] ≈ 0.8f0          # rock normal scale
    @test u[7][1] ≈ 0.015f0 && u[7][2] ≈ 0.2f0
    weight(a, b, slope, height, m) = LavaAdapter._layer_weight(a, b, Float32(slope), Float32(height), Float32(m))
    # Rock: slope ramp only; height and macro terms absent => 1.
    @test weight(u[3], u[4], 0.02, 15.0, 0.3) == 0.0f0
    @test weight(u[3], u[4], 0.10, 15.0, 0.3) == 1.0f0
    @test 0.0f0 < weight(u[3], u[4], 0.065, -1000.0, 0.99) < 1.0f0
    # Two-layer set: the third slot is padding and must never paint.
    for slope in (0.0, 0.5, 1.0)
        @test weight(u[5], u[6], slope, 20.0, 0.5) == 0.0f0
    end
    # A falling ramp (a > b) is a valid "below" rule.
    @test LavaAdapter._ramp(17.5f0, 14.5f0, 12.0f0) == 1.0f0
    @test LavaAdapter._ramp(17.5f0, 14.5f0, 20.0f0) == 0.0f0
end

@testset "macro variation runs only on layered terrain (N-4)" begin
    caps = LavaAdapter.GpuCapabilityProfile("NVIDIA GeForce RTX 5060", 16.0f0, true, true, 1.0f0, UInt32(64), true)
    macro_policy = LuxelGraphics.RenderPolicy(
        LuxelGraphics.GradePolicy(), LuxelGraphics.BloomPolicy(), LuxelGraphics.VignettePolicy(),
        LuxelGraphics.DitherPolicy(), LuxelGraphics.TerrainSurfacePolicy(1000, true, 2000, 15),
        LuxelGraphics.SamplerPolicy(), LuxelGraphics.ShadowPolicy(),
    )
    @test_throws LavaAdapter.AdapterError LavaAdapter._assert_render_policy_supported(macro_policy, caps)
    @test LavaAdapter._assert_render_policy_supported(macro_policy, caps; layered_terrain=true) === nothing
    clamped = LuxelGraphics.RenderPolicy(
        LuxelGraphics.GradePolicy(), LuxelGraphics.BloomPolicy(), LuxelGraphics.VignettePolicy(),
        LuxelGraphics.DitherPolicy(), LuxelGraphics.TerrainSurfacePolicy(1000, false, 0, 40),
        LuxelGraphics.SamplerPolicy(), LuxelGraphics.ShadowPolicy(),
    )
    @test_throws LavaAdapter.AdapterError LavaAdapter._assert_render_policy_supported(clamped, caps; layered_terrain=true)
end

# ---------------------------------------------------------------------------
# CONVERGE-1 N-1: analytic sky and exponential height-dependent atmosphere.
# ---------------------------------------------------------------------------

const N1_POLICY_JSON(overrides) = JSON3.read(JSON3.write(Dict{String,Any}("render_policy" => overrides)))
const N1_SKY = Dict{String,Any}("sun_disc_radius_milli_deg" => 650, "sun_disc_gain_bp" => 120000, "sun_glow_gain_bp" => 2400)
const N1_AIR = Dict{String,Any}("height_falloff_milli_per_m" => 10, "density_at_ground_bp" => 70, "sun_scatter_gain_bp" => 300)
# converge0 content: environment and the 25°-elevation key light.
const N1_ENVIRONMENT = LuxelGraphics.EnvironmentPacket(
    (0.16f0, 0.30f0, 0.58f0), (0.62f0, 0.66f0, 0.72f0), (0.11f0, 0.12f0, 0.085f0),
    (0.52f0, 0.58f0, 0.67f0), 0.0011f0, 1.08f0,
)
const N1_LIGHT = LavaAdapter.DirectionalLighting(Vec4f(1.6407f0, -1.0f0, -1.381f0, 0.0f0), Vec4f(1.0f0, 0.82f0, 0.62f0, 1.0f0), 5.0f0)
const N1_SUN = LavaAdapter._normalize_vector(Vec4f(-1.6407f0, 1.0f0, 1.381f0, 0.0f0))
const N1_TOP = Vec4f(N1_ENVIRONMENT.sky_top_rgb..., 1.0f0)
const N1_HORIZON = Vec4f(N1_ENVIRONMENT.sky_horizon_rgb..., 1.0f0)
const N1_GROUND = Vec4f(N1_ENVIRONMENT.ground_rgb..., 1.0f0)
const N1_ZERO = Vec4f(0.0f0, 0.0f0, 0.0f0, 0.0f0)

n1_sky_with(model) = merge(N1_SKY, Dict{String,Any}("model" => model))
n1_luminance(c) = 0.2126f0 * c[1] + 0.7152f0 * c[2] + 0.0722f0 * c[3]

function n1_analytic_parameters(turbidity_milli=3000)
    policy = LuxelGraphics._parse_render_policy(N1_POLICY_JSON(Dict(
        "sky" => n1_sky_with(Dict("kind" => "analytic", "turbidity_milli" => turbidity_milli)),
    )))
    return LavaAdapter._sky_parameters(policy, N1_LIGHT, N1_ENVIRONMENT)
end

n1_sky(direction, sky=n1_analytic_parameters()) = LavaAdapter._analytic_sky(direction, N1_SUN, sky, N1_TOP, N1_HORIZON)

@testset "N-1 sky model and atmosphere parse and fail closed" begin
    analytic = LuxelGraphics._parse_render_policy(N1_POLICY_JSON(Dict(
        "sky" => n1_sky_with(Dict("kind" => "analytic", "turbidity_milli" => 3000)),
        "atmosphere" => N1_AIR,
    )))
    @test analytic.sky.model === :analytic
    @test analytic.sky.turbidity_milli == 3000
    @test analytic.atmosphere.height_falloff_milli_per_m == 10
    @test analytic.atmosphere.density_at_ground_bp == 70
    @test analytic.atmosphere.sun_scatter_gain_bp == 300
    # Absent model and explicit gradient are the same CONVERGE-0 sky.
    absent_model = LuxelGraphics._parse_render_policy(N1_POLICY_JSON(Dict("sky" => N1_SKY)))
    explicit = LuxelGraphics._parse_render_policy(N1_POLICY_JSON(Dict("sky" => n1_sky_with(Dict("kind" => "gradient")))))
    @test absent_model.sky == explicit.sky
    @test absent_model.sky.model === :gradient
    @test absent_model.atmosphere === nothing
    @test LuxelGraphics.RenderPolicy().atmosphere === nothing
    # k = 0 is a homogeneous haze, accepted.
    @test LuxelGraphics._parse_render_policy(N1_POLICY_JSON(Dict(
        "sky" => N1_SKY, "atmosphere" => merge(N1_AIR, Dict("height_falloff_milli_per_m" => 0)),
    ))).atmosphere.height_falloff_milli_per_m == 0

    refuses(overrides) = @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(N1_POLICY_JSON(overrides))
    refuses(Dict("sky" => n1_sky_with(Dict("kind" => "hosek", "turbidity_milli" => 3000))))
    refuses(Dict("sky" => n1_sky_with(Dict("kind" => "analytic"))))
    refuses(Dict("sky" => n1_sky_with(Dict("turbidity_milli" => 3000))))
    refuses(Dict("sky" => n1_sky_with(Dict("kind" => "gradient", "turbidity_milli" => 3000))))
    refuses(Dict("sky" => n1_sky_with(Dict("kind" => "analytic", "turbidity_milli" => 1999))))
    refuses(Dict("sky" => n1_sky_with(Dict("kind" => "analytic", "turbidity_milli" => 10001))))
    refuses(Dict("sky" => N1_SKY, "atmosphere" => merge(N1_AIR, Dict("height_falloff_milli_per_m" => -1))))
    refuses(Dict("sky" => N1_SKY, "atmosphere" => merge(N1_AIR, Dict("height_falloff_milli_per_m" => 1001))))
    refuses(Dict("sky" => N1_SKY, "atmosphere" => merge(N1_AIR, Dict("density_at_ground_bp" => 1001))))
    refuses(Dict("sky" => N1_SKY, "atmosphere" => merge(N1_AIR, Dict("sun_scatter_gain_bp" => 20001))))
    refuses(Dict("sky" => N1_SKY, "atmosphere" => merge(N1_AIR, Dict("mie_g" => 7600))))
    # Haze fades into the per-pixel sky; without one there is nothing to fade into.
    refuses(Dict("atmosphere" => N1_AIR))
end

@testset "N-1 absent axes take the historical shader paths exactly" begin
    for direction in (Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0), Vec4f(0.6f0, 0.1f0, -0.79f0, 0.0f0), Vec4f(0.3f0, -0.5f0, 0.81f0, 0.0f0))
        @test LavaAdapter._sky_environment(direction, N1_TOP, N1_HORIZON, N1_GROUND, N1_SUN, N1_ZERO) ===
              LavaAdapter._environment_color(direction, N1_TOP, N1_HORIZON, N1_GROUND)
    end
    color = Vec4f(0.3f0, 0.25f0, 0.2f0, 1.0f0)
    fog = Vec4f(N1_ENVIRONMENT.fog_color_rgb..., 1.0f0)
    point, camera = Vec4f(10.0f0, 12.0f0, -40.0f0, 1.0f0), Vec4f(54.5f0, 23.4f0, 49.0f0, 1.0f0)
    distance = LavaAdapter._distance_between(point, camera)
    @test LavaAdapter._apply_aerial_perspective(
        color, point, camera, distance, fog, 0.0011f0, N1_LIGHT.direction, N1_LIGHT.color, N1_LIGHT.intensity,
        N1_TOP, N1_HORIZON, N1_ZERO, N1_ZERO,
    ) === LavaAdapter._apply_fog(color, fog, distance, 0.0011f0)
    @test LavaAdapter._sky_parameters(LuxelGraphics.RenderPolicy(), N1_LIGHT, N1_ENVIRONMENT) == N1_ZERO
    @test LavaAdapter._sky_parameters(LuxelGraphics._parse_render_policy(N1_POLICY_JSON(Dict("sky" => N1_SKY))), N1_LIGHT, N1_ENVIRONMENT) == N1_ZERO
    @test LavaAdapter._atmosphere_parameters(LuxelGraphics.RenderPolicy()) == N1_ZERO
    # The gradient model's sky radiance is the CONVERGE-0 sky-pass formula.
    for direction in (Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0), LavaAdapter._normalize_vector(Vec4f(0.6f0, 0.1f0, -0.79f0, 0.0f0)), Vec4f(0.6f0, -0.2f0, 0.77f0, 0.0f0))
        weight = direction[2] > 0.0f0 ? sqrt(direction[2]) : 0.0f0
        expected = Vec4f((N1_HORIZON[c] * (1.0f0 - weight) + N1_TOP[c] * weight for c in 1:3)..., 1.0f0)
        @test LavaAdapter._sky_radiance(direction, N1_TOP, N1_HORIZON, N1_SUN, N1_ZERO) === expected
    end
end

@testset "N-1 haze weight is the exact optical-depth integral" begin
    air = Vec4f(1.0f0, 0.015f0, 0.005f0, 0.03f0)
    # Midpoint-rule quadrature of σ₀·exp(−k·h) along the segment, in Float64.
    function quadrature(hc, hp, d, k=Float64(air[2]); n=200_000)
        s0 = Float64(air[3])
        tau = sum(s0 * exp(-k * (hc + (hp - hc) * (i - 0.5) / n)) for i in 1:n) * d / n
        return 1 - exp(-tau)
    end
    for (hc, hp, d) in ((23.4, 12.0, 32.0), (23.4, 40.0, 300.0), (20.0, 20.0, 150.0), (17.6, 13.1, 14.3), (23.4, 23.40001, 80.0))
        @test isapprox(LavaAdapter._haze_weight(Float32(hc), Float32(hp), Float32(d), air), quadrature(hc, hp, d); atol=2e-5)
    end
    # k = 0: homogeneous haze, 1 − exp(−σ₀·d).
    homogeneous = Vec4f(1.0f0, 0.0f0, 0.005f0, 0.0f0)
    @test isapprox(LavaAdapter._haze_weight(23.4f0, 40.0f0, 300.0f0, homogeneous), 1 - exp(-0.005 * 300); atol=1e-5)
    # The series branch (|k·Δh| < 1e-3) joins the closed form without a step.
    near = LavaAdapter._haze_weight(23.4f0, 23.4f0 + 0.0666f0, 100.0f0, air)
    far = LavaAdapter._haze_weight(23.4f0, 23.4f0 + 0.0668f0, 100.0f0, air)
    @test abs(near - far) < 1e-5
    # Valleys are hazier than ridges at the same distance.
    @test LavaAdapter._haze_weight(23.4f0, 10.0f0, 200.0f0, air) > LavaAdapter._haze_weight(23.4f0, 40.0f0, 200.0f0, air)
end

@testset "N-1 analytic sky: scale, sun-side horizon, one function" begin
    sky = n1_analytic_parameters()
    @test sky[1] == 3.0f0
    # The zenith is the authored zenith colour, exactly in hue and luminance.
    zenith = n1_sky(Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0))
    @test all(isapprox.(zenith[1:3], N1_TOP[1:3]; rtol=1e-4))
    horizontal_sun = LavaAdapter._normalize_vector(Vec4f(N1_SUN[1], 0.0f0, N1_SUN[3], 0.0f0))
    anti = Vec4f(-horizontal_sun[1], 0.0f0, -horizontal_sun[3], 0.0f0)
    sun_side = n1_luminance(n1_sky(horizontal_sun))
    anti_side = n1_luminance(n1_sky(anti))
    # Contract §1 acceptance (measured): sun-side horizon ≥ 15% above anti-sun.
    @test sun_side >= 1.15f0 * anti_side
    # Hue is the authored horizon's: no Preetham salmon/magenta cast.
    horizon_hue = n1_sky(anti)
    @test isapprox(horizon_hue[3] / horizon_hue[1], N1_HORIZON[3] / N1_HORIZON[1]; rtol=1e-4)
    @info "N-1 analytic horizon" sun_side anti_side ratio = sun_side / anti_side zenith = n1_luminance(zenith)
    for turbidity in (2000, 6000, 10000)
        p = n1_analytic_parameters(turbidity)
        for direction in (Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0), horizontal_sun, anti, N1_SUN, Vec4f(0.3f0, -0.6f0, 0.74f0, 0.0f0))
            c = n1_sky(direction, p)
            @test all(isfinite, c) && all(>=(0.0f0), c)
        end
    end
    # Ambient and the sky pass use the same function as the drawn sky.
    up_ish = LavaAdapter._normalize_vector(Vec4f(0.2f0, 0.7f0, -0.3f0, 0.0f0))
    @test LavaAdapter._sky_environment(up_ish, N1_TOP, N1_HORIZON, N1_GROUND, N1_SUN, sky) == n1_sky(up_ish)
    @test LavaAdapter._sky_radiance(up_ish, N1_TOP, N1_HORIZON, N1_SUN, sky) == n1_sky(up_ish)
    straight_down = LavaAdapter._sky_environment(Vec4f(0.0f0, -1.0f0, 0.0f0, 0.0f0), N1_TOP, N1_HORIZON, N1_GROUND, N1_SUN, sky)
    @test straight_down[1:3] == N1_GROUND[1:3]
    # A sun at or below the horizon is outside the Preetham fit: refused.
    low = LavaAdapter.DirectionalLighting(Vec4f(1.0f0, 0.1f0, 0.0f0, 0.0f0), N1_LIGHT.color, 5.0f0)
    policy = LuxelGraphics._parse_render_policy(N1_POLICY_JSON(Dict("sky" => n1_sky_with(Dict("kind" => "analytic", "turbidity_milli" => 3000)))))
    @test_throws LavaAdapter.AdapterError LavaAdapter._sky_parameters(policy, low, N1_ENVIRONMENT)
end

@testset "N-1 haze converges on the sky behind it and warms toward the sun" begin
    horizontal_sun = LavaAdapter._normalize_vector(Vec4f(N1_SUN[1], 0.0f0, N1_SUN[3], 0.0f0))
    away = Vec4f(-horizontal_sun[1], 0.0f0, -horizontal_sun[3], 0.0f0)
    radiance = Vec4f(5.0f0, 4.1f0, 3.1f0, 0.0f0)
    for sky in (N1_ZERO, n1_analytic_parameters())
        hot = LavaAdapter._inscattered_light(horizontal_sun, N1_SUN, radiance, N1_TOP, N1_HORIZON, sky, 0.03f0)
        cold = LavaAdapter._inscattered_light(away, N1_SUN, radiance, N1_TOP, N1_HORIZON, sky, 0.03f0)
        @test n1_luminance(hot) > n1_luminance(cold)
        @test hot[1] / hot[3] > cold[1] / cold[3]  # warmer: more red per blue
        # With no sun lobe the haze IS the sky radiance along the ray.
        for direction in (away, LavaAdapter._normalize_vector(Vec4f(-0.6f0, 0.05f0, -0.77f0, 0.0f0)))
            @test LavaAdapter._inscattered_light(direction, N1_SUN, radiance, N1_TOP, N1_HORIZON, sky, 0.0f0)[1:3] ==
                  LavaAdapter._sky_radiance(direction, N1_TOP, N1_HORIZON, N1_SUN, sky)[1:3]
        end
    end
    # A very distant surface converges on the sky behind it (the ridge test).
    air = Vec4f(1.0f0, 0.01f0, 0.007f0, 0.03f0)
    camera = Vec4f(54.5f0, 23.4f0, 49.0f0, 1.0f0)
    ridge = Vec4f(54.5f0 - 0.6f0 * 5000.0f0, 30.0f0, 49.0f0 - 0.77f0 * 5000.0f0, 1.0f0)
    dark = Vec4f(0.02f0, 0.02f0, 0.02f0, 1.0f0)
    sky = n1_analytic_parameters()
    fogged = LavaAdapter._apply_aerial_perspective(
        dark, ridge, camera, LavaAdapter._distance_between(ridge, camera), N1_ZERO, 0.0f0,
        N1_LIGHT.direction, N1_LIGHT.color, N1_LIGHT.intensity, N1_TOP, N1_HORIZON, sky, air,
    )
    ray = LavaAdapter._normalize_vector(Vec4f(ridge[1] - camera[1], ridge[2] - camera[2], ridge[3] - camera[3], 0.0f0))
    behind = LavaAdapter._inscattered_light(ray, N1_SUN, Vec4f(5.0f0, 4.1f0, 3.1f0, 0.0f0), N1_TOP, N1_HORIZON, sky, 0.03f0)
    @test all(isapprox.(fogged[1:3], behind[1:3]; rtol=1e-3))
end

@testset "CALIBRATION-1 debug albedo override" begin
    parsed = LuxelGraphics._parse_render_policy(N1_POLICY_JSON(Dict("debug" => Dict("albedo_override_bp" => 5000))))
    @test parsed.debug.albedo_override_bp == 5000
    @test LuxelGraphics.RenderPolicy().debug === nothing
    @test LuxelGraphics._parse_render_policy(N1_POLICY_JSON(Dict("sky" => N1_SKY))).debug === nothing
    refuses(overrides) = @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(N1_POLICY_JSON(overrides))
    refuses(Dict("debug" => Dict("albedo_override_bp" => 10001)))
    refuses(Dict("debug" => Dict("albedo_override_bp" => -1)))
    refuses(Dict("debug" => Dict("albedo_override_bp" => 5000, "normals" => true)))
    # Absent: zeros, and the override is the identity (packets keep their bytes).
    @test LavaAdapter._surface_options(LuxelGraphics.RenderPolicy()) == N1_ZERO
    color = Vec4f(0.3f0, 0.6f0, 0.1f0, 0.8f0)
    @test LavaAdapter._albedo_override(color, N1_ZERO) === color
    forced = LavaAdapter._albedo_override(color, LavaAdapter._surface_options(parsed))
    @test forced == Vec4f(0.5f0, 0.5f0, 0.5f0, 0.8f0)  # alpha is kept
end

@testset "N-2 IBL axis, layout and octahedral parity" begin
    on = LuxelGraphics._parse_render_policy(N1_POLICY_JSON(Dict("ibl" => Dict("enabled" => true))))
    @test on.ibl.enabled
    @test LuxelGraphics.RenderPolicy().ibl === nothing
    @test LavaAdapter._surface_options(on) == Vec4f(0.0f0, 0.0f0, 1.0f0, 0.0f0)
    off = LuxelGraphics._parse_render_policy(N1_POLICY_JSON(Dict("ibl" => Dict("enabled" => false))))
    @test LavaAdapter._surface_options(off) == N1_ZERO
    refuses(overrides) = @test_throws LuxelGraphics.ProtocolError LuxelGraphics._parse_render_policy(N1_POLICY_JSON(overrides))
    refuses(Dict("ibl" => Dict("enabled" => 1)))
    refuses(Dict("ibl" => Dict("enabled" => true, "probes" => 4)))
    # Atlas layout mirrors ibl::atlas_layout(): tiles of 128..4 then 32, +2 gutter each.
    origins = cumsum([0; [128, 64, 32, 16, 8, 4, 32] .+ 2])
    for level in 0:5
        @test LavaAdapter._ibl_level_origin(Int32(level)) == Float32(origins[level + 1])
        @test LavaAdapter._ibl_level_size(Int32(level)) == Float32(128 >> level)
    end
    @test LavaAdapter.IBL_IRRADIANCE_ORIGIN == Float32(origins[7])
    @test LavaAdapter.IBL_ATLAS_WIDTH == Float32(origins[8])
    @test LavaAdapter.IBL_ATLAS_HEIGHT == 130.0f0
    # Octahedral encode agrees with Rust's (same formula, hand-checked points).
    @test LavaAdapter._octahedral_uv(Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0)) == Vec2f(0.5f0, 0.5f0)
    @test LavaAdapter._octahedral_uv(Vec4f(1.0f0, 0.0f0, 0.0f0, 0.0f0)) == Vec2f(1.0f0, 0.5f0)
    @test LavaAdapter._octahedral_uv(Vec4f(0.0f0, -1.0f0, 0.0f0, 0.0f0)) == Vec2f(1.0f0, 1.0f0)
    @test LavaAdapter._octahedral_uv(Vec4f(0.0f0, 0.0f0, -1.0f0, 0.0f0)) == Vec2f(0.5f0, 0.0f0)
end

@testset "texture upload order is row-major (transpose regression)" begin
    # A 3-wide, 2-high payload whose pixels encode their own (x, y): every
    # packet texture used to reach Vulkan transposed (non-square: scrambled).
    width, height = 3, 2
    bytes = UInt8[]
    for y in 0:height-1, x in 0:width-1
        append!(bytes, (UInt8(10x), UInt8(10y), 0x00, 0xff))
    end
    matrix = LavaAdapter._texture_matrix(bytes, UInt32(width), UInt32(height), :data)
    uploaded = vec(LavaAdapter._upload_layout(matrix))
    @test length(uploaded) == width * height
    for (index, texel) in enumerate(uploaded)
        x, y = (index - 1) % width, (index - 1) ÷ width
        @test round(Int, texel[1] * 255) == 10x && round(Int, texel[2] * 255) == 10y
    end
end

@testset "N-2 IBL arithmetic in the real shader function (CPU)" begin
    # Light off, occlusion 1: the result is the IBL term alone.
    respond(base, metallic, roughness, source; ibl=1.0f0) = LavaAdapter._material_response(
        Vec4f(base, base, base, 1.0f0), Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0), Vec4f(0.0f0, -1.0f0, 0.0f0, 0.0f0),
        Vec4f(1.0f0, 1.0f0, 1.0f0, 1.0f0), 0.0f0, N1_TOP, N1_HORIZON, N1_GROUND, metallic, roughness, 0.0f0, 0.5f0,
        1.0f0, Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0), 1.0f0, 1.0f0, 0.0f0, N1_ZERO, N1_ZERO, N1_ZERO, ibl, source,
    )
    white = Vec4f(1.0f0, 1.0f0, 1.0f0, 1.0f0)
    # Head-on (N·V = 1), so F = F0: a white dielectric returns (1 − 0.04)·E + (0.04 A + B)·L.
    a, b = 0.6f0, 0.02f0
    dielectric = respond(1.0f0, 0.0f0, 1.0f0, LavaAdapter.ConstantIbl(white, white, Vec2f(a, b)))
    @test dielectric[1] ≈ (1.0f0 - 0.04f0) + (0.04f0 * a + b) atol = 1.0f-5
    # A white metal has no diffuse term: it reflects (F0·A + B)·L with F0 = 1.
    metal = respond(1.0f0, 1.0f0, 0.2f0, LavaAdapter.ConstantIbl(white, white, Vec2f(0.95f0, 0.01f0)))
    @test metal[1] ≈ 0.96f0 atol = 1.0f-5
    # With the flag off the IBL source is ignored: the historical path.
    @test respond(0.5f0, 0.0f0, 0.6f0, LavaAdapter.ConstantIbl(white, white, Vec2f(a, b)); ibl=0.0f0) ==
          respond(0.5f0, 0.0f0, 0.6f0, LavaAdapter.NoIbl(); ibl=0.0f0)
end
