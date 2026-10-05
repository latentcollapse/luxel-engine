module LavaAdapter

using Base64
using GeometryBasics: Vec2f, Vec4f
using Lava
using SHA
using Vulkan
import WGEGraphics

export AdapterError,
    LavaBackend,
    backend,
    backend_probe,
    backend_ready,
    render_probe,
    render_depth_probe,
    render_texture_probe,
    render_scene

const ADAPTER_REVISION = "wge.lava-adapter/v7"
const LAVA_REVISION = "11c7e31bdf62408d22bf379e9e59510f69d2103e"
const VULKAN_REVISION = "03b4ca2351477ccbb8ee378f512da50f7eec7bac"
const VULKAN_CORE_REVISION = "1d02829e8fa92da430d879db4dd7bf564a872035"
const MAX_CAPTURE_BYTES = 32 * 1024 * 1024

struct AdapterError <: Exception
    code::String
    detail::String
end

Base.showerror(io::IO, error::AdapterError) = print(io, error.code, ": ", error.detail)

struct TerrainResources
    cache_key::String
    heights::Lava.LavaArray{Float32,1}
    slopes::Lava.LavaArray{Float32,1}
    regions::Lava.LavaArray{UInt8,1}
    resolution::Int32
end

struct OverlayResources
    cache_key::String
    positions::Lava.LavaArray{Vec4f,1}
    colors::Lava.LavaArray{Vec4f,1}
    vertex_count::Int
end

struct MeshBatchResources
    material_id::String
    positions::Lava.LavaArray{Vec4f,1}
    normals::Lava.LavaArray{Vec4f,1}
    uvs::Lava.LavaArray{Vec2f,1}
    tangents::Lava.LavaArray{Vec4f,1}
    translations::Lava.LavaArray{Vec4f,1}
    rotations::Lava.LavaArray{Vec4f,1}
    scales::Lava.LavaArray{Vec4f,1}
    colors::Lava.LavaArray{Vec4f,1}
    material_parameters::Lava.LavaArray{Vec4f,1}
    surface_parameters::Lava.LavaArray{Vec4f,1}
    emissive_parameters::Lava.LavaArray{Vec4f,1}
    vertex_count::Int
    instance_count::Int
end

struct MeshResources
    cache_key::String
    batches::Vector{MeshBatchResources}
    instance_count::Int
    visible_instance_count::Int
    culled_instance_count::Int
    vertex_count::Int
end

struct MeshVisibility
    instance_count::Int
    visible_instance_count::Int
    culled_instance_count::Int
    background_visible_count::Int
    background_culled_count::Int
    landmark_visible_count::Int
    landmark_culled_count::Int
    gameplay_critical_visible_count::Int
    gameplay_critical_culled_count::Int
end

struct TextureProbeResources
    texture::Lava.LavaTexture2D
    sampler::Lava.LavaSampler
    bindings::Lava.TextureBindings
end

"""How a surface samples its textures.

This type is the CACHE KEY that makes per-surface sampling possible. Before it
existed, `_material_texture_resources!` cached by `material_id` alone, so two
surfaces sharing a material also shared one sampler — and terrain therefore
could not request `REPEAT` without silently changing every mesh's UV
addressing. That was the structural blocker behind sprint finding F-4.

Immutable and comparable, so it can key a `Dict`, and small enough that the
per-frame lookup is free. It deliberately carries ONLY what is actually
exercised today: anisotropy is NOT here, because the Vulkan device feature it
needs is still unenabled upstream (see `_anisotropy_refusal_detail`). Adding a
field here that nothing can set would be speculative machinery for a capability
that cannot execute.
"""
struct SamplerSpec
    filter::Symbol
    wrap::Symbol
end

const CLAMPED_LINEAR_SPEC = SamplerSpec(:linear, :clamp)
const REPEATED_LINEAR_SPEC = SamplerSpec(:linear, :repeat)

"""The sampling spec a given surface uses, derived from render policy.

Meshes are ALWAYS clamped: their UVs are authored in 0..1 per mesh, and letting
a terrain tiling policy reach them would be precisely the cross-surface
contamination F-4 describes. Terrain follows the policy. This function is the
single place that asymmetry is decided, so it can be tested rather than
discovered in a screenshot.
"""
function _surface_sampler_spec(
    policy::WGEGraphics.RenderPolicy,
    surface::Symbol,
)::SamplerSpec
    if surface === :mesh
        # CONVERGE-0 metric UVs exceed 1.0 and need REPEAT; the default clamp
        # is the historical spec, so an absent policy is byte-identical.
        return policy.mesh_surface.wrap_repeat ? REPEATED_LINEAR_SPEC : CLAMPED_LINEAR_SPEC
    elseif surface === :terrain
        return policy.terrain_surface.wrap_repeat ?
               REPEATED_LINEAR_SPEC : CLAMPED_LINEAR_SPEC
    end
    throw(AdapterError(
        "unsupported_texture",
        "unknown surface kind $surface; expected :terrain or :mesh",
    ))
end

struct MaterialTextures
    albedo_texture::Lava.LavaTexture2D
    normal_texture::Lava.LavaTexture2D
    roughness_texture::Lava.LavaTexture2D
    occlusion_texture::Lava.LavaTexture2D
    emissive_texture::Lava.LavaTexture2D
    max_sampler_lod::UInt32
end

struct MaterialTextureResources
    cache_key::String
    material_id::String
    spec::SamplerSpec
    textures::MaterialTextures
    sampler::Lava.LavaSampler
    bindings::Lava.TextureBindings
    max_sampler_lod::UInt32
end

struct DirectionalLighting
    direction::Vec4f
    color::Vec4f
    intensity::Float32
end

struct EnvironmentLighting
    sky_top::Vec4f
    sky_horizon::Vec4f
    ground::Vec4f
end

struct CameraFrame
    position::Vec4f
    right::Vec4f
    up::Vec4f
    forward::Vec4f
    # projection = (horizontal/vertical half extent for orthographic, or
    # tan(horizontal/vertical half-FOV) for perspective, near, far).
    projection::Vec4f
    # mode 0 is orthographic; mode 1 is perspective.
    mode::Float32
    width_px::Float32
    height_px::Float32
end

struct ShadowResources
    cache_key::String
    framebuffer::Lava.LavaFramebuffer
    texture::Lava.LavaTexture2D{NTuple{4,Float32}}
    sampler::Lava.LavaSampler
    light_frame::CameraFrame
end

struct ResolveResources
    source_size::Tuple{Int,Int}
    framebuffer::Lava.LavaFramebuffer
    texture::Lava.LavaTexture2D{NTuple{4,Float32}}
    sampler::Lava.LavaSampler
    bindings::Lava.TextureBindings
end

"""GPU state for an N-4 layered terrain: one descriptor set of 14 bindings
(three layers x albedo/normal/roughness/occlusion, the shadow map pinned at
binding 5 where `_shadow_depth` reads it, and the macro noise at 13), a sampler
built for the layers' own LOD range, and the lowered per-layer uniforms."""
struct TerrainLayerResources
    cache_key::String
    bindings::Lava.TextureBindings
    sampler::Lava.LavaSampler
    uniforms::NTuple{7,Vec4f}
end

"""N-2 IBL textures for one packet: the baked environment atlas and BRDF table
(1x1 stand-ins when the packet has no IBL, so every material descriptor set has
the same layout), sampled with clamp and no mips."""
struct IblResources
    cache_key::String
    environment::Lava.LavaTexture2D
    brdf_lut::Lava.LavaTexture2D
    placeholder::Lava.LavaTexture2D
    sampler::Lava.LavaSampler
end

mutable struct LavaBackend{C,Q,PP,SP,SVP,TP,TLP,OP,MP,MCP,TXP,DP,TSP,MSP,MCSP,RP}
    context::C
    queue::Q
    probe_pipeline::PP
    sky_pipeline::SP
    sky_view_pipeline::SVP
    terrain_pipeline::TP
    terrain_layered_pipeline::TLP
    overlay_pipeline::OP
    mesh_pipeline::MP
    mesh_cutout_pipeline::MCP
    texture_pipeline::TXP
    depth_pipeline::DP
    terrain_shadow_pipeline::TSP
    mesh_shadow_pipeline::MSP
    mesh_cutout_shadow_pipeline::MCSP
    resolve_pipeline::RP
    framebuffers::Dict{Tuple{Int,Int,Bool,Symbol},Lava.LavaFramebuffer}
    terrain_resources::Union{Nothing,TerrainResources}
    overlay_resources::Union{Nothing,OverlayResources}
    mesh_resources::Union{Nothing,MeshResources}
    shadow_mesh_resources::Union{Nothing,MeshResources}
    shadow_resources::Union{Nothing,ShadowResources}
    terrain_layer_resources::Union{Nothing,TerrainLayerResources}
    resolve_resources::Union{Nothing,ResolveResources}
    texture_resources::Union{Nothing,TextureProbeResources}
    material_texture_resources::Dict{Tuple{String,String,SamplerSpec},MaterialTextureResources}
    material_textures::Dict{Tuple{String,String},MaterialTextures}
    cutout_shadow_bindings::Dict{Tuple{String,String},Lava.TextureBindings}
    surface_samplers::Dict{Tuple{SamplerSpec,UInt32},Lava.LavaSampler}
    ibl_resources::Union{Nothing,IblResources}
    upload_bytes::UInt64
    draw_calls::UInt64
    readback_bytes::UInt64
    pipeline_compilations::UInt64
    gpu_timestamp_supported::Bool
    probe_compiled::Bool
    terrain_compiled::Bool
    terrain_layered_compiled::Bool
    overlay_compiled::Bool
    sky_compiled::Bool
    sky_view_compiled::Bool
    mesh_compiled::Bool
    texture_compiled::Bool
    depth_compiled::Bool
    terrain_shadow_compiled::Bool
    mesh_shadow_compiled::Bool
    resolve_compiled::Bool
    mesh_cutout_compiled::Bool
    mesh_cutout_shadow_compiled::Bool
end

const BACKEND_REF = Ref{Union{Nothing,LavaBackend}}(nothing)
const BACKEND_LOCK = ReentrantLock()

function _probe_vertex()
    vertex_id = Lava.vertex_index() - Int32(1)
    x = Float32(Int32(vertex_id & Int32(1)) * 4 - 1)
    y = Float32(Int32((vertex_id >> Int32(1)) & Int32(1)) * 4 - 1)
    Lava.set_position!(Vec4f(x, y, 0.5f0, 1.0f0))
    return nothing
end

function _probe_fragment()
    Lava.gfx_output(0, Vec4f(0.0f0, 1.0f0, 0.0f0, 1.0f0))
    return nothing
end

function _tone_map(color::Vec4f, exposure::Float32)::Vec4f
    scale = max(exposure, 0.01f0)
    red = max(color[1], 0.0f0) * scale
    green = max(color[2], 0.0f0) * scale
    blue = max(color[3], 0.0f0) * scale
    numerator_r = red * (2.51f0 * red + 0.03f0)
    numerator_g = green * (2.51f0 * green + 0.03f0)
    numerator_b = blue * (2.51f0 * blue + 0.03f0)
    denominator_r = red * (2.43f0 * red + 0.59f0) + 0.14f0
    denominator_g = green * (2.43f0 * green + 0.59f0) + 0.14f0
    denominator_b = blue * (2.43f0 * blue + 0.59f0) + 0.14f0
    return Vec4f(
        clamp(numerator_r / denominator_r, 0.0f0, 1.0f0),
        clamp(numerator_g / denominator_g, 0.0f0, 1.0f0),
        clamp(numerator_b / denominator_b, 0.0f0, 1.0f0),
        color[4],
    )
end

function _sky_vertex(
    sky_top::Vec4f,
    sky_horizon::Vec4f,
    ground::Vec4f,
    fog_color::Vec4f,
)
    vertex_id = Lava.vertex_index() - Int32(1)
    x = Float32(Int32(vertex_id & Int32(1)) * 4 - 1)
    y = Float32(Int32((vertex_id >> Int32(1)) & Int32(1)) * 4 - 1)
    Lava.set_position!(Vec4f(x, y, 0.99f0, 1.0f0))
    sky_color = _environment_color(Vec4f(0.0f0, y, 0.0f0, 0.0f0), sky_top, sky_horizon, ground)
    atmosphere = 0.08f0
    Lava.gfx_output(
        0,
        Vec4f(
            sky_color[1] + fog_color[1] * atmosphere,
            sky_color[2] + fog_color[2] * atmosphere,
            sky_color[3] + fog_color[3] * atmosphere,
            1.0f0,
        ),
    )
    return nothing
end

function _sky_fragment()
    Lava.gfx_output(0, Lava.gfx_input(Vec4f, 0))
    return nothing
end

"""View-direction sky (render_policy.sky).

The vertex stage emits the UNNORMALISED camera ray for each corner of the
full-screen triangle. The ray is affine in NDC, so its linear interpolation is
exact per pixel; the fragment normalises it. `ndc_y` is negated exactly as in
`_project_world`, because positive NDC Y maps to lower framebuffer rows.
"""
function _sky_view_vertex(
    camera_forward::Vec4f,
    camera_right_scaled::Vec4f,
    camera_up_scaled::Vec4f,
    sky_top::Vec4f,
    sky_horizon::Vec4f,
    sun_direction::Vec4f,
    sun_radiance::Vec4f,
    sun_parameters::Vec4f,
    sky_parameters::Vec4f,
    atmosphere_parameters::Vec4f,
)
    vertex_id = Lava.vertex_index() - Int32(1)
    x = Float32(Int32(vertex_id & Int32(1)) * 4 - 1)
    y = Float32(Int32((vertex_id >> Int32(1)) & Int32(1)) * 4 - 1)
    Lava.set_position!(Vec4f(x, y, 0.99f0, 1.0f0))
    Lava.gfx_output(
        0,
        Vec4f(
            camera_forward[1] + x * camera_right_scaled[1] - y * camera_up_scaled[1],
            camera_forward[2] + x * camera_right_scaled[2] - y * camera_up_scaled[2],
            camera_forward[3] + x * camera_right_scaled[3] - y * camera_up_scaled[3],
            0.0f0,
        ),
    )
    Lava.gfx_output(1, sky_top)
    Lava.gfx_output(2, sky_horizon)
    Lava.gfx_output(3, sun_direction)
    Lava.gfx_output(4, sun_radiance)
    Lava.gfx_output(5, sun_parameters)
    Lava.gfx_output(6, sky_parameters)
    Lava.gfx_output(7, atmosphere_parameters)
    return nothing
end

"""Shade one sky pixel along its view ray.

Above the horizon this is the same `sqrt`-weighted horizon→zenith gradient the
surface shading samples for ambient light, so the sky the eye sees and the sky
the materials are lit by are one function. Below the horizon it holds the
horizon colour: a ray that misses the terrain there would otherwise show the
dark ground term and read as a hole in the world.

`sun_parameters = (cos outer disc edge, cos inner disc edge, disc gain, glow gain)`.
The glow is `0.8 cos^32 + 0.2 cos^4` of the angle to the sun — a narrow
forward-scattering halo plus a broad warm cast on the sun side of the sky.

Under the analytic model (`sky_parameters[1] > 0`, N-1) the base is
`_analytic_sky`, the same function ambient light samples. With
`render_policy.atmosphere` the sun lobe of `_sun_scatter` is added, the same
lobe distant surfaces receive, so haze and sky stay matched toward the sun.
The sky itself is not hazed: it already is the atmosphere along an infinite ray.
"""
function _sky_view_fragment()
    ray = _normalize_vector(Lava.gfx_input(Vec4f, 0))
    sky_top = Lava.gfx_input(Vec4f, 1)
    sky_horizon = Lava.gfx_input(Vec4f, 2)
    sun_direction = Lava.gfx_input(Vec4f, 3)
    sun_radiance = Lava.gfx_input(Vec4f, 4)
    parameters = Lava.gfx_input(Vec4f, 5)
    sky_parameters = Lava.gfx_input(Vec4f, 6)
    atmosphere_parameters = Lava.gfx_input(Vec4f, 7)
    base = _sky_radiance(ray, sky_top, sky_horizon, sun_direction, sky_parameters)
    base_red = base[1]
    base_green = base[2]
    base_blue = base[3]
    alignment = max(_dot_vector(ray, sun_direction), 0.0f0)
    disc_t = clamp(
        (alignment - parameters[1]) / max(parameters[2] - parameters[1], 1.0f-7),
        0.0f0,
        1.0f0,
    )
    disc = disc_t * disc_t * (3.0f0 - 2.0f0 * disc_t)
    power2 = alignment * alignment
    power4 = power2 * power2
    power8 = power4 * power4
    power16 = power8 * power8
    power32 = power16 * power16
    sun = parameters[3] * disc + parameters[4] * (0.8f0 * power32 + 0.2f0 * power4)
    sky_color = Vec4f(
        base_red + sun_radiance[1] * sun,
        base_green + sun_radiance[2] * sun,
        base_blue + sun_radiance[3] * sun,
        1.0f0,
    )
    if atmosphere_parameters[1] > 0.5f0
        scattered = _sun_scatter(ray, sun_direction, sun_radiance, atmosphere_parameters[4])
        sky_color = Vec4f(sky_color[1] + scattered[1], sky_color[2] + scattered[2], sky_color[3] + scattered[3], 1.0f0)
    end
    Lava.gfx_output(0, sky_color)
    return nothing
end

"""Lower the sky policy and camera into `_sky_view_vertex` arguments.

Perspective only: an orthographic camera has one ray direction, so a gradient
sky along it is meaningless, and the honest response is a typed refusal.
"""
function _sky_view_arguments(
    camera_frame::CameraFrame,
    packet::WGEGraphics.GraphicsScenePacket,
    lighting::DirectionalLighting,
    sky::WGEGraphics.SkyPolicy,
)
    camera_frame.mode > 0.5f0 || throw(AdapterError(
        "unsupported_render_policy",
        "render_policy.sky requires a perspective camera",
    ))
    tan_x = camera_frame.projection[1]
    tan_y = camera_frame.projection[2]
    toward_sun = _normalize_vector(Vec4f(
        -lighting.direction[1],
        -lighting.direction[2],
        -lighting.direction[3],
        0.0f0,
    ))
    radius = Float32(sky.sun_disc_radius_milli_deg) / 1000.0f0 * Float32(pi) / 180.0f0
    # Antialias the disc edge over the outer 15% of its radius.
    cos_outer = cos(radius)
    cos_inner = cos(radius * 0.85f0)
    disc_gain = sky.sun_disc_radius_milli_deg > 0 ? Float32(sky.sun_disc_gain_bp) / POLICY_SCALE_F32 : 0.0f0
    radiance = Vec4f(
        lighting.color[1] * lighting.intensity,
        lighting.color[2] * lighting.intensity,
        lighting.color[3] * lighting.intensity,
        0.0f0,
    )
    return (
        camera_frame.forward,
        Vec4f(camera_frame.right[1] * tan_x, camera_frame.right[2] * tan_x, camera_frame.right[3] * tan_x, 0.0f0),
        Vec4f(camera_frame.up[1] * tan_y, camera_frame.up[2] * tan_y, camera_frame.up[3] * tan_y, 0.0f0),
        Vec4f(packet.environment.sky_top_rgb..., 1.0f0),
        Vec4f(packet.environment.sky_horizon_rgb..., 1.0f0),
        toward_sun,
        radiance,
        Vec4f(cos_outer, cos_inner, disc_gain, Float32(sky.sun_glow_gain_bp) / POLICY_SCALE_F32),
        _sky_parameters(packet.render_policy, lighting, packet.environment),
        _atmosphere_parameters(packet.render_policy),
    )
end

const POLICY_SCALE_F32 = 10_000.0f0

"""Lower `render_policy.sky.model` into the shader's `sky = (T, Ŷ, 0, 0)`.

Zero means the gradient model (absent sky, or `model` gradient). Under the
analytic model Ŷ = luminance(sky_top_rgb) / F(0, θs), so the zenith keeps the
authored brightness and the Perez distribution sets everything relative to it.

The Preetham fit is undefined for a sun at or below the horizon, so that is a
typed refusal rather than a clamped sky.
"""
function _sky_parameters(
    policy::WGEGraphics.RenderPolicy,
    lighting::DirectionalLighting,
    environment::WGEGraphics.EnvironmentPacket,
)::Vec4f
    sky = policy.sky
    (sky === nothing || sky.model !== :analytic) && return Vec4f(0.0f0, 0.0f0, 0.0f0, 0.0f0)
    toward_sun = _normalize_vector(Vec4f(-lighting.direction[1], -lighting.direction[2], -lighting.direction[3], 0.0f0))
    toward_sun[2] > 0.0f0 || throw(AdapterError(
        "unsupported_render_policy",
        "render_policy.sky.model analytic requires the key light above the horizon",
    ))
    turbidity = Float32(sky.turbidity_milli) / 1000.0f0
    theta_s = acos(clamp(toward_sun[2], -1.0f0, 1.0f0))
    top = environment.sky_top_rgb
    zenith_luminance = 0.2126f0 * top[1] + 0.7152f0 * top[2] + 0.0722f0 * top[3]
    return Vec4f(turbidity, zenith_luminance / _perez(turbidity, 1.0f0, theta_s, cos(theta_s)), 0.0f0, 0.0f0)
end

"""Per-surface options in vertex-output slot 20: `(albedo override enabled,
override albedo, IBL enabled, 0)` from `render_policy.debug` and
`render_policy.ibl`. Absent axes are zeros: the historical paths."""
function _surface_options(policy::WGEGraphics.RenderPolicy)::Vec4f
    debug = policy.debug
    return Vec4f(
        debug === nothing ? 0.0f0 : 1.0f0,
        debug === nothing ? 0.0f0 : Float32(debug.albedo_override_bp) / POLICY_SCALE_F32,
        _ibl_enabled(policy) ? 1.0f0 : 0.0f0,
        0.0f0,
    )
end

_ibl_enabled(policy::WGEGraphics.RenderPolicy) = policy.ibl !== nothing && policy.ibl.enabled

"""Lower `render_policy.atmosphere` into `(enabled, k [1/m], σ₀ [1/m], sun gain)`.
Absent is all zeros, which the shaders read as the historical linear fog."""
function _atmosphere_parameters(policy::WGEGraphics.RenderPolicy)::Vec4f
    atmosphere = policy.atmosphere
    atmosphere === nothing && return Vec4f(0.0f0, 0.0f0, 0.0f0, 0.0f0)
    return Vec4f(
        1.0f0,
        Float32(atmosphere.height_falloff_milli_per_m) / 1000.0f0,
        Float32(atmosphere.density_at_ground_bp) / POLICY_SCALE_F32,
        Float32(atmosphere.sun_scatter_gain_bp) / POLICY_SCALE_F32,
    )
end

function _texture_probe_vertex()
    vertex_id = Lava.vertex_index() - Int32(1)
    x = Float32(Int32(vertex_id & Int32(1)) * 4 - 1)
    y = Float32(Int32((vertex_id >> Int32(1)) & Int32(1)) * 4 - 1)
    Lava.set_position!(Vec4f(x, y, 0.5f0, 1.0f0))
    return nothing
end

function _texture_probe_fragment()
    red = Lava.sample_texture_2d(UInt32(0), 0.5f0, 0.5f0, UInt32(0))
    green = Lava.sample_texture_2d(UInt32(0), 0.5f0, 0.5f0, UInt32(1))
    blue = Lava.sample_texture_2d(UInt32(0), 0.5f0, 0.5f0, UInt32(2))
    alpha = Lava.sample_texture_2d(UInt32(0), 0.5f0, 0.5f0, UInt32(3))
    Lava.gfx_output(0, Vec4f(red, green, blue, alpha))
    return nothing
end

function _depth_probe_vertex(color::Vec4f, depth::Float32)
    vertex_id = Lava.vertex_index() - Int32(1)
    x = Float32(Int32(vertex_id & Int32(1)) * 4 - 1)
    y = Float32(Int32((vertex_id >> Int32(1)) & Int32(1)) * 4 - 1)
    Lava.set_position!(Vec4f(x, y, depth, 1.0f0))
    Lava.gfx_output(0, color)
    return nothing
end

function _depth_probe_fragment()
    Lava.gfx_output(0, Lava.gfx_input(Vec4f, 0))
    return nothing
end

function _normalize_vector(vector::Vec4f)::Vec4f
    length_squared = vector[1] * vector[1] + vector[2] * vector[2] + vector[3] * vector[3]
    inverse_length = inv(sqrt(max(length_squared, 1.0f-8)))
    return Vec4f(
        vector[1] * inverse_length,
        vector[2] * inverse_length,
        vector[3] * inverse_length,
        0.0f0,
    )
end

function _dot_vector(first::Vec4f, second::Vec4f)::Float32
    return first[1] * second[1] + first[2] * second[2] + first[3] * second[3]
end

function _environment_color(
    direction::Vec4f,
    sky_top::Vec4f,
    sky_horizon::Vec4f,
    ground::Vec4f,
)::Vec4f
    vertical = clamp(direction[2], -1.0f0, 1.0f0)
    if vertical >= 0.0f0
        weight = sqrt(vertical)
        return Vec4f(
            sky_horizon[1] * (1.0f0 - weight) + sky_top[1] * weight,
            sky_horizon[2] * (1.0f0 - weight) + sky_top[2] * weight,
            sky_horizon[3] * (1.0f0 - weight) + sky_top[3] * weight,
            1.0f0,
        )
    end
    weight = sqrt(-vertical)
    return Vec4f(
        sky_horizon[1] * (1.0f0 - weight) + ground[1] * weight,
        sky_horizon[2] * (1.0f0 - weight) + ground[2] * weight,
        sky_horizon[3] * (1.0f0 - weight) + ground[3] * weight,
        1.0f0,
    )
end

# Preetham, Shirley & Smits (1999), "A Practical Analytic Model for Daylight",
# Appendix: Perez luminance distribution coefficients A..E as linear functions
# of turbidity T, each row (slope, offset). Only the LUMINANCE fit is used. The
# paper's chromaticity fits give a salmon-to-magenta horizon at low turbidity
# (measured here: r/g/b 0.51/0.44/0.43 across the sun at T = 3; Zotti et al.
# 2007, "A Critical Review of the Preetham Skylight Model"), so hue comes from
# the packet's authored gradient instead.
const PREETHAM_Y = ((0.1787f0, -1.4630f0), (-0.3554f0, 0.4275f0), (-0.0227f0, 5.3251f0), (0.1206f0, -2.5771f0), (-0.0670f0, 0.3703f0))

"""Perez sky luminance distribution F(θ, γ) at turbidity T.

`cos_theta` is the cosine of the view ray's zenith angle, `gamma` the angle
between the ray and the sun. For T in [2, 10] B is negative, so
`exp(B / cos_theta)` tends to 0 at the horizon rather than overflowing.
"""
@inline function _perez(turbidity::Float32, cos_theta::Float32, gamma::Float32, cos_gamma::Float32)::Float32
    a = PREETHAM_Y[1][1] * turbidity + PREETHAM_Y[1][2]
    b = PREETHAM_Y[2][1] * turbidity + PREETHAM_Y[2][2]
    c = PREETHAM_Y[3][1] * turbidity + PREETHAM_Y[3][2]
    d = PREETHAM_Y[4][1] * turbidity + PREETHAM_Y[4][2]
    e = PREETHAM_Y[5][1] * turbidity + PREETHAM_Y[5][2]
    return (1.0f0 + a * exp(b / cos_theta)) * (1.0f0 + c * exp(d * gamma) + e * cos_gamma * cos_gamma)
end

"""The CONVERGE-0 gradient sky along `direction`: horizon→zenith along
`sqrt(ray.y)`, the horizon colour held below the horizon."""
@inline function _gradient_sky(direction::Vec4f, sky_top::Vec4f, sky_horizon::Vec4f)::Vec4f
    vertical = direction[2]
    weight = vertical > 0.0f0 ? sqrt(vertical) : 0.0f0
    return Vec4f(
        sky_horizon[1] * (1.0f0 - weight) + sky_top[1] * weight,
        sky_horizon[2] * (1.0f0 - weight) + sky_top[2] * weight,
        sky_horizon[3] * (1.0f0 - weight) + sky_top[3] * weight,
        1.0f0,
    )
end

"""Analytic clear sky along `direction` (render_policy.sky.model = analytic).

Luminance is Ŷ·F(θ, γ), the Perez distribution at turbidity T = `sky[1]`, with
Ŷ = `sky[2]` = luminance(sky_top_rgb) / F(0, θs) so the zenith keeps the
authored brightness (`_sky_parameters`). That is where the horizon glow toward
the sun and the darker sky opposite it come from. Hue is the authored gradient
along the same ray, normalised to unit luminance. Rays below the horizon are
flattened onto it, holding the horizon value at that azimuth.
"""
function _analytic_sky(direction::Vec4f, toward_sun::Vec4f, sky::Vec4f, sky_top::Vec4f, sky_horizon::Vec4f)::Vec4f
    flattened = _normalize_vector(Vec4f(direction[1], max(direction[2], 0.0f0), direction[3], 0.0f0))
    cos_gamma = clamp(_dot_vector(flattened, toward_sun), -1.0f0, 1.0f0)
    luminance = sky[2] * _perez(sky[1], max(flattened[2], 0.001f0), acos(cos_gamma), cos_gamma)
    hue = _gradient_sky(flattened, sky_top, sky_horizon)
    scale = luminance / max(0.2126f0 * hue[1] + 0.7152f0 * hue[2] + 0.0722f0 * hue[3], 1.0f-6)
    return Vec4f(hue[1] * scale, hue[2] * scale, hue[3] * scale, 1.0f0)
end

"""Sky radiance along a view ray: the analytic model when `sky[1] > 0`, the
gradient otherwise. The sky pass draws this; the atmosphere fades distant
surfaces toward it; ambient light samples it (`_sky_environment`)."""
@inline function _sky_radiance(direction::Vec4f, sky_top::Vec4f, sky_horizon::Vec4f, toward_sun::Vec4f, sky::Vec4f)::Vec4f
    return sky[1] > 0.0f0 ?
        _analytic_sky(direction, toward_sun, sky, sky_top, sky_horizon) :
        _gradient_sky(direction, sky_top, sky_horizon)
end

"""Environment radiance for ambient light along `direction`.

`sky[1] == 0` is the gradient model and returns `_environment_color` exactly,
so packets without the analytic model keep their bytes. Under the analytic
model the sky above the horizon is `_analytic_sky`; below it the horizon value
at that azimuth blends toward `ground` along the same `sqrt` the gradient uses.
"""
function _sky_environment(
    direction::Vec4f,
    sky_top::Vec4f,
    sky_horizon::Vec4f,
    ground::Vec4f,
    toward_sun::Vec4f,
    sky::Vec4f,
)::Vec4f
    sky[1] > 0.0f0 || return _environment_color(direction, sky_top, sky_horizon, ground)
    radiance = _analytic_sky(direction, toward_sun, sky, sky_top, sky_horizon)
    vertical = clamp(direction[2], -1.0f0, 1.0f0)
    vertical >= 0.0f0 && return radiance
    weight = sqrt(-vertical)
    return Vec4f(
        radiance[1] * (1.0f0 - weight) + ground[1] * weight,
        radiance[2] * (1.0f0 - weight) + ground[2] * weight,
        radiance[3] * (1.0f0 - weight) + ground[3] * weight,
        1.0f0,
    )
end

# Henyey–Greenstein asymmetry for the atmosphere's sun lobe: a moderately
# forward aerosol phase (haze, not molecules). Normalised so the sphere average
# is 1: 10x toward the sun, 0.16x away from it.
const ATMOSPHERE_PHASE_G = 0.6f0

"""Forward-scattered sunlight along a ray: key light × gain × Henyey–Greenstein
phase. Added to the sky AND to the haze, so both brighten and warm toward the
sun by the same amount and a distant ridge still meets the sky behind it."""
@inline function _sun_scatter(direction::Vec4f, toward_sun::Vec4f, sun_radiance::Vec4f, gain::Float32)::Vec4f
    g = ATMOSPHERE_PHASE_G
    denominator = max(1.0f0 + g * g - 2.0f0 * g * _dot_vector(direction, toward_sun), 1.0f-4)
    lobe = gain * (1.0f0 - g * g) / (denominator * sqrt(denominator))
    return Vec4f(sun_radiance[1] * lobe, sun_radiance[2] * lobe, sun_radiance[3] * lobe, 0.0f0)
end

"""Light the haze scatters into a view ray (render_policy.atmosphere): the sky
radiance along that same ray plus the sun lobe. This is the aerial-perspective
asymptote: as distance grows a surface converges on exactly the sky drawn
behind it, so distance reads as fading into the sky rather than into a grey."""
function _inscattered_light(
    direction::Vec4f,
    toward_sun::Vec4f,
    sun_radiance::Vec4f,
    sky_top::Vec4f,
    sky_horizon::Vec4f,
    sky::Vec4f,
    sun_scatter_gain::Float32,
)::Vec4f
    base = _sky_radiance(direction, sky_top, sky_horizon, toward_sun, sky)
    sun = _sun_scatter(direction, toward_sun, sun_radiance, sun_scatter_gain)
    return Vec4f(base[1] + sun[1], base[2] + sun[2], base[3] + sun[3], 1.0f0)
end

"""Haze weight `1 − exp(−τ)` between the camera and a point.

σ(h) = σ₀·exp(−k·h) above the datum y = 0; `atmosphere = (enabled, k, σ₀,
sun gain)`. The optical depth along the segment integrates exactly:
τ = σ₀·exp(−k·h_c)·d·(1 − exp(−k·Δh))/(k·Δh). For |k·Δh| < 1e-3 (including
k = 0, a homogeneous haze) the ratio is replaced by its first-order series,
which is exact to float precision there and avoids the 0/0.
"""
function _haze_weight(
    camera_height::Float32,
    point_height::Float32,
    distance::Float32,
    atmosphere::Vec4f,
)::Float32
    falloff = atmosphere[2]
    rise = falloff * (point_height - camera_height)
    ratio = abs(rise) < 1.0f-3 ? 1.0f0 - 0.5f0 * rise : (1.0f0 - exp(-rise)) / rise
    depth = atmosphere[3] * exp(-falloff * camera_height) * distance * ratio
    return 1.0f0 - exp(-depth)
end

"""Fog for one shaded surface point.

`atmosphere[1] < 0.5` is the historical linear fog (`_apply_fog`, unchanged),
so packets without `render_policy.atmosphere` keep their bytes. With it, the
surface fades toward `_inscattered_light` by `_haze_weight`; the packet's
`fog_color_rgb` and `fog_density` are not used, because the haze IS the sky.
"""
function _apply_aerial_perspective(
    color::Vec4f,
    world_position::Vec4f,
    camera_position::Vec4f,
    distance::Float32,
    fog_color::Vec4f,
    fog_density::Float32,
    light_direction::Vec4f,
    light_color::Vec4f,
    light_intensity::Float32,
    sky_top::Vec4f,
    sky_horizon::Vec4f,
    sky::Vec4f,
    atmosphere::Vec4f,
)::Vec4f
    atmosphere[1] > 0.5f0 || return _apply_fog(color, fog_color, distance, fog_density)
    weight = _haze_weight(camera_position[2], world_position[2], distance, atmosphere)
    direction = _normalize_vector(Vec4f(
        world_position[1] - camera_position[1],
        world_position[2] - camera_position[2],
        world_position[3] - camera_position[3],
        0.0f0,
    ))
    toward_sun = _normalize_vector(Vec4f(-light_direction[1], -light_direction[2], -light_direction[3], 0.0f0))
    haze = _inscattered_light(
        direction,
        toward_sun,
        Vec4f(light_color[1] * light_intensity, light_color[2] * light_intensity, light_color[3] * light_intensity, 0.0f0),
        sky_top,
        sky_horizon,
        sky,
        atmosphere[4],
    )
    clear = 1.0f0 - weight
    return Vec4f(
        color[1] * clear + haze[1] * weight,
        color[2] * clear + haze[2] * weight,
        color[3] * clear + haze[3] * weight,
        color[4],
    )
end

# N-2 image-based lighting (render_policy.ibl). The environment atlas and BRDF
# table are baked in Rust (`world_core/.../src/ibl.rs`), which documents the
# layout these constants mirror: six GGX-prefiltered octahedral tiles (perceptual
# roughness k/5, 128² down to 4²) and a 32² irradiance tile, each with a one-texel
# gutter, in one single-level 298 x 130 texture. Lava samples with implicit LOD
# only, so roughness levels are tiles blended here, not a mip chain.
const IBL_RADIANCE_SCALE = 8.0f0
const IBL_ATLAS_WIDTH = 298.0f0
const IBL_ATLAS_HEIGHT = 130.0f0
const IBL_IRRADIANCE_ORIGIN = 264.0f0
const IBL_ENVIRONMENT_BINDING = UInt32(14)
const IBL_LUT_BINDING = UInt32(15)

"""Octahedral encode (+Y up) of a unit direction to [0, 1]²; mirrors `ibl::octahedral_encode`."""
@inline function _octahedral_uv(d::Vec4f)::Vec2f
    s = abs(d[1]) + abs(d[2]) + abs(d[3])
    x = d[1] / s
    z = d[3] / s
    if d[2] < 0.0f0
        sign_x = x >= 0.0f0 ? 1.0f0 : -1.0f0
        sign_z = z >= 0.0f0 ? 1.0f0 : -1.0f0
        folded_x = (1.0f0 - abs(z)) * sign_x
        z = (1.0f0 - abs(x)) * sign_z
        x = folded_x
    end
    return Vec2f(x * 0.5f0 + 0.5f0, z * 0.5f0 + 0.5f0)
end

@inline function _ibl_tile(origin::Float32, size::Float32, uv::Vec2f)::Vec4f
    sampled = _sample_texture(
        IBL_ENVIRONMENT_BINDING,
        Vec2f((origin + 1.0f0 + uv[1] * size) / IBL_ATLAS_WIDTH, (1.0f0 + uv[2] * size) / IBL_ATLAS_HEIGHT),
    )
    return Vec4f(sampled[1] * IBL_RADIANCE_SCALE, sampled[2] * IBL_RADIANCE_SCALE, sampled[3] * IBL_RADIANCE_SCALE, 1.0f0)
end

@inline _ibl_level_origin(level::Int32)::Float32 =
    level == Int32(0) ? 0.0f0 : level == Int32(1) ? 130.0f0 : level == Int32(2) ? 196.0f0 :
    level == Int32(3) ? 230.0f0 : level == Int32(4) ? 248.0f0 : 258.0f0

@inline _ibl_level_size(level::Int32)::Float32 = Float32(Int32(128) >> level)

# Where `_material_response` gets IBL values from. Static dispatch keeps the
# texture intrinsics out of every compiled function that does not ask for
# them, so `_material_response` stays callable on the CPU (tests) with the
# default `NoIbl`, and tests can drive the real IBL arithmetic with
# `ConstantIbl`. Shaders pass `TextureIbl()`.
struct NoIbl end
struct TextureIbl end
struct ConstantIbl
    irradiance::Vec4f
    prefiltered::Vec4f
    brdf::Vec2f
end
@inline _ibl_irradiance(::NoIbl, ::Vec4f)::Vec4f = Vec4f(0.0f0, 0.0f0, 0.0f0, 0.0f0)
@inline _ibl_prefiltered(::NoIbl, ::Vec4f, ::Float32)::Vec4f = Vec4f(0.0f0, 0.0f0, 0.0f0, 0.0f0)
@inline _ibl_brdf(::NoIbl, ::Float32, ::Float32)::Vec2f = Vec2f(0.0f0, 0.0f0)
@inline _ibl_irradiance(source::ConstantIbl, ::Vec4f)::Vec4f = source.irradiance
@inline _ibl_prefiltered(source::ConstantIbl, ::Vec4f, ::Float32)::Vec4f = source.prefiltered
@inline _ibl_brdf(source::ConstantIbl, ::Float32, ::Float32)::Vec2f = source.brdf

"""GGX-prefiltered radiance along `reflection` at perceptual `roughness`: the
two bracketing tiles, blended linearly in roughness."""
@inline function _ibl_prefiltered(::TextureIbl, reflection::Vec4f, roughness::Float32)::Vec4f
    uv = _octahedral_uv(_normalize_vector(reflection))
    level = clamp(roughness, 0.0f0, 1.0f0) * 5.0f0
    lower = min(Int32(floor(level)), Int32(4))
    t = level - Float32(lower)
    a = _ibl_tile(_ibl_level_origin(lower), _ibl_level_size(lower), uv)
    b = _ibl_tile(_ibl_level_origin(lower + Int32(1)), _ibl_level_size(lower + Int32(1)), uv)
    return _mix4(a, b, t)
end

@inline _ibl_irradiance(::TextureIbl, normal::Vec4f)::Vec4f = _ibl_tile(IBL_IRRADIANCE_ORIGIN, 32.0f0, _octahedral_uv(normal))

"""Split-sum (A, B) for (N·V, roughness) from the baked 64² table."""
@inline function _ibl_brdf(::TextureIbl, normal_view::Float32, roughness::Float32)::Vec2f
    sampled = _sample_texture(IBL_LUT_BINDING, Vec2f(clamp(normal_view, 0.0f0, 1.0f0), clamp(roughness, 0.0f0, 1.0f0)))
    return Vec2f(sampled[1], sampled[2])
end

"""Diagnostic albedo override (render_policy.debug): `debug = (enabled, albedo)`.
Disabled returns `color` unchanged, so packets without the axis keep their bytes."""
@inline function _albedo_override(color::Vec4f, debug::Vec4f)::Vec4f
    debug[1] > 0.5f0 || return color
    return Vec4f(debug[2], debug[2], debug[2], color[4])
end

function _reflect_vector(incident::Vec4f, normal::Vec4f)::Vec4f
    scale = 2.0f0 * _dot_vector(incident, normal)
    return Vec4f(
        incident[1] - scale * normal[1],
        incident[2] - scale * normal[2],
        incident[3] - scale * normal[3],
        0.0f0,
    )
end

function _cross_vector(first::Vec4f, second::Vec4f)::Vec4f
    return Vec4f(
        first[2] * second[3] - first[3] * second[2],
        first[3] * second[1] - first[1] * second[3],
        first[1] * second[2] - first[2] * second[1],
        0.0f0,
    )
end

function _stable_tangent(normal::Vec4f)::Vec4f
    surface_normal = _normalize_vector(normal)
    reference = abs(surface_normal[1]) < 0.9f0 ?
        Vec4f(1.0f0, 0.0f0, 0.0f0, 0.0f0) :
        Vec4f(0.0f0, 0.0f0, 1.0f0, 0.0f0)
    projected_reference = Vec4f(
        reference[1] - surface_normal[1] * _dot_vector(surface_normal, reference),
        reference[2] - surface_normal[2] * _dot_vector(surface_normal, reference),
        reference[3] - surface_normal[3] * _dot_vector(surface_normal, reference),
        0.0f0,
    )
    tangent = _normalize_vector(projected_reference)
    return Vec4f(tangent[1], tangent[2], tangent[3], 1.0f0)
end

function _perturbed_normal(
    normal::Vec4f,
    tangent::Vec4f,
    uv::Vec2f,
    normal_scale::Float32,
)::Vec4f
    surface_normal = _normalize_vector(normal)
    normal_sample = _sample_texture(UInt32(1), uv)
    tangent_space = Vec4f(
        (normal_sample[1] * 2.0f0 - 1.0f0) * clamp(normal_scale, 0.0f0, 2.0f0),
        (normal_sample[2] * 2.0f0 - 1.0f0) * clamp(normal_scale, 0.0f0, 2.0f0),
        max(normal_sample[3] * 2.0f0 - 1.0f0, 0.05f0),
        0.0f0,
    )
    tangent_orthogonal = Vec4f(
        tangent[1] - surface_normal[1] * _dot_vector(surface_normal, tangent),
        tangent[2] - surface_normal[2] * _dot_vector(surface_normal, tangent),
        tangent[3] - surface_normal[3] * _dot_vector(surface_normal, tangent),
        0.0f0,
    )
    tangent_vector = _normalize_vector(tangent_orthogonal)
    bitangent = _normalize_vector(_cross_vector(surface_normal, tangent_vector))
    handedness = tangent[4] < 0.0f0 ? -1.0f0 : 1.0f0
    return _normalize_vector(
        Vec4f(
            tangent_vector[1] * tangent_space[1] + handedness * bitangent[1] * tangent_space[2] + surface_normal[1] * tangent_space[3],
            tangent_vector[2] * tangent_space[1] + handedness * bitangent[2] * tangent_space[2] + surface_normal[2] * tangent_space[3],
            tangent_vector[3] * tangent_space[1] + handedness * bitangent[3] * tangent_space[2] + surface_normal[3] * tangent_space[3],
            0.0f0,
        ),
    )
end

function _material_response(
    base_color::Vec4f,
    normal::Vec4f,
    light_direction::Vec4f,
    light_color::Vec4f,
    light_intensity::Float32,
    environment_top::Vec4f,
    environment_horizon::Vec4f,
    environment_ground::Vec4f,
    metallic::Float32,
    roughness::Float32,
    clearcoat::Float32,
    clearcoat_roughness::Float32,
    shadow_visibility::Float32,
    view_direction::Vec4f,
    roughness_sample::Float32,
    occlusion_sample::Float32,
    occlusion_strength::Float32,
    emissive_factor::Vec4f,
    emissive_sample::Vec4f,
    sky::Vec4f,
    ibl::Float32=0.0f0,
    ibl_source=NoIbl(),
)::Vec4f
    surface_normal = _normalize_vector(normal)
    light_vector = _normalize_vector(
        Vec4f(-light_direction[1], -light_direction[2], -light_direction[3], 0.0f0),
    )
    view_vector = _normalize_vector(view_direction)
    half_vector = _normalize_vector(
        Vec4f(
            light_vector[1] + view_vector[1],
            light_vector[2] + view_vector[2],
            light_vector[3] + view_vector[3],
            0.0f0,
        ),
    )
    normal_light = max(_dot_vector(surface_normal, light_vector), 0.0f0)
    normal_view = max(_dot_vector(surface_normal, view_vector), 0.0f0)
    normal_half = max(_dot_vector(surface_normal, half_vector), 0.0f0)
    view_half = max(_dot_vector(view_vector, half_vector), 0.0f0)
    metalness = clamp(metallic, 0.0f0, 1.0f0)
    surface_roughness = clamp(
        roughness * clamp(roughness_sample, 0.0f0, 1.0f0),
        0.045f0,
        1.0f0,
    )
    alpha = surface_roughness * surface_roughness
    alpha_squared = alpha * alpha
    normal_half_squared = normal_half * normal_half
    distribution_denominator = normal_half_squared * (alpha_squared - 1.0f0) + 1.0f0
    distribution = alpha_squared /
        (3.1415927f0 * distribution_denominator * distribution_denominator + 0.0001f0)
    roughness_plus_one = surface_roughness + 1.0f0
    geometry_k = roughness_plus_one * roughness_plus_one * 0.125f0
    geometry_view = normal_view / (normal_view * (1.0f0 - geometry_k) + geometry_k)
    geometry_light = normal_light / (normal_light * (1.0f0 - geometry_k) + geometry_k)
    geometry = geometry_view * geometry_light
    fresnel_power = 1.0f0 - view_half
    fresnel_power *= fresnel_power
    fresnel_power *= fresnel_power
    fresnel_power *= 1.0f0 - view_half
    f0_red = 0.04f0 * (1.0f0 - metalness) + base_color[1] * metalness
    f0_green = 0.04f0 * (1.0f0 - metalness) + base_color[2] * metalness
    f0_blue = 0.04f0 * (1.0f0 - metalness) + base_color[3] * metalness
    fresnel_red = f0_red + (1.0f0 - f0_red) * fresnel_power
    fresnel_green = f0_green + (1.0f0 - f0_green) * fresnel_power
    fresnel_blue = f0_blue + (1.0f0 - f0_blue) * fresnel_power
    specular_denominator = 4.0f0 * normal_view * normal_light + 0.0001f0
    specular_scale = distribution * geometry / specular_denominator
    coat = clamp(clearcoat, 0.0f0, 1.0f0)
    coat_roughness = clamp(clearcoat_roughness, 0.045f0, 1.0f0)
    coat_alpha = coat_roughness * coat_roughness
    coat_alpha_squared = coat_alpha * coat_alpha
    coat_distribution_denominator = normal_half_squared *
        (coat_alpha_squared - 1.0f0) + 1.0f0
    coat_distribution = coat_alpha_squared /
        (3.1415927f0 * coat_distribution_denominator * coat_distribution_denominator + 0.0001f0)
    coat_roughness_plus_one = coat_roughness + 1.0f0
    coat_geometry_k = coat_roughness_plus_one * coat_roughness_plus_one * 0.125f0
    coat_geometry_view = normal_view /
        (normal_view * (1.0f0 - coat_geometry_k) + coat_geometry_k)
    coat_geometry_light = normal_light /
        (normal_light * (1.0f0 - coat_geometry_k) + coat_geometry_k)
    coat_geometry = coat_geometry_view * coat_geometry_light
    coat_fresnel_power = 1.0f0 - view_half
    coat_fresnel_power *= coat_fresnel_power
    coat_fresnel_power *= coat_fresnel_power
    coat_fresnel_power *= 1.0f0 - view_half
    coat_fresnel = 0.04f0 + (1.0f0 - 0.04f0) * coat_fresnel_power
    coat_specular_scale = coat * coat_fresnel * coat_distribution * coat_geometry /
        specular_denominator
    diffuse_scale = (1.0f0 - metalness) * 0.31830987f0
    direct_scale = light_intensity * normal_light * shadow_visibility
    environment_diffuse = _sky_environment(
        surface_normal,
        environment_top,
        environment_horizon,
        environment_ground,
        light_vector,
        sky,
    )
    reflection = _reflect_vector(
        Vec4f(-view_vector[1], -view_vector[2], -view_vector[3], 0.0f0),
        surface_normal,
    )
    environment_specular = _sky_environment(
        reflection,
        environment_top,
        environment_horizon,
        environment_ground,
        light_vector,
        sky,
    )
    occlusion = 1.0f0 -
        clamp(occlusion_strength, 0.0f0, 1.0f0) *
        (1.0f0 - clamp(occlusion_sample, 0.0f0, 1.0f0))
    coat_environment_scale = coat * coat_fresnel *
        (0.10f0 + 0.16f0 * (1.0f0 - coat_roughness)) * occlusion
    ambient_diffuse_scale = (1.0f0 - metalness) * 0.52f0 * occlusion
    ambient_specular_scale = (0.08f0 + 0.16f0 * (1.0f0 - surface_roughness)) * occlusion
    emissive_red = emissive_factor[1] * emissive_sample[1]
    emissive_green = emissive_factor[2] * emissive_sample[2]
    emissive_blue = emissive_factor[3] * emissive_sample[3]
    if ibl > 0.5f0
        # N-2: diffuse (1 − F)(1 − metal)·albedo·irradiance/π and specular
        # prefiltered·(F0·A + B), F the roughness-aware Schlick term at N·V
        # (Karis 2013 / Fdez-Agüera 2019), all scaled by occlusion. Replaces
        # the un-normalised 0.52 and 0.08 + 0.16(1 − r) constants.
        nv = max(normal_view, 1.0f-4)
        grazing = (1.0f0 - nv) * (1.0f0 - nv)
        grazing = grazing * grazing * (1.0f0 - nv)
        smooth = 1.0f0 - surface_roughness
        fr_red = f0_red + (max(smooth, f0_red) - f0_red) * grazing
        fr_green = f0_green + (max(smooth, f0_green) - f0_green) * grazing
        fr_blue = f0_blue + (max(smooth, f0_blue) - f0_blue) * grazing
        irradiance = _ibl_irradiance(ibl_source, surface_normal)
        specular_radiance = _ibl_prefiltered(ibl_source, reflection, surface_roughness)
        brdf = _ibl_brdf(ibl_source, nv, surface_roughness)
        coat_radiance = _ibl_prefiltered(ibl_source, reflection, coat_roughness)
        coat_scale = coat * coat_fresnel * occlusion
        diffuse_weight = (1.0f0 - metalness) * occlusion
        ibl_red = base_color[1] * (1.0f0 - fr_red) * diffuse_weight * irradiance[1] +
            specular_radiance[1] * (f0_red * brdf[1] + brdf[2]) * occlusion + coat_scale * coat_radiance[1]
        ibl_green = base_color[2] * (1.0f0 - fr_green) * diffuse_weight * irradiance[2] +
            specular_radiance[2] * (f0_green * brdf[1] + brdf[2]) * occlusion + coat_scale * coat_radiance[2]
        ibl_blue = base_color[3] * (1.0f0 - fr_blue) * diffuse_weight * irradiance[3] +
            specular_radiance[3] * (f0_blue * brdf[1] + brdf[2]) * occlusion + coat_scale * coat_radiance[3]
        lit_red = (base_color[1] * diffuse_scale + fresnel_red * specular_scale) * light_color[1] * direct_scale +
            coat_specular_scale * light_color[1] * direct_scale + ibl_red + emissive_red
        lit_green = (base_color[2] * diffuse_scale + fresnel_green * specular_scale) * light_color[2] * direct_scale +
            coat_specular_scale * light_color[2] * direct_scale + ibl_green + emissive_green
        lit_blue = (base_color[3] * diffuse_scale + fresnel_blue * specular_scale) * light_color[3] * direct_scale +
            coat_specular_scale * light_color[3] * direct_scale + ibl_blue + emissive_blue
        return Vec4f(max(lit_red, 0.0f0), max(lit_green, 0.0f0), max(lit_blue, 0.0f0), base_color[4])
    end
    red = (
        (base_color[1] * diffuse_scale + fresnel_red * specular_scale) *
                light_color[1] * direct_scale +
            coat_specular_scale * light_color[1] * direct_scale +
            base_color[1] * ambient_diffuse_scale * environment_diffuse[1] +
            fresnel_red * ambient_specular_scale * environment_specular[1] +
            coat_environment_scale * environment_specular[1] +
            emissive_red
    )
    green = (
        (base_color[2] * diffuse_scale + fresnel_green * specular_scale) *
                light_color[2] * direct_scale +
            coat_specular_scale * light_color[2] * direct_scale +
            base_color[2] * ambient_diffuse_scale * environment_diffuse[2] +
            fresnel_green * ambient_specular_scale * environment_specular[2] +
            coat_environment_scale * environment_specular[2] +
            emissive_green
    )
    blue = (
        (base_color[3] * diffuse_scale + fresnel_blue * specular_scale) *
                light_color[3] * direct_scale +
            coat_specular_scale * light_color[3] * direct_scale +
            base_color[3] * ambient_diffuse_scale * environment_diffuse[3] +
            fresnel_blue * ambient_specular_scale * environment_specular[3] +
            coat_environment_scale * environment_specular[3] +
            emissive_blue
    )
    return Vec4f(max(red, 0.0f0), max(green, 0.0f0), max(blue, 0.0f0), base_color[4])
end

function _sample_texture(binding::UInt32, uv::Vec2f)::Vec4f
    return Vec4f(
        Lava.sample_texture_2d(binding, uv[1], uv[2], UInt32(0)),
        Lava.sample_texture_2d(binding, uv[1], uv[2], UInt32(1)),
        Lava.sample_texture_2d(binding, uv[1], uv[2], UInt32(2)),
        Lava.sample_texture_2d(binding, uv[1], uv[2], UInt32(3)),
    )
end

function _textured_color(
    base_color::Vec4f,
    uv::Vec2f,
    texture_enabled::Float32,
)::Vec4f
    sampled = _sample_texture(UInt32(0), uv)
    weight = clamp(texture_enabled, 0.0f0, 1.0f0)
    return Vec4f(
        base_color[1] * (1.0f0 - weight + weight * sampled[1]),
        base_color[2] * (1.0f0 - weight + weight * sampled[2]),
        base_color[3] * (1.0f0 - weight + weight * sampled[3]),
        # Alpha is coverage. Filtering an opaque texture at an extreme grazing
        # angle returned 1.0000001 (CALIBRATION-1 grazing view), which the
        # capture check rightly refuses; clamped, every alpha <= 1 is unchanged.
        clamp(base_color[4] * (1.0f0 - weight + weight * sampled[4]), 0.0f0, 1.0f0),
    )
end

function _apply_fog(
    color::Vec4f,
    fog_color::Vec4f,
    distance::Float32,
    density::Float32,
)::Vec4f
    fog_weight = clamp(distance * density, 0.0f0, 0.92f0)
    clear_weight = 1.0f0 - fog_weight
    return Vec4f(
        color[1] * clear_weight + fog_color[1] * fog_weight,
        color[2] * clear_weight + fog_color[2] * fog_weight,
        color[3] * clear_weight + fog_color[3] * fog_weight,
        color[4],
    )
end

function _distance_between(first::Vec4f, second::Vec4f)::Float32
    delta_x = first[1] - second[1]
    delta_y = first[2] - second[2]
    delta_z = first[3] - second[3]
    return sqrt(delta_x * delta_x + delta_y * delta_y + delta_z * delta_z)
end

function _project_world(
    world_position::Vec4f,
    camera_position::Vec4f,
    camera_right::Vec4f,
    camera_up::Vec4f,
    camera_forward::Vec4f,
    camera_projection::Vec4f,
    camera_mode::Float32,
)::Vec4f
    relative = Vec4f(
        world_position[1] - camera_position[1],
        world_position[2] - camera_position[2],
        world_position[3] - camera_position[3],
        0.0f0,
    )
    horizontal = _dot_vector(camera_right, relative)
    vertical = _dot_vector(camera_up, relative)
    forward_distance = _dot_vector(camera_forward, relative)
    perspective_distance = max(forward_distance, camera_projection[3])
    scale = 1.0f0 - camera_mode + camera_mode * perspective_distance
    ndc_x = horizontal / (camera_projection[1] * scale)
    # WGE camera up is a semantic world-space basis. Vulkan's positive-height
    # viewport maps positive NDC Y toward the framebuffer's lower rows, so
    # negate the vertical component at this single lowering boundary. Keeping
    # the flip here makes raster geometry, overlays, measurements, and shadow
    # projections agree without burdening the engine-neutral packet contract.
    ndc_y = -vertical / (camera_projection[2] * scale)
    depth_range = camera_projection[4] - camera_projection[3]
    depth = camera_mode < 0.5f0 ?
        (forward_distance - camera_projection[3]) / depth_range :
        camera_projection[4] * (forward_distance - camera_projection[3]) /
        (depth_range * perspective_distance)
    return Vec4f(ndc_x, ndc_y, depth, 1.0f0)
end

"""Return homogeneous clip coordinates for a raster vertex.

`_project_world` intentionally returns NDC for CPU-side visibility and for
shadow-map lookup values. Rasterization needs the undivided form, however:
perspective interpolation is defined by the clip-space `w` component.
"""
function _project_clip(
    world_position::Vec4f,
    camera_position::Vec4f,
    camera_right::Vec4f,
    camera_up::Vec4f,
    camera_forward::Vec4f,
    camera_projection::Vec4f,
    camera_mode::Float32,
)::Vec4f
    relative = Vec4f(
        world_position[1] - camera_position[1],
        world_position[2] - camera_position[2],
        world_position[3] - camera_position[3],
        0.0f0,
    )
    horizontal = _dot_vector(camera_right, relative)
    vertical = _dot_vector(camera_up, relative)
    forward_distance = _dot_vector(camera_forward, relative)
    depth_range = camera_projection[4] - camera_projection[3]
    if camera_mode < 0.5f0
        return Vec4f(
            horizontal / camera_projection[1],
            -vertical / camera_projection[2],
            (forward_distance - camera_projection[3]) / depth_range,
            1.0f0,
        )
    end
    clip_depth = camera_projection[4] * (forward_distance - camera_projection[3]) / depth_range
    return Vec4f(
        horizontal / camera_projection[1],
        -vertical / camera_projection[2],
        clip_depth,
        forward_distance,
    )
end

function _project_overlay_point(
    frame::CameraFrame,
    world_position::NTuple{3,Float32},
)::Union{Nothing,Vec4f}
    relative = Vec4f(
        world_position[1] - frame.position[1],
        world_position[2] - frame.position[2],
        world_position[3] - frame.position[3],
        0.0f0,
    )
    forward_distance = _dot_vector(frame.forward, relative)
    frame.projection[3] <= forward_distance <= frame.projection[4] || return nothing
    return _project_point(frame, world_position)
end

function _terrain_normal(
    heights::Lava.LavaDeviceArray{Float32,1},
    resolution::Int32,
    sample_x::Int32,
    sample_z::Int32,
    width_m::Float32,
    length_m::Float32,
)::Vec4f
    cells_per_axis = resolution - Int32(1)
    left_x = max(sample_x - Int32(1), Int32(0))
    right_x = min(sample_x + Int32(1), cells_per_axis)
    near_z = max(sample_z - Int32(1), Int32(0))
    far_z = min(sample_z + Int32(1), cells_per_axis)
    left_height = heights[sample_z*resolution+left_x+Int32(1)]
    right_height = heights[sample_z*resolution+right_x+Int32(1)]
    near_height = heights[near_z*resolution+sample_x+Int32(1)]
    far_height = heights[far_z*resolution+sample_x+Int32(1)]
    horizontal_step = width_m / Float32(cells_per_axis)
    depth_step = length_m / Float32(cells_per_axis)
    tangent_x = Vec4f(
        Float32(right_x - left_x) * horizontal_step,
        right_height - left_height,
        0.0f0,
        0.0f0,
    )
    tangent_z = Vec4f(
        0.0f0,
        far_height - near_height,
        -Float32(far_z - near_z) * depth_step,
        0.0f0,
    )
    return _normalize_vector(
        Vec4f(
            tangent_x[2] * tangent_z[3] - tangent_x[3] * tangent_z[2],
            tangent_x[3] * tangent_z[1] - tangent_x[1] * tangent_z[3],
            tangent_x[1] * tangent_z[2] - tangent_x[2] * tangent_z[1],
            0.0f0,
        ),
    )
end

function _terrain_tangent(
    heights::Lava.LavaDeviceArray{Float32,1},
    resolution::Int32,
    sample_x::Int32,
    sample_z::Int32,
    width_m::Float32,
    ::Float32,
)::Vec4f
    cells_per_axis = resolution - Int32(1)
    left_x = max(sample_x - Int32(1), Int32(0))
    right_x = min(sample_x + Int32(1), cells_per_axis)
    left_height = heights[sample_z*resolution+left_x+Int32(1)]
    right_height = heights[sample_z*resolution+right_x+Int32(1)]
    horizontal_step = width_m / Float32(cells_per_axis)
    return _normalize_vector(Vec4f(
        Float32(right_x - left_x) * horizontal_step,
        right_height - left_height,
        0.0f0,
        0.0f0,
    ))
end

@inline function _terrain_base_color(
    base_color::Vec4f,
    ::Float32,
    ::UInt8,
)::Vec4f
    # Slope already affects the geometric normal and therefore receives
    # direction-dependent lighting. Region codes are categorical traversal
    # labels, not an ordered material palette. Keep albedo tied to the typed
    # material until the packet carries an explicit surface-layer mapping.
    return base_color
end

"""Every terrain vertex output (locations 0-17), shared by the single-material
and the layered terrain pipelines so their geometry cannot drift apart."""
@inline function _emit_terrain_vertex(
    heights::Lava.LavaDeviceArray{Float32,1},
    slopes::Lava.LavaDeviceArray{Float32,1},
    regions::Lava.LavaDeviceArray{UInt8,1},
    resolution::Int32,
    width_m::Float32,
    length_m::Float32,
    camera_position::Vec4f,
    camera_right::Vec4f,
    camera_up::Vec4f,
    camera_forward::Vec4f,
    camera_projection::Vec4f,
    camera_mode::Float32,
    light_position::Vec4f,
    light_right::Vec4f,
    light_up::Vec4f,
    light_forward::Vec4f,
    light_projection::Vec4f,
    light_mode::Float32,
    base_color::Vec4f,
    metallic::Float32,
    roughness::Float32,
    clearcoat::Float32,
    clearcoat_roughness::Float32,
    normal_scale::Float32,
    occlusion_strength::Float32,
    light_direction::Vec4f,
    light_color::Vec4f,
    light_intensity::Float32,
    environment_top::Vec4f,
    environment_horizon::Vec4f,
    environment_ground::Vec4f,
    fog_color::Vec4f,
    fog_density::Float32,
    exposure::Float32,
    texture_enabled::Float32,
    emissive_factor::Vec4f,
    shadow::Vec4f,
    uv_repeat::Float32,
    sky_parameters::Vec4f,
    atmosphere_parameters::Vec4f,
    surface_options::Vec4f,
)
    vertex_id = Lava.vertex_index() - Int32(1)
    cells_per_axis = resolution - Int32(1)
    cell_id = div(vertex_id, Int32(6))
    corner = rem(vertex_id, Int32(6))
    cell_x = rem(cell_id, cells_per_axis)
    cell_z = div(cell_id, cells_per_axis)
    right = corner == Int32(1) || corner == Int32(2) || corner == Int32(4)
    far = corner == Int32(2) || corner == Int32(4) || corner == Int32(5)
    sample_x = cell_x + (right ? Int32(1) : Int32(0))
    sample_z = cell_z + (far ? Int32(1) : Int32(0))
    sample_index = sample_z * resolution + sample_x + Int32(1)
    height = heights[sample_index]
    slope = slopes[sample_index]
    region = regions[sample_index]
    normalized_x = Float32(sample_x) / Float32(cells_per_axis)
    normalized_z = Float32(sample_z) / Float32(cells_per_axis)
    world_x = (normalized_x - 0.5f0) * width_m
    world_z = (0.5f0 - normalized_z) * length_m
    world_position = Vec4f(world_x, height, world_z, 1.0f0)
    Lava.set_position!(
        _project_clip(
            world_position,
            camera_position,
            camera_right,
            camera_up,
            camera_forward,
            camera_projection,
            camera_mode,
        ),
    )
    terrain_color = _terrain_base_color(base_color, slope, region)
    # Multiplied, not recomputed from world metres. At the declared default
    # `uv_repeat == 1.0f0` this is a multiplication by exactly one, so the frozen
    # captures are bit-identical. A world-metre formulation would be prettier to
    # read but would introduce a float round-trip through `width_m` that changes
    # the low bits of every UV — a byte-identity regression for no gain, since
    # the physical scale is already expressed by `uv_repeat`.
    uv = Vec2f(normalized_x * uv_repeat, normalized_z * uv_repeat)
    terrain_normal = _terrain_normal(heights, resolution, sample_x, sample_z, width_m, length_m)
    Lava.gfx_output(0, terrain_color)
    Lava.gfx_output(1, terrain_normal)
    Lava.gfx_output(2, world_position)
    Lava.gfx_output(3, uv)
    Lava.gfx_output(4, Vec4f(metallic, roughness, normal_scale, occlusion_strength))
    Lava.gfx_output(5, light_direction)
    Lava.gfx_output(6, light_color)
    Lava.gfx_output(7, Vec4f(light_intensity, fog_density, exposure, texture_enabled))
    Lava.gfx_output(8, environment_top)
    Lava.gfx_output(9, environment_horizon)
    Lava.gfx_output(10, environment_ground)
    Lava.gfx_output(11, fog_color)
    Lava.gfx_output(12, camera_position)
    Lava.gfx_output(
        13,
        _project_world(
            world_position,
            light_position,
            light_right,
            light_up,
            light_forward,
            light_projection,
            light_mode,
        ),
    )
    Lava.gfx_output(14, emissive_factor)
    Lava.gfx_output(15, Vec4f(clearcoat, clearcoat_roughness, 0.0f0, 0.0f0))
    Lava.gfx_output(
        16,
        _terrain_tangent(heights, resolution, sample_x, sample_z, width_m, length_m),
    )
    Lava.gfx_output(17, shadow)
    Lava.gfx_output(18, sky_parameters)
    Lava.gfx_output(19, atmosphere_parameters)
    Lava.gfx_output(20, surface_options)
    return nothing
end

function _terrain_vertex(
    heights::Lava.LavaDeviceArray{Float32,1},
    slopes::Lava.LavaDeviceArray{Float32,1},
    regions::Lava.LavaDeviceArray{UInt8,1},
    resolution::Int32,
    width_m::Float32,
    length_m::Float32,
    camera_position::Vec4f,
    camera_right::Vec4f,
    camera_up::Vec4f,
    camera_forward::Vec4f,
    camera_projection::Vec4f,
    camera_mode::Float32,
    light_position::Vec4f,
    light_right::Vec4f,
    light_up::Vec4f,
    light_forward::Vec4f,
    light_projection::Vec4f,
    light_mode::Float32,
    base_color::Vec4f,
    metallic::Float32,
    roughness::Float32,
    clearcoat::Float32,
    clearcoat_roughness::Float32,
    normal_scale::Float32,
    occlusion_strength::Float32,
    light_direction::Vec4f,
    light_color::Vec4f,
    light_intensity::Float32,
    environment_top::Vec4f,
    environment_horizon::Vec4f,
    environment_ground::Vec4f,
    fog_color::Vec4f,
    fog_density::Float32,
    exposure::Float32,
    texture_enabled::Float32,
    emissive_factor::Vec4f,
    shadow::Vec4f,
    uv_repeat::Float32,
    sky_parameters::Vec4f,
    atmosphere_parameters::Vec4f,
    surface_options::Vec4f,
)
    _emit_terrain_vertex(heights, slopes, regions, resolution, width_m, length_m, camera_position, camera_right, camera_up, camera_forward, camera_projection, camera_mode, light_position, light_right, light_up, light_forward, light_projection, light_mode, base_color, metallic, roughness, clearcoat, clearcoat_roughness, normal_scale, occlusion_strength, light_direction, light_color, light_intensity, environment_top, environment_horizon, environment_ground, fog_color, fog_density, exposure, texture_enabled, emissive_factor, shadow, uv_repeat, sky_parameters, atmosphere_parameters, surface_options)
    return nothing
end

"""Layered terrain (N-4): the shared terrain outputs plus seven uniforms at
locations 18-24 — two per layer (see `_terrain_layer_uniforms`) and the macro
parameters. Fragment shaders here receive uniforms only through varyings."""
function _terrain_layered_vertex(
    heights::Lava.LavaDeviceArray{Float32,1},
    slopes::Lava.LavaDeviceArray{Float32,1},
    regions::Lava.LavaDeviceArray{UInt8,1},
    resolution::Int32,
    width_m::Float32,
    length_m::Float32,
    camera_position::Vec4f,
    camera_right::Vec4f,
    camera_up::Vec4f,
    camera_forward::Vec4f,
    camera_projection::Vec4f,
    camera_mode::Float32,
    light_position::Vec4f,
    light_right::Vec4f,
    light_up::Vec4f,
    light_forward::Vec4f,
    light_projection::Vec4f,
    light_mode::Float32,
    base_color::Vec4f,
    metallic::Float32,
    roughness::Float32,
    clearcoat::Float32,
    clearcoat_roughness::Float32,
    normal_scale::Float32,
    occlusion_strength::Float32,
    light_direction::Vec4f,
    light_color::Vec4f,
    light_intensity::Float32,
    environment_top::Vec4f,
    environment_horizon::Vec4f,
    environment_ground::Vec4f,
    fog_color::Vec4f,
    fog_density::Float32,
    exposure::Float32,
    texture_enabled::Float32,
    emissive_factor::Vec4f,
    shadow::Vec4f,
    uv_repeat::Float32,
    sky_parameters::Vec4f,
    atmosphere_parameters::Vec4f,
    surface_options::Vec4f,
    layer0_a::Vec4f,
    layer0_b::Vec4f,
    layer1_a::Vec4f,
    layer1_b::Vec4f,
    layer2_a::Vec4f,
    layer2_b::Vec4f,
    macro_parameters::Vec4f,
)
    _emit_terrain_vertex(heights, slopes, regions, resolution, width_m, length_m, camera_position, camera_right, camera_up, camera_forward, camera_projection, camera_mode, light_position, light_right, light_up, light_forward, light_projection, light_mode, base_color, metallic, roughness, clearcoat, clearcoat_roughness, normal_scale, occlusion_strength, light_direction, light_color, light_intensity, environment_top, environment_horizon, environment_ground, fog_color, fog_density, exposure, texture_enabled, emissive_factor, shadow, uv_repeat, sky_parameters, atmosphere_parameters, surface_options)
    Lava.gfx_output(21, layer0_a)
    Lava.gfx_output(22, layer0_b)
    Lava.gfx_output(23, layer1_a)
    Lava.gfx_output(24, layer1_b)
    Lava.gfx_output(25, layer2_a)
    Lava.gfx_output(26, layer2_b)
    Lava.gfx_output(27, macro_parameters)
    return nothing
end

function _mesh_vertex(
    positions::Lava.LavaDeviceArray{Vec4f,1},
    normals::Lava.LavaDeviceArray{Vec4f,1},
    uvs::Lava.LavaDeviceArray{Vec2f,1},
    tangents::Lava.LavaDeviceArray{Vec4f,1},
    translations::Lava.LavaDeviceArray{Vec4f,1},
    rotations::Lava.LavaDeviceArray{Vec4f,1},
    scales::Lava.LavaDeviceArray{Vec4f,1},
    colors::Lava.LavaDeviceArray{Vec4f,1},
    material_parameters::Lava.LavaDeviceArray{Vec4f,1},
    surface_parameters::Lava.LavaDeviceArray{Vec4f,1},
    emissive_parameters::Lava.LavaDeviceArray{Vec4f,1},
    camera_position::Vec4f,
    camera_right::Vec4f,
    camera_up::Vec4f,
    camera_forward::Vec4f,
    camera_projection::Vec4f,
    camera_mode::Float32,
    light_position::Vec4f,
    light_right::Vec4f,
    light_up::Vec4f,
    light_forward::Vec4f,
    light_projection::Vec4f,
    light_mode::Float32,
    light_direction::Vec4f,
    light_color::Vec4f,
    light_intensity::Float32,
    environment_top::Vec4f,
    environment_horizon::Vec4f,
    environment_ground::Vec4f,
    fog_color::Vec4f,
    fog_density::Float32,
    exposure::Float32,
    texture_enabled::Float32,
    shadow::Vec4f,
    sky_parameters::Vec4f,
    atmosphere_parameters::Vec4f,
    surface_options::Vec4f,
)
    vertex_id = Lava.vertex_index()
    instance_id = Lava.instance_index()
    local_position = positions[vertex_id]
    local_normal = normals[vertex_id]
    local_tangent = tangents[vertex_id]
    translation = translations[instance_id]
    rotation = rotations[instance_id]
    scale = scales[instance_id]
    base_color = colors[instance_id]
    material = material_parameters[instance_id]
    scaled_position = Vec4f(
        local_position[1] * scale[1],
        local_position[2] * scale[2],
        local_position[3] * scale[3],
        0.0f0,
    )
    rotated_position = _rotate_vector(rotation, scaled_position)
    world_position = Vec4f(
        rotated_position[1] + translation[1],
        rotated_position[2] + translation[2],
        rotated_position[3] + translation[3],
        1.0f0,
    )
    inverse_scaled_normal = Vec4f(
        local_normal[1] / scale[1],
        local_normal[2] / scale[2],
        local_normal[3] / scale[3],
        0.0f0,
    )
    normal = _normalize_vector(_rotate_vector(rotation, inverse_scaled_normal))
    tangent = _transform_tangent(rotation, local_tangent, scale)
    Lava.set_position!(
        _project_clip(
            world_position,
            camera_position,
            camera_right,
            camera_up,
            camera_forward,
            camera_projection,
            camera_mode,
        ),
    )
    uv = uvs[vertex_id]
    Lava.gfx_output(0, base_color)
    Lava.gfx_output(1, normal)
    Lava.gfx_output(2, world_position)
    Lava.gfx_output(3, uv)
    Lava.gfx_output(4, material)
    Lava.gfx_output(5, light_direction)
    Lava.gfx_output(6, light_color)
    Lava.gfx_output(7, Vec4f(light_intensity, fog_density, exposure, texture_enabled))
    Lava.gfx_output(8, environment_top)
    Lava.gfx_output(9, environment_horizon)
    Lava.gfx_output(10, environment_ground)
    Lava.gfx_output(11, fog_color)
    Lava.gfx_output(12, camera_position)
    Lava.gfx_output(
        13,
        _project_world(
            world_position,
            light_position,
            light_right,
            light_up,
            light_forward,
            light_projection,
            light_mode,
        ),
    )
    Lava.gfx_output(14, emissive_parameters[instance_id])
    Lava.gfx_output(15, surface_parameters[instance_id])
    Lava.gfx_output(16, tangent)
    Lava.gfx_output(17, shadow)
    Lava.gfx_output(18, sky_parameters)
    Lava.gfx_output(19, atmosphere_parameters)
    Lava.gfx_output(20, surface_options)
    return nothing
end

function _terrain_shadow_vertex(
    heights::Lava.LavaDeviceArray{Float32,1},
    resolution::Int32,
    width_m::Float32,
    length_m::Float32,
    light_position::Vec4f,
    light_right::Vec4f,
    light_up::Vec4f,
    light_forward::Vec4f,
    light_projection::Vec4f,
    light_mode::Float32,
)
    vertex_id = Lava.vertex_index() - Int32(1)
    cells_per_axis = resolution - Int32(1)
    cell_id = div(vertex_id, Int32(6))
    corner = rem(vertex_id, Int32(6))
    cell_x = rem(cell_id, cells_per_axis)
    cell_z = div(cell_id, cells_per_axis)
    right = corner == Int32(1) || corner == Int32(2) || corner == Int32(4)
    far = corner == Int32(2) || corner == Int32(4) || corner == Int32(5)
    sample_x = cell_x + (right ? Int32(1) : Int32(0))
    sample_z = cell_z + (far ? Int32(1) : Int32(0))
    sample_index = sample_z * resolution + sample_x + Int32(1)
    normalized_x = Float32(sample_x) / Float32(cells_per_axis)
    normalized_z = Float32(sample_z) / Float32(cells_per_axis)
    world_position = Vec4f(
        (normalized_x - 0.5f0) * width_m,
        heights[sample_index],
        (0.5f0 - normalized_z) * length_m,
        1.0f0,
    )
    light_space = _project_world(
        world_position,
        light_position,
        light_right,
        light_up,
        light_forward,
        light_projection,
        light_mode,
    )
    Lava.set_position!(
        _project_clip(
            world_position,
            light_position,
            light_right,
            light_up,
            light_forward,
            light_projection,
            light_mode,
        ),
    )
    Lava.gfx_output(0, light_space)
    return nothing
end

function _mesh_shadow_vertex(
    positions::Lava.LavaDeviceArray{Vec4f,1},
    translations::Lava.LavaDeviceArray{Vec4f,1},
    rotations::Lava.LavaDeviceArray{Vec4f,1},
    scales::Lava.LavaDeviceArray{Vec4f,1},
    light_position::Vec4f,
    light_right::Vec4f,
    light_up::Vec4f,
    light_forward::Vec4f,
    light_projection::Vec4f,
    light_mode::Float32,
)
    vertex_id = Lava.vertex_index()
    instance_id = Lava.instance_index()
    local_position = positions[vertex_id]
    translation = translations[instance_id]
    rotation = rotations[instance_id]
    scale = scales[instance_id]
    scaled_position = Vec4f(
        local_position[1] * scale[1],
        local_position[2] * scale[2],
        local_position[3] * scale[3],
        0.0f0,
    )
    rotated_position = _rotate_vector(rotation, scaled_position)
    world_position = Vec4f(
        rotated_position[1] + translation[1],
        rotated_position[2] + translation[2],
        rotated_position[3] + translation[3],
        1.0f0,
    )
    light_space = _project_world(
        world_position,
        light_position,
        light_right,
        light_up,
        light_forward,
        light_projection,
        light_mode,
    )
    Lava.set_position!(
        _project_clip(
            world_position,
            light_position,
            light_right,
            light_up,
            light_forward,
            light_projection,
            light_mode,
        ),
    )
    Lava.gfx_output(0, light_space)
    return nothing
end

function _mesh_cutout_shadow_vertex(
    positions::Lava.LavaDeviceArray{Vec4f,1},
    uvs::Lava.LavaDeviceArray{Vec2f,1},
    translations::Lava.LavaDeviceArray{Vec4f,1},
    rotations::Lava.LavaDeviceArray{Vec4f,1},
    scales::Lava.LavaDeviceArray{Vec4f,1},
    light_position::Vec4f,
    light_right::Vec4f,
    light_up::Vec4f,
    light_forward::Vec4f,
    light_projection::Vec4f,
    light_mode::Float32,
    coverage::Vec4f,
)
    _mesh_shadow_vertex(
        positions,
        translations,
        rotations,
        scales,
        light_position,
        light_right,
        light_up,
        light_forward,
        light_projection,
        light_mode,
    )
    Lava.gfx_output(1, uvs[Lava.vertex_index()])
    Lava.gfx_output(2, coverage)
    return nothing
end

"""Shadow caster for alpha-mask materials. `coverage` is (base alpha, texture
enabled, cutoff, 0); the albedo is the only texture bound (binding 0)."""
function _shadow_cutout_fragment()
    light_space = Lava.gfx_input(Vec4f, 0)
    uv = Lava.gfx_input(Vec2f, 1)
    coverage = Lava.gfx_input(Vec4f, 2)
    weight = clamp(coverage[2], 0.0f0, 1.0f0)
    alpha = coverage[1] * (1.0f0 - weight + weight * Lava.sample_texture_2d(UInt32(0), uv[1], uv[2], UInt32(3)))
    if alpha < coverage[3]
        Lava.discard()
    end
    depth = clamp(light_space[3], 0.0f0, 1.0f0)
    Lava.gfx_output(0, Vec4f(depth, depth, depth, 1.0f0))
    return nothing
end

function _shadow_fragment()
    light_space = Lava.gfx_input(Vec4f, 0)
    depth = clamp(light_space[3], 0.0f0, 1.0f0)
    Lava.gfx_output(0, Vec4f(depth, depth, depth, 1.0f0))
    return nothing
end

function _shadow_depth(u::Float32, v::Float32)::Float32
    return Lava.sample_texture_2d(UInt32(5), u, v, UInt32(0))
end

"""Directional shadow lookup: 4-tap PCF with a policy-driven darkness floor.

The historical adapter returned `0.25 + 0.75 * pcf`, i.e. a shadowed surface
could never receive less than 25% of direct light. The audit called that out as
the single reason the image reads as ambient-lit and toy-like: contact shadows
were not missing, they were UNREPRESENTABLE.

`darkness` and `filter_texel` are policy parameters rather than constants, so
the baseline value (0.75 / one texel) still reproduces the frozen captures
byte-for-byte while a candidate policy can ask for a genuinely dark, softer
shadow. `min_visibility = 1 - darkness`.

Taps are unrolled because Lava's shader JIT cannot lower a loop with a
constant bound (see `_apply_bloom`).
"""
@inline function _shadow_visibility(light_space::Vec4f, darkness::Float32, filter_texel::Float32)::Float32
    inside =
        -1.0f0 <= light_space[1] <= 1.0f0 &&
            -1.0f0 <= light_space[2] <= 1.0f0 &&
            0.0f0 <= light_space[3] <= 1.0f0
    inside || return 1.0f0
    uv_x = light_space[1] * 0.5f0 + 0.5f0
    uv_y = light_space[2] * 0.5f0 + 0.5f0
    texel = filter_texel / 512.0f0
    bias = 0.0035f0
    depth = light_space[3] - bias
    visible = 0.0f0
    visible += depth <= _shadow_depth(uv_x - texel, uv_y - texel) ? 1.0f0 : 0.0f0
    visible += depth <= _shadow_depth(uv_x + texel, uv_y - texel) ? 1.0f0 : 0.0f0
    visible += depth <= _shadow_depth(uv_x - texel, uv_y + texel) ? 1.0f0 : 0.0f0
    visible += depth <= _shadow_depth(uv_x + texel, uv_y + texel) ? 1.0f0 : 0.0f0
    pcf = visible * 0.25f0
    return (1.0f0 - darkness) + darkness * pcf
end

"""Lower the shadow policy into the flat wire form the scene shaders take.

`(darkness, filter_texel, _, _)`. The defaults 7500/1000 reproduce the historical
`0.25 + 0.75 * pcf` with a one-texel tap spread EXACTLY — `1.0f0 - 0.75f0` is
0.25f0 in Float32 and `filter_texel = 1.0f0` gives `1.0f0/512.0f0` — so an
absent policy is byte-identical, which is the whole premise of this channel.
"""
@inline function _shadow_uniform(policy::WGEGraphics.RenderPolicy)::Vec4f
    shadow = policy.shadow
    return Vec4f(
        _bp(shadow.darkness_bp),
        Float32(shadow.filter_radius_milli) / 1000.0f0,
        0.0f0,
        0.0f0,
    )
end

"""Reject any policy axis this renderer cannot actually honour.

The sprint rule is that a style signal must never be *silently* accepted and
then ignored: a packet asking for anisotropic filtering that renders exactly as
it would with the flag off is worse than no flag at all, because the receipt
will report a policy that had no effect. Every refusal below names the axis, the
value asked for, and the concrete reason — so the gap is recorded where it can
be found instead of being discovered later from a screenshot.

Each entry is a real dependency or structural blocker, not an unimplemented
nice-to-have:

  * `sampler.anisotropy` — the pinned Lava revision creates its VkDevice
    WITHOUT the `samplerAnisotropy` feature enabled. Setting `anisotropyEnable`
    on a sampler without that feature is invalid Vulkan usage (undefined
    behaviour, not a graceful no-op), so this cannot be turned on from the
    adapter alone; it needs an upstream device-feature change.

  * `terrain_surface.macro_variation_bp` — has no shader implementation. The
    tiling half of the same policy (`uv_repeat_scale_milli` + `wrap_repeat`) IS
    executed, after the sampler-per-surface split.

Called once per frame, before any GPU work, so a refusal costs nothing and
cannot leave a half-rendered frame.
"""
# NOTE: the definition of `_assert_render_policy_supported` deliberately sits
# BELOW `GpuCapabilityProfile` and `_anisotropy_refusal_detail`, because its
# signature names both types and Julia resolves a signature's types at
# definition time. Moving the docstring here and the function body above the
# type is what produced an `UndefVarError` once already.

"""What the GPU can actually do, queried rather than assumed.

Every field is a REAL QUERY against the selected physical device. Nothing here
is hardcoded and nothing here is a literal — this exists because the sprint
found two capabilities being asserted rather than measured:

  * `sampler_anisotropy` — the audit treated anisotropy as "just turn it on".
    The device almost certainly supports it, but the pinned Lava revision
    creates its VkDevice WITHOUT enabling the feature, so setting
    `anisotropyEnable` would be invalid Vulkan usage. Querying separates "the
    hardware cannot" (nothing to do) from "the device creation must change"
    (a concrete upstream edit) — which is the difference between a blocker and
    a ticket.

  * `timestamp_compute_and_graphics` — the previous capability check tested only
    `timestamp_period` and `timestamp_valid_bits`, and so reported GPU timing
    as available on a device where it could never work. This is the limit that
    actually gates timestamp queries and it was being ignored.

Declared here rather than beside the probe because the render-time policy
refusal below names this type in its signature, and Julia resolves a
signature's types when the method is defined.

The struct is immutable and computed once per context, at backend construction.
"""
struct GpuCapabilityProfile
    device_name::String
    # Supported by the HARDWARE (limit), independent of what was enabled.
    max_sampler_anisotropy::Float32
    # Supported by the HARDWARE (feature bit).
    sampler_anisotropy_supported::Bool
    # The limit that actually gates timestamp queries.
    timestamp_compute_and_graphics::Bool
    timestamp_period::Float32
    timestamp_valid_bits::UInt32
    # Set when a query failed. A capability we could not determine must be
    # reported as unknown, never defaulted to true.
    probe_complete::Bool
end

const UNKNOWN_CAPABILITY_PROFILE = GpuCapabilityProfile(
    "unknown", 1.0f0, false, false, 0.0f0, UInt32(0), false)

"""Explain an anisotropy refusal from MEASURED device capability, not folklore.

The three cases are genuinely different problems with genuinely different fixes,
and collapsing them into one message is what made the original refusal read as
an excuse rather than a diagnosis:

  * the hardware cannot        → there is nothing to do; request 1
  * the probe failed           → we do not know, so we refuse; fix the probe
  * the hardware can, but the
    pinned device creation
    never enabled the feature → a concrete, bounded upstream change
"""
function _anisotropy_refusal_detail(requested::Int32, capabilities::GpuCapabilityProfile)
    requested_text = "render_policy.sampler.anisotropy=$requested is refused. "
    if !capabilities.probe_complete
        return requested_text *
            "The device capability probe did not complete, so WGE cannot tell " *
            "whether this GPU supports anisotropy. Request anisotropy 1, or fix " *
            "the probe before requesting more."
    end
    if !capabilities.sampler_anisotropy_supported || capabilities.max_sampler_anisotropy <= 1.0f0
        return requested_text *
            "This device ($(capabilities.device_name)) reports " *
            "sampler_anisotropy unsupported and maxSamplerAnisotropy=" *
            "$(capabilities.max_sampler_anisotropy), so this is a hardware limit, " *
            "not a configuration problem. Request anisotropy 1."
    end
    return requested_text *
        "This device ($(capabilities.device_name)) SUPPORTS it: " *
        "maxSamplerAnisotropy=$(capabilities.max_sampler_anisotropy). It is refused " *
        "because the pinned Lava revision creates its VkDevice without enabling " *
        "the samplerAnisotropy feature, and setting anisotropyEnable on a sampler " *
        "without that feature is invalid Vulkan usage rather than a silent no-op. " *
        "The fix is a bounded upstream change: enable the feature during device " *
        "creation, clamped to the value above."
end

function _assert_render_policy_supported(
    policy::WGEGraphics.RenderPolicy,
    capabilities::GpuCapabilityProfile;
    layered_terrain::Bool=false,
)
    if policy.sampler.anisotropy > 1
        throw(AdapterError(
            "unsupported_render_policy",
            _anisotropy_refusal_detail(policy.sampler.anisotropy, capabilities),
        ))
    end
    terrain = policy.terrain_surface
    # Tiling and the repeat wrap are executed (sprint F-4/F-5). Macro variation
    # is executed by the layered terrain shader (N-4) and ONLY there: on a
    # single-material terrain there is no macro texture, so a non-zero amplitude
    # would render identically to zero while a receipt implied otherwise.
    if layered_terrain && !terrain.wrap_repeat
        throw(AdapterError(
            "unsupported_render_policy",
            "layered terrain samples metric UVs and needs render_policy.terrain_surface.wrap_repeat",
        ))
    end
    if terrain.macro_variation_bp != 0 && !layered_terrain
        throw(AdapterError(
            "unsupported_render_policy",
            "render_policy.terrain_surface.macro_variation_bp=" *
            "$(terrain.macro_variation_bp) is refused: terrain macro variation has no " *
            "shader implementation yet, and honouring the value silently is worse " *
            "than refusing it. Request 0, or land the macro-variation pass first.",
        ))
    end
    return nothing
end

# Resolve-pass policy uniforms. The packed vectors carry the render policy in
# the exact fields the shader needs; `_resolve_policy` is the ONE lowering point
# from `WGEGraphics.RenderPolicy` to GPU state, so "what does absent mean" is
# decided in WGEGraphics (auditable, cross-language tested) and nowhere else.
struct ResolvePolicy
    grade_lift_rgb::Vec4f        # lift r,g,b, unused
    grade_gain_rgb::Vec4f        # gain r,g,b, unused
    grade_gamma_saturation::Vec4f  # gamma, saturation, unused, unused
    bloom::Vec4f                 # threshold, intensity, unused, unused
    vignette::Vec4f              # strength, radius, softness, unused
    dither::Vec4f                # amplitude_milli_lsb, unused, unused, unused
end

@inline _bp(value::Int32)::Float32 = Float32(value) / Float32(WGEGraphics.POLICY_SCALE)

"""Lower a validated RenderPolicy into resolve-pass uniforms.

The defaults here are the FROZEN BASELINE: bloom intensity 0, vignette
strength 0, gamma/saturation/gain identity, dither 0. A packet with no
`render_policy` section therefore produces byte-identical pixels, which is
asserted end to end by the frozen-baseline comparison.
"""
function _resolve_policy(policy::WGEGraphics.RenderPolicy)::ResolvePolicy
    grade = policy.grade
    bloom = policy.bloom
    vignette = policy.vignette
    dither = policy.dither
    return ResolvePolicy(
        Vec4f(_bp(grade.lift_rgb_bp[1]), _bp(grade.lift_rgb_bp[2]), _bp(grade.lift_rgb_bp[3]), 0.0f0),
        Vec4f(_bp(grade.gain_rgb_bp[1]), _bp(grade.gain_rgb_bp[2]), _bp(grade.gain_rgb_bp[3]), 0.0f0),
        Vec4f(_bp(grade.gamma_bp), _bp(grade.saturation_bp), 0.0f0, 0.0f0),
        Vec4f(_bp(bloom.threshold_bp), _bp(bloom.intensity_bp), 0.0f0, 0.0f0),
        Vec4f(_bp(vignette.strength_bp), _bp(vignette.radius_bp), _bp(vignette.softness_bp), 0.0f0),
        Vec4f(Float32(dither.amplitude_milli_lsb) / 1000.0f0, 0.0f0, 0.0f0, 0.0f0),
    )
end

"""Resolve vertex stage.

Takes FLAT arguments rather than the `ResolvePolicy` struct: Lava's `draw!`
passes each `args` entry to the shader as a separate parameter, so a struct
parameter has no way to be bound. The struct is the lowering-side grouping;
the wire form is these eight scalars.
"""
function _resolve_vertex(
    texel_size::Vec2f,
    exposure::Float32,
    grade_lift_rgb::Vec4f,
    grade_gain_rgb::Vec4f,
    grade_gamma_saturation::Vec4f,
    bloom::Vec4f,
    vignette::Vec4f,
    dither::Vec4f,
)
    vertex_id = Lava.vertex_index() - Int32(1)
    x = Float32(Int32(vertex_id & Int32(1)) * 4 - 1)
    y = Float32(Int32((vertex_id >> Int32(1)) & Int32(1)) * 4 - 1)
    Lava.set_position!(Vec4f(x, y, 0.5f0, 1.0f0))
    Lava.gfx_output(0, Vec2f((x + 1.0f0) * 0.5f0, (y + 1.0f0) * 0.5f0))
    Lava.gfx_output(1, texel_size)
    Lava.gfx_output(2, Vec4f(exposure, 0.0f0, 0.0f0, 0.0f0))
    Lava.gfx_output(3, grade_lift_rgb)
    Lava.gfx_output(4, grade_gain_rgb)
    Lava.gfx_output(5, grade_gamma_saturation)
    Lava.gfx_output(6, bloom)
    Lava.gfx_output(7, vignette)
    Lava.gfx_output(8, dither)
    return nothing
end

function _resolve_sample(u::Float32, v::Float32)::Vec4f
    return Vec4f(
        Lava.sample_texture_2d(UInt32(0), u, v, UInt32(0)),
        Lava.sample_texture_2d(UInt32(0), u, v, UInt32(1)),
        Lava.sample_texture_2d(UInt32(0), u, v, UInt32(2)),
        Lava.sample_texture_2d(UInt32(0), u, v, UInt32(3)),
    )
end

"""Display-referred grade: lift, gamma, gain, then saturation.

Saturation runs LAST and in display space because that is where a viewer
judges it; applying it in linear HDR would make the same number mean different
things at different exposures, which is precisely the kind of un-auditable
knob this policy channel exists to remove.
"""
@inline function _apply_grade(color::Vec4f, lift::Vec4f, gain::Vec4f, gamma_saturation::Vec4f)::Vec4f
    # Identity must be BIT-EXACT, not merely close. `x ^ 1.0` goes through
    # `powf`, which is not guaranteed to return `x` unchanged: applying the
    # default grade moved 6 bytes of the frozen baseline by 1 LSB each. The
    # whole point of "absent policy == baseline bytes" is that it is provable,
    # so the neutral case short-circuits instead of being multiplied through.
    if lift[1] == 0.0f0 && lift[2] == 0.0f0 && lift[3] == 0.0f0 &&
       gamma_saturation[1] == 1.0f0 && gamma_saturation[2] == 1.0f0 &&
       gain[1] == 1.0f0 && gain[2] == 1.0f0 && gain[3] == 1.0f0
        return color
    end
    red = lift[1] + color[1]
    green = lift[2] + color[2]
    blue = lift[3] + color[3]
    gamma = gamma_saturation[1]
    if gamma == 1.0f0
        red = max(red, 0.0f0) * gain[1]
        green = max(green, 0.0f0) * gain[2]
        blue = max(blue, 0.0f0) * gain[3]
    else
        red = max(red, 0.0f0)^gamma * gain[1]
        green = max(green, 0.0f0)^gamma * gain[2]
        blue = max(blue, 0.0f0)^gamma * gain[3]
    end
    red = clamp(red, 0.0f0, 1.0f0)
    green = clamp(green, 0.0f0, 1.0f0)
    blue = clamp(blue, 0.0f0, 1.0f0)
    saturation = gamma_saturation[2]
    if saturation != 1.0f0
        # Rec.709 luma on already-encoded values; this is a display-space
        # stylisation knob, not a colourimetric transform.
        luma = 0.2126f0 * red + 0.7152f0 * green + 0.0722f0 * blue
        red = clamp(luma + (red - luma) * saturation, 0.0f0, 1.0f0)
        green = clamp(luma + (green - luma) * saturation, 0.0f0, 1.0f0)
        blue = clamp(luma + (blue - luma) * saturation, 0.0f0, 1.0f0)
    end
    return Vec4f(red, green, blue, color[4])
end

"""Restrained bloom from the already-downsampled resolve input.

A single wide tap ring at the resolve scale. This is deliberately NOT a
progressive downsample/upsample bloom: that needs extra framebuffers and a
multi-pass graph, and at the baseline's 2x supersample the single ring buys
most of the perceived halo for a fraction of the machinery. Intensity 0 (the
default) short-circuits to the input unchanged.
"""
@inline _bloom_engaged(bloom::Vec4f)::Bool = bloom[2] > 0.0f0

"""Soft-knee highlight extraction: how much of `luma` exceeds `threshold`.

Pure arithmetic, deliberately factored out of `_apply_bloom` so it is
host-testable — the bloom body itself calls a GPU intrinsic and cannot run on
the CPU.
"""
@inline _bloom_knee(luma::Float32, threshold::Float32)::Float32 =
    max(luma - threshold, 0.0f0)

"""Restrained bloom from the already-downsampled resolve input.

A single wide tap ring at the resolve scale, UNROLLED. This is deliberately
NOT a progressive downsample/upsample bloom: that needs extra framebuffers and
a multi-pass graph, and at the baseline's 2x supersample one ring buys most of
the perceived halo for a fraction of the machinery.

The taps are written out rather than looped because Lava's shader JIT rejects
a `for` over a constant tuple — it constant-folds the bound into an
`LLVM.ConstantExpr` the backend cannot lower, and the whole pipeline fails to
compile. Every other shader in this adapter is loop-free for the same reason.
"""
@inline function _bloom_tap_luma(texel_size::Vec2f, uv::Vec2f, dx::Float32, dy::Float32)::Float32
    tap = _resolve_sample(
        uv[1] + dx * 4.0f0 * texel_size[1],
        uv[2] + dy * 4.0f0 * texel_size[2],
    )
    return 0.2126f0 * tap[1] + 0.7152f0 * tap[2] + 0.0722f0 * tap[3]
end

@inline function _apply_bloom(color::Vec4f, bloom::Vec4f, texel_size::Vec2f, uv::Vec2f)::Vec4f
    _bloom_engaged(bloom) || return color
    threshold = bloom[1]
    intensity = bloom[2]
    accumulated = (
        _bloom_knee(_bloom_tap_luma(texel_size, uv, 1.0f0, 0.0f0), threshold) +
        _bloom_knee(_bloom_tap_luma(texel_size, uv, -1.0f0, 0.0f0), threshold) +
        _bloom_knee(_bloom_tap_luma(texel_size, uv, 0.0f0, 1.0f0), threshold) +
        _bloom_knee(_bloom_tap_luma(texel_size, uv, 0.0f0, -1.0f0), threshold)
    ) * 0.25f0
    scale = accumulated * intensity
    return Vec4f(
        min(color[1] + color[1] * scale, 1.0f0),
        min(color[2] + color[2] * scale, 1.0f0),
        min(color[3] + color[3] * scale, 1.0f0),
        color[4],
    )
end

"""Radial vignette in normalized frame space.

`radius` and `softness` are fractions of the half-diagonal so the falloff is
resolution independent: the same policy produces the same composition at
768x512 and at 1920x1080.
"""
@inline function _apply_vignette(color::Vec4f, vignette::Vec4f, uv::Vec2f)::Vec4f
    strength = vignette[1]
    strength <= 0.0f0 && return color
    radius = vignette[2]
    softness = max(vignette[3], 1.0f-4)
    centered_x = uv[1] * 2.0f0 - 1.0f0
    centered_y = uv[2] * 2.0f0 - 1.0f0
    distance = sqrt(centered_x * centered_x + centered_y * centered_y) / 1.41421356f0
    falloff = clamp((distance - radius) / softness, 0.0f0, 1.0f0)
    # smoothstep so the transition has no visible edge of its own.
    shaped = falloff * falloff * (3.0f0 - 2.0f0 * falloff)
    attenuation = 1.0f0 - shaped * strength
    return Vec4f(color[1] * attenuation, color[2] * attenuation, color[3] * attenuation, color[4])
end

"""Bayer 8x8 ordered dither, applied in DISPLAY space immediately before
quantisation.

Two properties matter here and both are load-bearing:

1. ORDERED, not hashed or temporal. The certified frame must be a pure
   function of the packet; a noise sequence would make it a function of a
   counter, which is exactly the determinism this engine is built on.
2. APPLIED IN DISPLAY SPACE. The banding this attacks is created by quantising
   an sRGB-encoded gradient to 8 bits, so dithering the linear value would
   attack a different (invisible) artefact.

Amplitude is in LSB, so 1.0 is one full quantisation step peak-to-peak. The
value is added AFTER the sRGB encode, in `_rgba8`/capture, which is the last
point before the UInt8 conversion.
"""
# The standard recursive Bayer 8x8 ordered-dither threshold matrix.
#
# Written out rather than computed because (a) it is the single artefact a
# reviewer needs to check by eye, and (b) an earlier bit-twiddling construction
# of this matrix was WRONG in a way that only surfaced as "dither did not
# reduce banding": every value in a row came out negative, so the dither could
# only push a pixel down and never across a quantisation boundary. A literal
# table cannot drift that way.
const BAYER_8X8 = (
    (0, 32, 8, 40, 2, 34, 10, 42),
    (48, 16, 56, 24, 50, 18, 58, 26),
    (12, 44, 4, 36, 14, 46, 6, 38),
    (60, 28, 52, 20, 62, 30, 54, 22),
    (3, 35, 11, 43, 1, 33, 9, 41),
    (51, 19, 59, 27, 49, 17, 57, 25),
    (15, 47, 7, 39, 13, 45, 5, 37),
    (63, 31, 55, 23, 61, 29, 53, 21),
)

@inline function _bayer8x8(x::Int32, y::Int32)::Float32
    row = BAYER_8X8[(y & 0x07) + 1]
    value = Float32(row[(x & 0x07) + 1])
    return (value + 0.5f0) / 64.0f0 - 0.5f0
end

@inline function _apply_dither_lsb(value::Float32, x::Int32, y::Int32, amplitude::Float32)::Float32
    amplitude <= 0.0f0 && return value
    return clamp(value + _bayer8x8(x, y) * amplitude, 0.0f0, 1.0f0)
end

function _resolve_fragment()
    uv = Lava.gfx_input(Vec2f, 0)
    texel_size = Lava.gfx_input(Vec2f, 1)
    exposure = Lava.gfx_input(Vec4f, 2)[1]
    lift = Lava.gfx_input(Vec4f, 3)
    gain = Lava.gfx_input(Vec4f, 4)
    gamma_saturation = Lava.gfx_input(Vec4f, 5)
    bloom = Lava.gfx_input(Vec4f, 6)
    vignette = Lava.gfx_input(Vec4f, 7)
    offset = 0.5f0 * texel_size
    first = _resolve_sample(uv[1] - offset[1], uv[2] - offset[2])
    second = _resolve_sample(uv[1] + offset[1], uv[2] - offset[2])
    third = _resolve_sample(uv[1] - offset[1], uv[2] + offset[2])
    fourth = _resolve_sample(uv[1] + offset[1], uv[2] + offset[2])
    hdr = Vec4f(
        0.25f0 * (first[1] + second[1] + third[1] + fourth[1]),
        0.25f0 * (first[2] + second[2] + third[2] + fourth[2]),
        0.25f0 * (first[3] + second[3] + third[3] + fourth[3]),
        0.25f0 * (first[4] + second[4] + third[4] + fourth[4]),
    )
    mapped = _tone_map(hdr, exposure)
    bloomed = _apply_bloom(mapped, bloom, texel_size, uv)
    graded = _apply_grade(bloomed, lift, gain, gamma_saturation)
    Lava.gfx_output(0, _apply_vignette(graded, vignette, uv))
    return nothing
end

function _terrain_fragment()
    lit = _surface_fragment_color(Lava.gfx_input(Vec4f, 1), Lava.gfx_input(Vec4f, 16))
    Lava.gfx_output(0, lit)
    return nothing
end

"""Alpha-mask and double-sided meshes (CONVERGE-2 N-6).

`surface_parameters[3]` is the alpha cutoff (0 for a double-sided opaque
material, which never discards) and `[4]` is 1 for double-sided. Back faces of a
double-sided material are shaded with the normal and the whole tangent frame
negated (glTF: "back-facing normals are reversed"). The discard comes after every
texture sample, so implicit-LOD derivatives are taken while the whole quad is
still running.
"""
function _mesh_cutout_fragment()
    surface_parameters = Lava.gfx_input(Vec4f, 15)
    normal = Lava.gfx_input(Vec4f, 1)
    tangent = Lava.gfx_input(Vec4f, 16)
    back = surface_parameters[4] > 0.5f0 && !Lava.front_facing()
    facing_normal = back ? Vec4f(-normal[1], -normal[2], -normal[3], -normal[4]) : normal
    facing_tangent = back ? Vec4f(-tangent[1], -tangent[2], -tangent[3], -tangent[4]) : tangent
    lit = _surface_fragment_color(facing_normal, facing_tangent)
    coverage = _textured_color(
        Lava.gfx_input(Vec4f, 0),
        Lava.gfx_input(Vec2f, 3),
        Lava.gfx_input(Vec4f, 7)[4],
    )[4]
    if coverage < surface_parameters[3]
        Lava.discard()
    end
    Lava.gfx_output(0, lit)
    return nothing
end

@inline function _surface_fragment_color(normal::Vec4f, tangent::Vec4f)::Vec4f
    base_color = Lava.gfx_input(Vec4f, 0)
    world_position = Lava.gfx_input(Vec4f, 2)
    uv = Lava.gfx_input(Vec2f, 3)
    material = Lava.gfx_input(Vec4f, 4)
    light_direction = Lava.gfx_input(Vec4f, 5)
    light_color = Lava.gfx_input(Vec4f, 6)
    lighting_parameters = Lava.gfx_input(Vec4f, 7)
    environment_top = Lava.gfx_input(Vec4f, 8)
    environment_horizon = Lava.gfx_input(Vec4f, 9)
    environment_ground = Lava.gfx_input(Vec4f, 10)
    fog_color = Lava.gfx_input(Vec4f, 11)
    camera_position = Lava.gfx_input(Vec4f, 12)
    light_space = Lava.gfx_input(Vec4f, 13)
    material_emissive = Lava.gfx_input(Vec4f, 14)
    surface_parameters = Lava.gfx_input(Vec4f, 15)
    shadow = Lava.gfx_input(Vec4f, 17)
    sky_parameters = Lava.gfx_input(Vec4f, 18)
    atmosphere_parameters = Lava.gfx_input(Vec4f, 19)
    surface_options = Lava.gfx_input(Vec4f, 20)
    light_intensity = lighting_parameters[1]
    fog_density = lighting_parameters[2]
    exposure = lighting_parameters[3]
    texture_enabled = lighting_parameters[4]
    view_direction = Vec4f(
        camera_position[1] - world_position[1],
        camera_position[2] - world_position[2],
        camera_position[3] - world_position[3],
        0.0f0,
    )
    # Roughness is read from GREEN, glTF's channel. `asset_projection` binds an
    # imported `metallicRoughnessTexture` (roughness = G, metallic = B, R unused
    # or AO in ORM packing) to this slot, and reading RED gave every imported
    # PBR asset the wrong roughness. Every procedural roughness map and the
    # 1x1 fallback are scalar (R == G == B, verified for all campaign2 maps),
    # so this is byte-identical for existing content. The metallic channel (B)
    # is still not read: metallic is factor-only until the contract carries
    # channel semantics (CONVERGE-0 ledger, turn 11).
    roughness_sample = _sample_texture(UInt32(2), uv)[2]
    occlusion_sample = _sample_texture(UInt32(3), uv)[1]
    emissive_sample = _sample_texture(UInt32(4), uv)
    lit_color = _material_response(
        _albedo_override(_textured_color(base_color, uv, texture_enabled), surface_options),
        _perturbed_normal(normal, tangent, uv, material[3]),
        light_direction,
        light_color,
        light_intensity,
        environment_top,
        environment_horizon,
        environment_ground,
        material[1],
        material[2],
        surface_parameters[1],
        surface_parameters[2],
        _shadow_visibility(light_space, shadow[1], shadow[2]),
        view_direction,
        roughness_sample,
        occlusion_sample,
        material[4],
        material_emissive,
        emissive_sample,
        sky_parameters,
        surface_options[3],
        TextureIbl(),
    )
    fogged_color = _apply_aerial_perspective(
        lit_color,
        world_position,
        camera_position,
        _distance_between(world_position, camera_position),
        fog_color,
        fog_density,
        light_direction,
        light_color,
        light_intensity,
        environment_top,
        environment_horizon,
        sky_parameters,
        atmosphere_parameters,
    )
    return fogged_color
end

# Layered-terrain far-field resampling (see `_terrain_layered_fragment`). A
# renderer property of the layered shader, declared here rather than buried in
# it: 7.13 is deliberately not a small rational so the two scales never align.
const TERRAIN_FAR_REPEAT = 7.13f0
const TERRAIN_FAR_BLEND_START_M = 25.0f0
const TERRAIN_FAR_BLEND_END_M = 90.0f0

@inline function _ramp(low::Float32, high::Float32, value::Float32)::Float32
    t = clamp((value - low) / (high - low), 0.0f0, 1.0f0)
    return t * t * (3.0f0 - 2.0f0 * t)
end

"""Coverage weight of one painted layer: the product of its slope, height, and
macro-noise ramps (`_terrain_layer_uniforms` encodes an absent term as a ramp
that is always 1, and a padding layer as a slope ramp that is always 0)."""
@inline function _layer_weight(a::Vec4f, b::Vec4f, slope::Float32, height::Float32, macro_value::Float32)::Float32
    return _ramp(a[2], a[3], slope) * _ramp(b[1], b[2], height) * _ramp(a[4] - b[3], a[4] + b[3], macro_value)
end

@inline function _tangent_space_normal(sample::Vec4f, scale::Float32)::Vec4f
    return Vec4f(
        (sample[1] * 2.0f0 - 1.0f0) * scale,
        (sample[2] * 2.0f0 - 1.0f0) * scale,
        max(sample[3] * 2.0f0 - 1.0f0, 0.05f0),
        0.0f0,
    )
end

@inline _mix(a::Float32, b::Float32, t::Float32)::Float32 = a + (b - a) * t

@inline function _mix4(a::Vec4f, b::Vec4f, t::Float32)::Vec4f
    return Vec4f(_mix(a[1], b[1], t), _mix(a[2], b[2], t), _mix(a[3], b[3], t), _mix(a[4], b[4], t))
end

"""Rotate a tangent-space normal into world space (the frame `_perturbed_normal`
uses, kept separate so the single-material path stays byte-identical)."""
@inline function _apply_tangent_normal(normal::Vec4f, tangent::Vec4f, tangent_space::Vec4f)::Vec4f
    surface_normal = _normalize_vector(normal)
    tangent_orthogonal = Vec4f(
        tangent[1] - surface_normal[1] * _dot_vector(surface_normal, tangent),
        tangent[2] - surface_normal[2] * _dot_vector(surface_normal, tangent),
        tangent[3] - surface_normal[3] * _dot_vector(surface_normal, tangent),
        0.0f0,
    )
    tangent_vector = _normalize_vector(tangent_orthogonal)
    bitangent = _normalize_vector(_cross_vector(surface_normal, tangent_vector))
    handedness = tangent[4] < 0.0f0 ? -1.0f0 : 1.0f0
    return _normalize_vector(
        Vec4f(
            tangent_vector[1] * tangent_space[1] + handedness * bitangent[1] * tangent_space[2] + surface_normal[1] * tangent_space[3],
            tangent_vector[2] * tangent_space[1] + handedness * bitangent[2] * tangent_space[2] + surface_normal[2] * tangent_space[3],
            tangent_vector[3] * tangent_space[1] + handedness * bitangent[3] * tangent_space[2] + surface_normal[3] * tangent_space[3],
            0.0f0,
        ),
    )
end

"""Layered terrain fragment (N-4).

Every layer is sampled in WORLD metres — u = x / repeat, v = -z / repeat, the
orientation the terrain tangent frame already uses — so tiling is physical and
independent of the terrain extent. Layer 0 is the base; layers 1 and 2 are
painted over it with `_layer_weight`. Albedo, roughness (G, glTF), occlusion
(R), and tangent-space normals are blended with the same weights, then macro
noise scales albedo by (1 + amplitude x (2m - 1)). The terrain material's base
colour is deliberately NOT applied: layer materials are neutral and the scans
carry the colour.
"""
function _terrain_layered_fragment()
    normal = Lava.gfx_input(Vec4f, 1)
    world_position = Lava.gfx_input(Vec4f, 2)
    material = Lava.gfx_input(Vec4f, 4)
    light_direction = Lava.gfx_input(Vec4f, 5)
    light_color = Lava.gfx_input(Vec4f, 6)
    lighting_parameters = Lava.gfx_input(Vec4f, 7)
    environment_top = Lava.gfx_input(Vec4f, 8)
    environment_horizon = Lava.gfx_input(Vec4f, 9)
    environment_ground = Lava.gfx_input(Vec4f, 10)
    fog_color = Lava.gfx_input(Vec4f, 11)
    camera_position = Lava.gfx_input(Vec4f, 12)
    light_space = Lava.gfx_input(Vec4f, 13)
    surface_parameters = Lava.gfx_input(Vec4f, 15)
    tangent = Lava.gfx_input(Vec4f, 16)
    shadow = Lava.gfx_input(Vec4f, 17)
    sky_parameters = Lava.gfx_input(Vec4f, 18)
    atmosphere_parameters = Lava.gfx_input(Vec4f, 19)
    surface_options = Lava.gfx_input(Vec4f, 20)
    layer0_a = Lava.gfx_input(Vec4f, 21)
    layer0_b = Lava.gfx_input(Vec4f, 22)
    layer1_a = Lava.gfx_input(Vec4f, 23)
    layer1_b = Lava.gfx_input(Vec4f, 24)
    layer2_a = Lava.gfx_input(Vec4f, 25)
    layer2_b = Lava.gfx_input(Vec4f, 26)
    macro_parameters = Lava.gfx_input(Vec4f, 27)
    u = world_position[1]
    v = -world_position[3]
    slope = 1.0f0 - clamp(_normalize_vector(normal)[2], 0.0f0, 1.0f0)
    height = world_position[2]
    macro_value = Lava.sample_texture_2d(UInt32(13), u * macro_parameters[1], v * macro_parameters[1], UInt32(0))
    uv0 = Vec2f(u * layer0_a[1], v * layer0_a[1])
    uv1 = Vec2f(u * layer1_a[1], v * layer1_a[1])
    uv2 = Vec2f(u * layer2_a[1], v * layer2_a[1])
    # Far-field resampling: past ~25 m a 2-3 m tile spans so few pixels that its
    # own low-frequency structure repeats as a visible grid (N4-1 ledger), and
    # mips cannot remove repetition larger than a pixel. Albedo is blended toward
    # a second sample at TERRAIN_FAR_REPEAT x the tile size, so the period the
    # eye can see grows by that factor where repetition would show.
    distance = _distance_between(world_position, camera_position)
    far = _ramp(TERRAIN_FAR_BLEND_START_M, TERRAIN_FAR_BLEND_END_M, distance)
    far_scale = 1.0f0 / TERRAIN_FAR_REPEAT
    albedo0 = _mix4(
        _sample_texture(UInt32(0), uv0),
        _sample_texture(UInt32(0), Vec2f(uv0[1] * far_scale, uv0[2] * far_scale)),
        far,
    )
    normal0 = _sample_texture(UInt32(1), uv0)
    rough0 = _sample_texture(UInt32(2), uv0)[2]
    occlusion0 = _sample_texture(UInt32(3), uv0)[1]
    albedo1 = _mix4(
        _sample_texture(UInt32(4), uv1),
        _sample_texture(UInt32(4), Vec2f(uv1[1] * far_scale, uv1[2] * far_scale)),
        far,
    )
    normal1 = _sample_texture(UInt32(6), uv1)
    rough1 = _sample_texture(UInt32(7), uv1)[2]
    occlusion1 = _sample_texture(UInt32(8), uv1)[1]
    albedo2 = _mix4(
        _sample_texture(UInt32(9), uv2),
        _sample_texture(UInt32(9), Vec2f(uv2[1] * far_scale, uv2[2] * far_scale)),
        far,
    )
    normal2 = _sample_texture(UInt32(10), uv2)
    rough2 = _sample_texture(UInt32(11), uv2)[2]
    occlusion2 = _sample_texture(UInt32(12), uv2)[1]
    weight1 = _layer_weight(layer1_a, layer1_b, slope, height, macro_value)
    weight2 = _layer_weight(layer2_a, layer2_b, slope, height, macro_value)
    albedo = _mix4(_mix4(albedo0, albedo1, weight1), albedo2, weight2)
    tangent_space = _mix4(
        _mix4(_tangent_space_normal(normal0, layer0_b[4]), _tangent_space_normal(normal1, layer1_b[4]), weight1),
        _tangent_space_normal(normal2, layer2_b[4]),
        weight2,
    )
    roughness_sample = _mix(_mix(rough0, rough1, weight1), rough2, weight2)
    occlusion_sample = _mix(_mix(occlusion0, occlusion1, weight1), occlusion2, weight2)
    macro_scale = 1.0f0 + macro_parameters[2] * (macro_value * 2.0f0 - 1.0f0)
    surface_color = _albedo_override(
        Vec4f(albedo[1] * macro_scale, albedo[2] * macro_scale, albedo[3] * macro_scale, 1.0f0),
        surface_options,
    )
    view_direction = Vec4f(
        camera_position[1] - world_position[1],
        camera_position[2] - world_position[2],
        camera_position[3] - world_position[3],
        0.0f0,
    )
    lit_color = _material_response(
        surface_color,
        _apply_tangent_normal(normal, tangent, tangent_space),
        light_direction,
        light_color,
        lighting_parameters[1],
        environment_top,
        environment_horizon,
        environment_ground,
        0.0f0,
        1.0f0,
        surface_parameters[1],
        surface_parameters[2],
        _shadow_visibility(light_space, shadow[1], shadow[2]),
        view_direction,
        roughness_sample,
        occlusion_sample,
        material[4],
        Vec4f(0.0f0, 0.0f0, 0.0f0, 0.0f0),
        Vec4f(0.0f0, 0.0f0, 0.0f0, 0.0f0),
        sky_parameters,
        surface_options[3],
        TextureIbl(),
    )
    Lava.gfx_output(0, _apply_aerial_perspective(
        lit_color,
        world_position,
        camera_position,
        distance,
        fog_color,
        lighting_parameters[2],
        light_direction,
        light_color,
        lighting_parameters[1],
        environment_top,
        environment_horizon,
        sky_parameters,
        atmosphere_parameters,
    ))
    return nothing
end

function _overlay_vertex(
    positions::Lava.LavaDeviceArray{Vec4f,1},
    colors::Lava.LavaDeviceArray{Vec4f,1},
)
    vertex_id = Lava.vertex_index()
    Lava.set_position!(positions[vertex_id])
    Lava.gfx_output(0, colors[vertex_id])
    return nothing
end

function _overlay_fragment()
    Lava.gfx_output(0, Lava.gfx_input(Vec4f, 0))
    return nothing
end

function backend()::LavaBackend
    lock(BACKEND_LOCK) do
        current = BACKEND_REF[]
        current isa LavaBackend && return current

        context = Lava.vk_context()
        probe_pipeline = GraphicsPipeline(
            ;
            vertex=_probe_vertex,
            fragment=_probe_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthOff(),
        )
        sky_pipeline = GraphicsPipeline(
            ;
            vertex=_sky_vertex,
            fragment=_sky_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthOff(),
        )
        sky_view_pipeline = GraphicsPipeline(
            ;
            vertex=_sky_view_vertex,
            fragment=_sky_view_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthOff(),
        )
        terrain_pipeline = GraphicsPipeline(
            ;
            vertex=_terrain_vertex,
            fragment=_terrain_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthLess(),
        )
        terrain_layered_pipeline = GraphicsPipeline(
            ;
            vertex=_terrain_layered_vertex,
            fragment=_terrain_layered_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthLess(),
        )
        overlay_pipeline = GraphicsPipeline(
            ;
            vertex=_overlay_vertex,
            fragment=_overlay_fragment,
            topology=TriangleList(),
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthOff(),
        )
        mesh_pipeline = GraphicsPipeline(
            ;
            vertex=_mesh_vertex,
            fragment=_terrain_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthLess(),
        )
        mesh_cutout_pipeline = GraphicsPipeline(
            ;
            vertex=_mesh_vertex,
            fragment=_mesh_cutout_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthLess(),
        )
        texture_pipeline = GraphicsPipeline(
            ;
            vertex=_texture_probe_vertex,
            fragment=_texture_probe_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthOff(),
        )
        depth_pipeline = GraphicsPipeline(
            ;
            vertex=_depth_probe_vertex,
            fragment=_depth_probe_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthLess(),
        )
        terrain_shadow_pipeline = GraphicsPipeline(
            ;
            vertex=_terrain_shadow_vertex,
            fragment=_shadow_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthLess(),
        )
        mesh_shadow_pipeline = GraphicsPipeline(
            ;
            vertex=_mesh_shadow_vertex,
            fragment=_shadow_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthLess(),
        )
        mesh_cutout_shadow_pipeline = GraphicsPipeline(
            ;
            vertex=_mesh_cutout_shadow_vertex,
            fragment=_shadow_cutout_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthLess(),
        )
        resolve_pipeline = GraphicsPipeline(
            ;
            vertex=_resolve_vertex,
            fragment=_resolve_fragment,
            blend=Opaque(),
            cull=NoCull(),
            depth=DepthOff(),
        )
        created = LavaBackend(
            context,
            context.default_bq,
            probe_pipeline,
            sky_pipeline,
            sky_view_pipeline,
            terrain_pipeline,
            terrain_layered_pipeline,
            overlay_pipeline,
            mesh_pipeline,
            mesh_cutout_pipeline,
            texture_pipeline,
            depth_pipeline,
            terrain_shadow_pipeline,
            mesh_shadow_pipeline,
            mesh_cutout_shadow_pipeline,
            resolve_pipeline,
            Dict{Tuple{Int,Int,Bool,Symbol},Lava.LavaFramebuffer}(),
            nothing,
            nothing,
            nothing,
            nothing,
            nothing,
            nothing,
            nothing,
            nothing,
            Dict{Tuple{String,String,SamplerSpec},MaterialTextureResources}(),
            Dict{Tuple{String,String},MaterialTextures}(),
            Dict{Tuple{String,String},Lava.TextureBindings}(),
            Dict{Tuple{SamplerSpec,UInt32},Lava.LavaSampler}(),
            nothing,
            UInt64(0),
            UInt64(0),
            UInt64(0),
            UInt64(0),
            _gpu_timestamp_capable(context),
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
        )
        BACKEND_REF[] = created
        return created
    end
end

function _validate_dimensions(width_px::Integer, height_px::Integer)
    1 <= width_px <= 8192 || throw(AdapterError("invalid_dimensions", "width is outside the adapter bound"))
    1 <= height_px <= 8192 || throw(AdapterError("invalid_dimensions", "height is outside the adapter bound"))
    width_px * height_px * 4 <= MAX_CAPTURE_BYTES ||
        throw(AdapterError("invalid_dimensions", "capture byte length exceeds the native frame bound"))
    return (Int(width_px), Int(height_px))
end

function _probe_gpu_capabilities(context::Lava.VkContext)::GpuCapabilityProfile
    try
        properties = Vulkan.get_physical_device_properties(context.physical_device)
        features = Vulkan.get_physical_device_features(context.physical_device)
        queue_properties = Vulkan.get_physical_device_queue_family_properties(
            context.physical_device,
        )[Int(context.queue_family_index) + 1]
        return GpuCapabilityProfile(
            properties.device_name,
            Float32(properties.limits.max_sampler_anisotropy),
            Bool(features.sampler_anisotropy),
            Bool(properties.limits.timestamp_compute_and_graphics),
            Float32(properties.limits.timestamp_period),
            UInt32(queue_properties.timestamp_valid_bits),
            true,
        )
    catch
        # An incomplete probe is reported as incomplete. Defaulting an unknown
        # capability to `true` is precisely how fake capability claims start.
        return UNKNOWN_CAPABILITY_PROFILE
    end
end

function _gpu_timestamp_capable(context::Lava.VkContext)::Bool
    profile = _probe_gpu_capabilities(context)
    return profile.probe_complete &&
           profile.timestamp_compute_and_graphics &&
           profile.timestamp_period > 0.0f0 &&
           profile.timestamp_valid_bits > 0
end

function _framebuffer!(
    state::LavaBackend,
    width_px::Int,
    height_px::Int,
    depth::Bool=false,
    purpose::Symbol=:scene,
)::Lava.LavaFramebuffer
    key = (width_px, height_px, depth, purpose)
    return get!(state.framebuffers, key) do
        Lava.LavaFramebuffer(
            width_px,
            height_px;
            ctx=state.context,
            depth=depth,
            color_format=Vulkan.FORMAT_R32G32B32A32_SFLOAT,
        )
    end
end

function _linear_to_srgb(value::Float32)::Float32
    clamped = clamp(value, 0.0f0, 1.0f0)
    return clamped <= 0.0031308f0 ?
           12.92f0 * clamped :
           1.055f0 * clamped^(1.0f0 / 2.4f0) - 0.055f0
end

function _srgb_to_linear(value::Float32)::Float32
    clamped = clamp(value, 0.0f0, 1.0f0)
    return clamped <= 0.04045f0 ?
           clamped / 12.92f0 :
           ((clamped + 0.055f0) / 1.055f0)^2.4f0
end

function _rgba8(value::NTuple{4,<:Real})::NTuple{4,UInt8}
    return _rgba8(value, 0.0f0, 0, 0)
end

"""Quantise one RGBA sample, optionally dithered.

`dither_amplitude_lsb` is in quantisation steps (1.0 == 1 LSB peak-to-peak).
The dither is added to the sRGB-ENCODED value, because the banding being
attacked is created by quantising an encoded gradient to 8 bits; dithering the
linear value would break a different artefact that does not exist.

Alpha is never dithered: it is a coverage/identity channel here, and dithering
it would make `artifact_rate` (which counts non-opaque pixels) meaningless.
`x`/`y` are CAPTURE pixel coordinates, so the pattern is anchored to the image
rather than to the readback buffer's layout.
"""
function _rgba8(
    value::NTuple{4,<:Real},
    dither_amplitude_lsb::Float32,
    x::Integer,
    y::Integer,
)::NTuple{4,UInt8}
    return ntuple(
        index -> begin
            channel = Float32(value[index])
            isfinite(channel) && 0.0f0 <= channel <= 1.0f0 ||
                throw(AdapterError(
                    "invalid_capture",
                    "RGBA channel is outside [0, 1]: pixel ($x, $y) channel $index = $channel",
                ))
            encoded = index == 4 ? channel : _linear_to_srgb(channel)
            if index != 4 && dither_amplitude_lsb > 0.0f0
                # amplitude is in LSB; one LSB in encoded [0,1] space is 1/255.
                encoded = clamp(
                    encoded + _bayer8x8(Int32(x), Int32(y)) * (dither_amplitude_lsb / 255.0f0),
                    0.0f0,
                    1.0f0,
                )
            end
            # Rust's authority quantizer rounds positive half-way values away
            # from zero; Julia's default `round` is ties-to-even.
            UInt8(clamp(floor(Int, encoded * 255.0f0 + 0.5f0), 0, 255))
        end,
        4,
    )
end

function _rgba8(value::NTuple{4,UInt8})::NTuple{4,UInt8}
    return value
end

function _capture_bytes(pixels::AbstractMatrix{<:NTuple{4,<:Real}})::Vector{UInt8}
    return _capture_bytes(pixels, 0.0f0)
end

function _capture_bytes(
    pixels::AbstractMatrix{<:NTuple{4,<:Real}},
    dither_amplitude_lsb::Float32,
)::Vector{UInt8}
    bytes = Vector{UInt8}(undef, 4 * length(pixels))
    offset = 1
    # Lava readback is indexed as (x, y); the contract stores rows top-to-bottom.
    for row in axes(pixels, 2), column in axes(pixels, 1)
        rgba = _rgba8(pixels[column, row], dither_amplitude_lsb, column - 1, row - 1)
        for component in 1:4
            bytes[offset+component-1] = rgba[component]
        end
        offset += 4
    end
    return bytes
end

function _device_uuid(context)
    properties = Vulkan.get_physical_device_properties_2(
        context.physical_device,
        Vulkan.PhysicalDeviceIDProperties,
    )
    return bytes2hex(UInt8[properties.next.device_uuid...])
end

function _physical_properties(context)
    return Vulkan.get_physical_device_properties(context.physical_device)
end

function backend_probe(state::LavaBackend=backend())
    properties = _physical_properties(state.context)
    return (
        schema="wge.lava-backend-probe/v1",
        adapter_revision=ADAPTER_REVISION,
        lava_revision=LAVA_REVISION,
        vulkan_revision=VULKAN_REVISION,
        vulkan_core_revision=VULKAN_CORE_REVISION,
        julia_version=string(VERSION),
        backend_id="lava-vulkan",
        device_name=state.context.device_name,
        device_uuid=_device_uuid(state.context),
        vulkan_api_version=string(properties.api_version),
        hardware_ray_tracing=state.context.rt_pipeline_properties !== nothing,
        gpu_timestamps=state.gpu_timestamp_supported,
        persistent_context=true,
        offscreen_raster=state.probe_compiled,
        depth_attachment=state.depth_compiled,
        texture_sampling=state.texture_compiled,
        readback=state.probe_compiled,
    )
end

function backend_ready(state::LavaBackend=backend())
    properties = _physical_properties(state.context)
    return (
        schema_version="wge.graphics-ready/v1",
        backend_id="lava-vulkan",
        adapter_revision=ADAPTER_REVISION,
        lava_revision=LAVA_REVISION,
        julia_version=string(VERSION),
        vulkan_api_version=string(properties.api_version),
        device_name=state.context.device_name,
        device_uuid=_device_uuid(state.context),
        features=(
            offscreen_raster=state.probe_compiled,
            depth_attachment=state.depth_compiled,
            texture_sampling=state.texture_compiled,
            readback=state.probe_compiled,
            hardware_ray_tracing=state.context.rt_pipeline_properties !== nothing,
            gpu_timestamps=state.gpu_timestamp_supported,
        ),
    )
end

function render_probe(
    width_px::Integer=16,
    height_px::Integer=16,
    state::LavaBackend=backend(),
)
    width, height = _validate_dimensions(width_px, height_px)
    draw_calls_start = state.draw_calls
    readback_bytes_start = state.readback_bytes
    pipeline_compilations_start = state.pipeline_compilations
    framebuffer = _framebuffer!(state, width, height)
    target = OffscreenTarget(framebuffer)
    draw!(state.queue, state.probe_pipeline, target, 3; clear_color=(0.0f0, 0.0f0, 0.0f0, 1.0f0))
    pixels = readback_framebuffer(framebuffer)
    capture_bytes = _capture_bytes(pixels)
    state.draw_calls += 1
    state.readback_bytes += length(capture_bytes)
    if !state.probe_compiled
        state.probe_compiled = true
        state.pipeline_compilations += 1
    end
    center = pixels[cld(size(pixels, 1), 2), cld(size(pixels, 2), 2)]
    green_pixels = count(pixel -> pixel[2] > 0.9f0 && pixel[1] < 0.1f0, pixels)
    return (
        schema="wge.lava-render-probe/v1",
        backend_id="lava-vulkan",
        adapter_revision=ADAPTER_REVISION,
        lava_revision=LAVA_REVISION,
        width_px=width,
        height_px=height,
        matrix_size=(size(pixels, 1), size(pixels, 2)),
        center_rgba=collect(center),
        green_pixels=green_pixels,
        capture_sha256="sha256:" * bytes2hex(sha256(capture_bytes)),
        capture_base64=base64encode(capture_bytes),
        telemetry=(
            upload_bytes=0,
            readback_bytes=Int(state.readback_bytes - readback_bytes_start),
            draw_calls=Int(state.draw_calls - draw_calls_start),
            dispatch_calls=0,
            pipeline_compilations=Int(state.pipeline_compilations - pipeline_compilations_start),
        ),
    )
end

function render_depth_probe(state::LavaBackend=backend())
    width, height = (16, 16)
    framebuffer = _framebuffer!(state, width, height, true)
    target = OffscreenTarget(framebuffer)
    draw!(
        state.queue,
        state.depth_pipeline,
        target,
        3;
        args=(Vec4f(0.0f0, 0.0f0, 1.0f0, 1.0f0), 0.3f0),
        clear_color=(0.0f0, 0.0f0, 0.0f0, 1.0f0),
    )
    draw!(
        state.queue,
        state.depth_pipeline,
        target,
        3;
        args=(Vec4f(1.0f0, 0.0f0, 0.0f0, 1.0f0), 0.7f0),
        clear_color=nothing,
        depth_clear=nothing,
    )
    state.draw_calls += 2
    if !state.depth_compiled
        state.depth_compiled = true
        state.pipeline_compilations += 1
    end
    pixels = readback_framebuffer(framebuffer)
    center = pixels[cld(size(pixels, 1), 2), cld(size(pixels, 2), 2)]
    return (
        schema="wge.lava-depth-probe/v1",
        width_px=width,
        height_px=height,
        center_rgba=collect(center),
        nearer_fragment_won=center[3] > 0.9f0 && center[1] < 0.1f0,
        depth_attachment=true,
    )
end

function _texture_resources!(state::LavaBackend)
    current = state.texture_resources
    current !== nothing && return current
    texel = (0.2f0, 0.7f0, 0.9f0, 1.0f0)
    data = fill(texel, 4, 4)
    texture = Lava.LavaTexture2D(data; ctx=state.context, filter=:nearest, wrap=:clamp)
    sampler = Lava.LavaSampler(ctx=state.context, filter=:nearest, wrap=:clamp)
    bindings = Lava.bind_textures([texture * sampler])
    created = TextureProbeResources(texture, sampler, bindings)
    state.texture_resources = created
    state.upload_bytes += UInt64(sizeof(texel) * length(data))
    return created
end

function _decode_texture_channel(::Val{:linear}, channel::Float32)::Float32
    return channel
end

function _decode_texture_channel(::Val{:srgb}, channel::Float32)::Float32
    return _srgb_to_linear(channel)
end

function _decode_texture_channel(::Val{:normal_map}, channel::Float32)::Float32
    return channel
end

function _decode_texture_channel(::Val{:data}, channel::Float32)::Float32
    return channel
end

"""Validate a packet texture payload into explicit CPU upload levels: the base
level plus any authority-conditioned mip levels, each decoded to the adapter's
proven RGBA32F sample format. Packet-level byte framing is revalidated before
decoding; the adapter never trusts packet framing."""
function _texture_levels(payload)
    width = UInt32(payload.width)
    height = UInt32(payload.height)
    width > 0 && height > 0 ||
        throw(AdapterError("malformed_texture", "texture dimensions must be positive"))
    color_space = payload.color_space
    levels = Vector{Matrix{NTuple{4,Float32}}}()
    push!(levels, _texture_matrix(payload.bytes, width, height, color_space))
    for mip in get(payload, :mips, ())
        # `UInt32 ÷ Int` promotes to UInt64, and `_texture_matrix` only accepts
        # UInt32 dimensions: with an Int literal here every packet texture that
        # carried a mip chain crashed the worker (found in EDGE-1; no test had
        # pushed a real chain through this loop).
        width = max(width ÷ UInt32(2), UInt32(1))
        height = max(height ÷ UInt32(2), UInt32(1))
        expected = 4 * Int(width) * Int(height)
        length(mip.bytes) == expected ||
            throw(AdapterError(
                "malformed_texture",
                "mip level payload is $expected bytes, received $(length(mip.bytes))",
            ))
        push!(levels, _texture_matrix(mip.bytes, width, height, color_space))
    end
    return levels
end

"""A (height, width) texel matrix in Vulkan upload order: x fastest, then y.
`permutedims` gives the (width, height) matrix whose column-major memory is
exactly that; the matrix itself runs down columns (see `_lava_texture2d!`)."""
_upload_layout(level::Matrix{NTuple{4,Float32}}) = permutedims(level)

"""Create a Vulkan image with one mip level per validated CPU level, copy each
level through a single shared staging buffer, and return the resident texture.
The view spans every level so sampler LOD range 0..levels-1 is backed by real
data. Levels are RGBA32F — the adapter's proven sample format — until a
narrower-format seam is validated. Image, memory, and view are owned by the
texture; the pinned Lava rev owns destruction through finalizers, so nothing
is destroyed manually."""
function _lava_texture2d!(
    state::LavaBackend,
    levels::Vector{Matrix{NTuple{4,Float32}}},
)
    isempty(levels) &&
        throw(AdapterError("malformed_texture", "texture needs at least one level"))
    base = levels[1]
    base_height, base_width = size(base)
    base_width > 0 && base_height > 0 ||
        throw(AdapterError("malformed_texture", "texture dimensions must be positive"))
    level_count = UInt32(length(levels))
    format = Vulkan.FORMAT_R32G32B32A32_SFLOAT
    dev = state.context.device

    image = Vulkan.Image(
        dev,
        Vulkan.IMAGE_TYPE_2D,
        format,
        Vulkan.Extent3D(UInt32(base_width), UInt32(base_height), UInt32(1)),
        level_count,
        UInt32(1),
        Vulkan.SAMPLE_COUNT_1_BIT,
        Vulkan.IMAGE_TILING_OPTIMAL,
        Vulkan.IMAGE_USAGE_SAMPLED_BIT | Vulkan.IMAGE_USAGE_TRANSFER_DST_BIT,
        Vulkan.SHARING_MODE_EXCLUSIVE,
        UInt32[],
        Vulkan.IMAGE_LAYOUT_UNDEFINED,
    )
    memory = Lava.alloc_image_memory(state.context, image)
    view = Vulkan.ImageView(
        dev,
        image,
        Vulkan.IMAGE_VIEW_TYPE_2D,
        format,
        Vulkan.ComponentMapping(
            Vulkan.COMPONENT_SWIZZLE_IDENTITY,
            Vulkan.COMPONENT_SWIZZLE_IDENTITY,
            Vulkan.COMPONENT_SWIZZLE_IDENTITY,
            Vulkan.COMPONENT_SWIZZLE_IDENTITY,
        ),
        Vulkan.ImageSubresourceRange(
            Vulkan.IMAGE_ASPECT_COLOR_BIT,
            UInt32(0),
            level_count,
            UInt32(0),
            UInt32(1),
        ),
    )
    texture = Lava.LavaTexture2D{NTuple{4,Float32}}(
        image,
        memory,
        view,
        base_width,
        base_height,
        format,
        state.context,
    )

    bq = state.context.default_bq
    cmd = Lava.ensure_active_batch!(bq).cmd_buf
    level_bytes = [16 * size(level, 2) * size(level, 1) for level in levels]
    # Exclusive prefix: level i lives at [offsets[i], offsets[i] + level_bytes[i]).
    # Julia's accumulate folds `init` into the first element, so accumulate(+,
    # bytes; init=0) yields the inclusive prefix [b1, b1+b2, ...] and level 1
    # would land past the end of the staging buffer.
    offsets = pushfirst!(accumulate(+, level_bytes), 0)
    staging_buf, _, mapped_ptr, _ = Lava.get_staging(bq, offsets[end])
    for (index, level) in enumerate(levels)
        write_ptr = Ptr{UInt8}(mapped_ptr) + offsets[index]
        # `level` is (height, width), indexed [row, column]. Julia stores it
        # column-major, so its raw memory runs DOWN a column; a tightly packed
        # Vulkan buffer copy reads ACROSS a row (x fastest). Copying `level`
        # directly therefore uploaded every square texture transposed (and
        # would scramble a non-square one): planks and bark furrows rendered at
        # 90° and normal maps lit across the wrong tangent axes, from the first
        # textured packet until N-2's non-square IBL atlas exposed it.
        packed = _upload_layout(level)
        # Both buffers must be GC-preserved across the raw copy. unsafe_copyto!
        # needs matching element types, so the staging slice is reinterpreted
        # to RGBA32F and the length is in 16-byte elements (staging offsets
        # are 16-byte multiples, so alignment holds).
        GC.@preserve packed unsafe_copyto!(
            Ptr{NTuple{4,Float32}}(write_ptr), pointer(packed), length(packed)
        )
    end

    Lava.transition_image!(
        cmd,
        image,
        Vulkan.IMAGE_LAYOUT_UNDEFINED,
        Vulkan.IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        Vulkan.PIPELINE_STAGE_TOP_OF_PIPE_BIT,
        Vulkan.PIPELINE_STAGE_TRANSFER_BIT,
        Vulkan.AccessFlag(0),
        Vulkan.ACCESS_TRANSFER_WRITE_BIT,
    )
    regions = [
        Vulkan.BufferImageCopy(
            UInt64(offsets[index]),
            UInt32(0),
            UInt32(0),
            Vulkan.ImageSubresourceLayers(
                Vulkan.IMAGE_ASPECT_COLOR_BIT,
                UInt32(index - 1),
                UInt32(0),
                UInt32(1),
            ),
            Vulkan.Offset3D(0, 0, 0),
            Vulkan.Extent3D(
                UInt32(size(level, 2)),
                UInt32(size(level, 1)),
                UInt32(1),
            ),
        )
        for (index, level) in enumerate(levels)
    ]
    Vulkan.cmd_copy_buffer_to_image(
        cmd,
        staging_buf,
        image,
        Vulkan.IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        regions,
    )
    Lava.transition_image!(
        cmd,
        image,
        Vulkan.IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        Vulkan.IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
        Vulkan.PIPELINE_STAGE_TRANSFER_BIT,
        Vulkan.PIPELINE_STAGE_FRAGMENT_SHADER_BIT,
        Vulkan.ACCESS_TRANSFER_WRITE_BIT,
        Vulkan.ACCESS_SHADER_READ_BIT,
    )
    Lava.pin!(Lava.ensure_active_batch!(bq), texture)
    Lava.flush!(bq, dev)
    return texture
end

"""Build a sampler whose LOD range spans the resident mip chain. The pinned
Lava revision hardcodes min/max LOD to zero, so the handle is constructed here
from the same argument order as LavaSampler; max_lod = 0 reproduces it."""
function _lod_sampler!(
    state::LavaBackend,
    max_lod::UInt32;
    filter::Symbol=:linear,
    wrap::Symbol=:clamp,
)
    vk_filter = filter == :linear ? Vulkan.FILTER_LINEAR :
                filter == :nearest ? Vulkan.FILTER_NEAREST :
                throw(AdapterError("unsupported_texture", "unknown texture filter $filter"))
    vk_wrap = wrap == :clamp ? Vulkan.SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE :
              wrap == :repeat ? Vulkan.SAMPLER_ADDRESS_MODE_REPEAT :
              wrap == :mirror ? Vulkan.SAMPLER_ADDRESS_MODE_MIRRORED_REPEAT :
              throw(AdapterError("unsupported_texture", "unknown texture wrap $wrap"))
    handle = Vulkan.Sampler(
        state.context.device,
        vk_filter,
        vk_filter,
        Vulkan.SAMPLER_MIPMAP_MODE_LINEAR,
        vk_wrap,
        vk_wrap,
        vk_wrap,
        0.0f0,
        false,
        0.0f0,
        false,
        Vulkan.COMPARE_OP_ALWAYS,
        0.0f0,
        Float32(max_lod),
        Vulkan.BORDER_COLOR_FLOAT_TRANSPARENT_BLACK,
        false,
    )
    return Lava.LavaSampler(handle, filter, wrap, 0.0f0, state.context)
end

"""Assemble the residency telemetry shape for a frame receipt, mirroring the
Rust contract's independent expectation exactly: distinct source texture ids
referenced by any material in the packet, summed mip level counts and decoded
payload bytes, and the widest sampler LOD span. Adapter-owned default textures
are excluded. Nothing is reported when no source textures are referenced."""
function _packet_residency_summary(packet::WGEGraphics.GraphicsScenePacket)
    referenced = Set{String}()
    for material in packet.materials
        union!(referenced, material.texture_ids)
        for texture_id in (
            material.normal_texture_id,
            material.roughness_texture_id,
            material.occlusion_texture_id,
            material.emissive_texture_id,
        )
            texture_id === nothing || push!(referenced, texture_id)
        end
    end
    isempty(referenced) && return nothing
    texture_count = 0
    mip_levels = 0
    payload_bytes = 0
    max_sampler_lod = 0
    for texture in packet.textures
        texture.texture_id in referenced || continue
        texture.payload === nothing && throw(AdapterError(
            "unsupported_texture",
            "referenced texture $(texture.texture_id) has no inline payload for native residency",
        ))
        texture_count += 1
        mip_levels += Int(texture.mip_levels)
        payload_bytes += length(texture.payload)
        for mip in texture.mip_chain
            payload_bytes += length(mip.bytes)
        end
        max_sampler_lod = max(max_sampler_lod, Int(texture.mip_levels) - 1)
    end
    texture_count > 0 || return nothing
    return (
        texture_count=texture_count,
        mip_levels=mip_levels,
        payload_bytes=payload_bytes,
        max_sampler_lod=max_sampler_lod,
    )
end

function _texture_matrix(
    bytes::Vector{UInt8},
    width::UInt32,
    height::UInt32,
    color_space::Symbol=:linear,
)::Matrix{NTuple{4,Float32}}
    color_space == :linear && return _texture_matrix(bytes, width, height, Val{:linear}())
    color_space == :srgb && return _texture_matrix(bytes, width, height, Val{:srgb}())
    color_space == :normal_map && return _texture_matrix(bytes, width, height, Val{:normal_map}())
    color_space == :data && return _texture_matrix(bytes, width, height, Val{:data}())
    throw(AdapterError("unsupported_texture", "texture color space is not a color payload"))
end

function _texture_matrix(
    bytes::Vector{UInt8},
    width::UInt32,
    height::UInt32,
    decoder::Val,
)::Matrix{NTuple{4,Float32}}
    width_int = Int(width)
    height_int = Int(height)
    width_int > 0 && height_int > 0 ||
        throw(AdapterError("malformed_texture", "texture dimensions must be positive"))
    expected_length = 4 * width_int * height_int
    length(bytes) == expected_length ||
        throw(AdapterError("malformed_texture", "texture payload length does not match dimensions"))
    data = Matrix{NTuple{4,Float32}}(undef, height_int, width_int)
    offset = 1
    for row in 1:height_int, column in 1:width_int
        data[row, column] = ntuple(
            index -> begin
                channel = Float32(bytes[offset+index-1]) / 255.0f0
                index == 4 ? channel : _decode_texture_channel(decoder, channel)
            end,
            4,
        )
        offset += 4
    end
    return data
end

function _material_texture_id(
    material::WGEGraphics.MaterialPacket,
    ::Val{:albedo},
)::Union{Nothing,String}
    length(material.texture_ids) <= 1 ||
        throw(AdapterError("unsupported_material", "native path supports one albedo texture per material"))
    return isempty(material.texture_ids) ? nothing : only(material.texture_ids)
end

_material_texture_id(material::WGEGraphics.MaterialPacket, ::Val{:normal}) = material.normal_texture_id
_material_texture_id(material::WGEGraphics.MaterialPacket, ::Val{:roughness}) = material.roughness_texture_id
_material_texture_id(material::WGEGraphics.MaterialPacket, ::Val{:occlusion}) = material.occlusion_texture_id
_material_texture_id(material::WGEGraphics.MaterialPacket, ::Val{:emissive}) = material.emissive_texture_id

function _default_texture_payload(::Val{:albedo})
    return (bytes=UInt8[255, 255, 255, 255], width=UInt32(1), height=UInt32(1), color_space=:linear)
end

function _default_texture_payload(::Val{:normal})
    return (bytes=UInt8[128, 128, 255, 255], width=UInt32(1), height=UInt32(1), color_space=:normal_map)
end

function _default_texture_payload(::Val{:roughness})
    return (bytes=UInt8[255, 255, 255, 255], width=UInt32(1), height=UInt32(1), color_space=:data)
end

function _default_texture_payload(::Val{:occlusion})
    return (bytes=UInt8[255, 255, 255, 255], width=UInt32(1), height=UInt32(1), color_space=:data)
end

function _default_texture_payload(::Val{:emissive})
    return (bytes=UInt8[0, 0, 0, 255], width=UInt32(1), height=UInt32(1), color_space=:linear)
end

function _texture_payload(
    packet::WGEGraphics.GraphicsScenePacket,
    ::Nothing,
    role::Val,
)
    return _default_texture_payload(role)
end

_texture_color_spaces(::Val{:albedo}) = (:srgb, :linear)
_texture_color_spaces(::Val{:normal}) = (:normal_map,)
_texture_color_spaces(::Val{:roughness}) = (:data, :linear)
_texture_color_spaces(::Val{:occlusion}) = (:data, :linear)
_texture_color_spaces(::Val{:emissive}) = (:srgb, :linear)

function _texture_payload(
    packet::WGEGraphics.GraphicsScenePacket,
    texture_id::String,
    role::Val,
)
    texture_index = findfirst(texture -> texture.texture_id == texture_id, packet.textures)
    texture_index === nothing &&
        throw(AdapterError("provenance", "material texture $texture_id is absent from the packet"))
    texture = packet.textures[texture_index]
    allowed_color_spaces = _texture_color_spaces(role)
    texture.color_space in allowed_color_spaces ||
        throw(AdapterError(
            "unsupported_texture",
            "texture $texture_id has an invalid color space for $(typeof(role).parameters[1])",
        ))
    texture.payload === nothing &&
        throw(AdapterError("unsupported_texture", "native path requires inline texture payloads"))
    return (
        bytes=texture.payload::Vector{UInt8},
        width=texture.width_px,
        height=texture.height_px,
        color_space=texture.color_space,
        mips=texture.mip_chain,
    )
end

"""GPU textures for a material, uploaded once per (content, material).

Split from the binding construction so that a second sampling spec for the same
material reuses the uploaded image instead of re-uploading it. Before this split
the sampler and the upload were fused, which is precisely why tiling a material
would have multiplied `upload_bytes` — and that counter is independently
validated on the Rust side.
"""
function _material_textures!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    material::WGEGraphics.MaterialPacket,
)::MaterialTextures
    key = (packet.content_sha256, material.material_id)
    existing = get(state.material_textures, key, nothing)
    existing === nothing || return existing
    roles = (
        Val{:albedo}(),
        Val{:normal}(),
        Val{:roughness}(),
        Val{:occlusion}(),
        Val{:emissive}(),
    )
    payloads = map(
        role -> _texture_payload(packet, _material_texture_id(material, role), role),
        roles,
    )
    levels = map(_texture_levels, payloads)
    textures = map(texture_levels -> _lava_texture2d!(state, texture_levels), levels)
    max_sampler_lod = UInt32(maximum(
        maximum(length(texture_levels) - 1, init=0) for texture_levels in levels
    ))
    for texture_levels in levels
        state.upload_bytes += UInt64(sum(
            16 * size(level, 2) * size(level, 1) for level in texture_levels; init=0
        ))
    end
    created = MaterialTextures(
        textures[1],
        textures[2],
        textures[3],
        textures[4],
        textures[5],
        max_sampler_lod,
    )
    state.material_textures[key] = created
    return created
end

"""Sampler for a sampling spec, built once and shared.

Samplers carry no per-material state, so one per spec is enough for the whole
scene no matter how many materials use it.
"""
function _surface_sampler!(
    state::LavaBackend,
    spec::SamplerSpec,
    max_lod::UInt32,
)::Lava.LavaSampler
    # Keyed by the mip range too. Keyed by mode alone, the first material to
    # ask fixed the max LOD for every later one (the latent defect recorded in
    # N-4): a single-level texture first would strip every later chain.
    key = (spec, max_lod)
    existing = get(state.surface_samplers, key, nothing)
    existing === nothing || return existing
    created = _lod_sampler!(state, max_lod; filter=spec.filter, wrap=spec.wrap)
    state.surface_samplers[key] = created
    return created
end

const IBL_ENVIRONMENT_TEXTURE_ID = "wge-ibl-environment"
const IBL_BRDF_LUT_TEXTURE_ID = "wge-ibl-brdf-lut"

function _ibl_resources!(state::LavaBackend, packet::WGEGraphics.GraphicsScenePacket)::IblResources
    current = state.ibl_resources
    current !== nothing && current.cache_key == packet.content_sha256 && return current
    enabled = _ibl_enabled(packet.render_policy)
    upload(payload) = begin
        levels = _texture_levels(payload)
        state.upload_bytes += UInt64(sum(16 * size(level, 2) * size(level, 1) for level in levels; init=0))
        _lava_texture2d!(state, levels)
    end
    placeholder = upload(_default_texture_payload(Val{:occlusion}()))
    environment = enabled ? upload(_texture_payload(packet, IBL_ENVIRONMENT_TEXTURE_ID, Val{:albedo}())) : placeholder
    brdf_lut = enabled ? upload(_texture_payload(packet, IBL_BRDF_LUT_TEXTURE_ID, Val{:roughness}())) : placeholder
    sampler = _surface_sampler!(state, CLAMPED_LINEAR_SPEC, UInt32(0))
    created = IblResources(packet.content_sha256, environment, brdf_lut, placeholder, sampler)
    state.ibl_resources = created
    return created
end

function _material_texture_resources!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    shadow::ShadowResources,
    material::WGEGraphics.MaterialPacket,
    spec::SamplerSpec,
)
    cache = state.material_texture_resources
    if any(resource -> resource.cache_key != packet.content_sha256, values(cache))
        empty!(cache)
        empty!(state.material_textures)
        empty!(state.surface_samplers)
    end
    key = (packet.content_sha256, material.material_id, spec)
    cached = get(cache, key, nothing)
    cached === nothing || return cached
    textures = _material_textures!(state, packet, material)
    sampler = _surface_sampler!(state, spec, textures.max_sampler_lod)
    ibl = _ibl_resources!(state, packet)
    # Bindings 6..13 are stand-ins so the IBL textures sit at 14/15 in every
    # surface pipeline (the layered terrain uses 0..13 for its layers).
    bindings = Lava.bind_textures([
        textures.albedo_texture * sampler,
        textures.normal_texture * sampler,
        textures.roughness_texture * sampler,
        textures.occlusion_texture * sampler,
        textures.emissive_texture * sampler,
        shadow.texture * shadow.sampler,
        (ibl.placeholder * ibl.sampler for _ in 6:13)...,
        ibl.environment * ibl.sampler,
        ibl.brdf_lut * ibl.sampler,
    ])
    created = MaterialTextureResources(
        packet.content_sha256,
        material.material_id,
        spec,
        textures,
        sampler,
        bindings,
        textures.max_sampler_lod,
    )
    cache[key] = created
    return created
end

"""Lower the layer set into the seven layered-terrain uniforms.

Per layer `a = (1/metres_per_repeat, slope_lo, slope_hi, macro_threshold)` and
`b = (height_lo_m, height_hi_m, macro_softness, normal_scale)`. An absent term
becomes a ramp that is 1 everywhere; a padding layer (two-layer sets fill the
third slot) gets the slope ramp (2, 3), which is 0 for every real slope.
Then `macro = (cycles per metre, amplitude, 0, 0)` from the render policy.
"""
function _terrain_layer_uniforms(packet::WGEGraphics.GraphicsScenePacket)::NTuple{7,Vec4f}
    layers = packet.terrain.layers.layers
    normal_scales = Float32[_material(packet, layer.material_id).normal_scale for layer in layers]
    return _terrain_layer_uniforms(layers, normal_scales, packet.render_policy.terrain_surface)
end

function _terrain_layer_uniforms(
    layers::Vector{WGEGraphics.TerrainLayerPacket},
    normal_scales::Vector{Float32},
    terrain::WGEGraphics.TerrainSurfacePolicy,
)::NTuple{7,Vec4f}
    vectors = Vec4f[]
    for index in 1:WGEGraphics.MAX_TERRAIN_LAYERS
        padding = index > length(layers)
        layer = layers[min(index, length(layers))]
        inverse_repeat = 1000.0f0 / Float32(layer.metres_per_repeat_milli)
        normal_scale = normal_scales[min(index, length(layers))]
        coverage = layer.coverage
        slope = padding ? (2.0f0, 3.0f0) :
            (coverage === nothing || coverage.slope_bp === nothing) ? (-2.0f0, -1.0f0) :
            (_bp(coverage.slope_bp[1]), _bp(coverage.slope_bp[2]))
        height = (coverage === nothing || coverage.height_mm === nothing) ? (-2.0f9, -1.0f9) :
            (Float32(coverage.height_mm[1]) / 1000.0f0, Float32(coverage.height_mm[2]) / 1000.0f0)
        macro_ramp = (coverage === nothing || coverage.macro_ramp === nothing) ? (-2.0f0, 0.5f0) :
            (_bp(coverage.macro_ramp[1]), _bp(coverage.macro_ramp[2]))
        push!(vectors, Vec4f(inverse_repeat, slope[1], slope[2], macro_ramp[1]))
        push!(vectors, Vec4f(height[1], height[2], macro_ramp[2], normal_scale))
    end
    push!(vectors, Vec4f(Float32(terrain.macro_frequency_milli) / 1000.0f0, _bp(terrain.macro_variation_bp), 0.0f0, 0.0f0))
    return Tuple(vectors)
end

function _terrain_layer_resources!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    shadow::ShadowResources,
)::TerrainLayerResources
    current = state.terrain_layer_resources
    current !== nothing && current.cache_key == packet.content_sha256 && return current
    layers = packet.terrain.layers
    count = length(layers.layers)
    sets = [
        _material_textures!(state, packet, _material(packet, layers.layers[min(index, count)].material_id))
        for index in 1:WGEGraphics.MAX_TERRAIN_LAYERS
    ]
    macro_levels = _texture_levels(_texture_payload(packet, layers.macro_texture_id, Val{:roughness}()))
    macro_texture = _lava_texture2d!(state, macro_levels)
    state.upload_bytes += UInt64(sum(16 * size(level, 2) * size(level, 1) for level in macro_levels; init=0))
    # Not `_surface_sampler!`: that cache keeps the max LOD of whichever material
    # created it first, which would silently strip the layers' mips.
    max_lod = max(maximum(set.max_sampler_lod for set in sets), UInt32(length(macro_levels) - 1))
    sampler = _lod_sampler!(state, max_lod; filter=:linear, wrap=:repeat)
    ibl = _ibl_resources!(state, packet)
    base, middle, top = sets
    bindings = Lava.bind_textures([
        base.albedo_texture * sampler,
        base.normal_texture * sampler,
        base.roughness_texture * sampler,
        base.occlusion_texture * sampler,
        middle.albedo_texture * sampler,
        shadow.texture * shadow.sampler,
        middle.normal_texture * sampler,
        middle.roughness_texture * sampler,
        middle.occlusion_texture * sampler,
        top.albedo_texture * sampler,
        top.normal_texture * sampler,
        top.roughness_texture * sampler,
        top.occlusion_texture * sampler,
        macro_texture * sampler,
        ibl.environment * ibl.sampler,
        ibl.brdf_lut * ibl.sampler,
    ])
    created = TerrainLayerResources(packet.content_sha256, bindings, sampler, _terrain_layer_uniforms(packet))
    state.terrain_layer_resources = created
    return created
end

function _material_texture_enabled(material::WGEGraphics.MaterialPacket)::Float32
    length(material.texture_ids) <= 1 ||
        throw(AdapterError("unsupported_material", "native path supports one texture per material"))
    return isempty(material.texture_ids) ? 0.0f0 : 1.0f0
end

function render_texture_probe(state::LavaBackend=backend())
    resource = _texture_resources!(state)
    width, height = (16, 16)
    framebuffer = _framebuffer!(state, width, height, false)
    target = OffscreenTarget(framebuffer)
    draw!(
        state.queue,
        state.texture_pipeline,
        target,
        3;
        descriptor_set_layout=resource.bindings.layout,
        descriptor_set=resource.bindings.set,
        clear_color=(0.0f0, 0.0f0, 0.0f0, 1.0f0),
    )
    state.draw_calls += 1
    if !state.texture_compiled
        state.texture_compiled = true
        state.pipeline_compilations += 1
    end
    pixels = readback_framebuffer(framebuffer)
    center = pixels[cld(size(pixels, 1), 2), cld(size(pixels, 2), 2)]
    expected = (0.2f0, 0.7f0, 0.9f0, 1.0f0)
    return (
        schema="wge.lava-texture-probe/v1",
        width_px=width,
        height_px=height,
        center_rgba=collect(center),
        expected_rgba=collect(expected),
        texture_sampled=all(isapprox(center[index], expected[index]; atol=0.05f0) for index in 1:4),
        texture_binding_count=1,
    )
end

function _basis_vectors(
    direction::Vec4f,
    up_hint::Vec4f,
    label::String,
)::Tuple{Vec4f,Vec4f,Vec4f}
    direction_length_squared = _dot_vector(direction, direction)
    up_length_squared = _dot_vector(up_hint, up_hint)
    isfinite(direction_length_squared) && isfinite(up_length_squared) &&
        direction_length_squared > 1.0f-8 && up_length_squared > 1.0f-8 ||
        throw(AdapterError("invalid_basis", "$label contains a degenerate direction"))
    forward = _normalize_vector(direction)
    requested_up = _normalize_vector(up_hint)
    abs(_dot_vector(forward, requested_up)) < 0.999f0 ||
        throw(AdapterError("invalid_basis", "$label directions must not be collinear"))
    right = _normalize_vector(_cross_vector(forward, requested_up))
    up = _normalize_vector(_cross_vector(right, forward))
    return forward, right, up
end

"""Up hint for a directional light's shadow basis.

+Y unless the light is near +-Z or within ~8 deg of vertical. The vertical test
was missing (only z was checked), so a sun straight overhead was collinear
with its +Y hint and failed basis construction (found by CALIBRATION-1's
overcast rig). The 0.99 threshold leaves every existing light (steepest
|y| = 0.952) on its old basis, byte-identical.
"""
function _shadow_up_hint(direction::Vec4f)::Vec4f
    vertical = abs(_normalize_vector(direction)[2])
    return abs(direction[3]) < 0.9f0 && vertical < 0.99f0 ?
        Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0) :
        Vec4f(1.0f0, 0.0f0, 0.0f0, 0.0f0)
end

function _camera_projection(
    projection::WGEGraphics.OrthographicProjection,
    aspect::Float32,
    near_plane::Float32,
    far_plane::Float32,
)::Tuple{Vec4f,Float32}
    projection.span_m > 0.0f0 ||
        throw(AdapterError("invalid_projection", "orthographic span must be positive"))
    return (
        Vec4f(
            projection.span_m * aspect * 0.5f0,
            projection.span_m * 0.5f0,
            near_plane,
            far_plane,
        ),
        0.0f0,
    )
end

function _camera_projection(
    projection::WGEGraphics.PerspectiveProjection,
    aspect::Float32,
    near_plane::Float32,
    far_plane::Float32,
)::Tuple{Vec4f,Float32}
    half_fov = Float32(tan(Float64(projection.fov_y_degrees) * pi / 360.0))
    half_fov > 0.0f0 ||
        throw(AdapterError("invalid_projection", "perspective field of view is invalid"))
    return (
        Vec4f(half_fov * aspect, half_fov, near_plane, far_plane),
        1.0f0,
    )
end

function _camera_frame(camera::WGEGraphics.CameraPacket)::CameraFrame
    position = Vec4f(camera.position_xyz_m..., 0.0f0)
    forward, right, up = _basis_vectors(
        Vec4f(camera.forward_xyz..., 0.0f0),
        Vec4f(camera.up_xyz..., 0.0f0),
        "camera forward/up basis",
    )
    aspect = Float32(camera.width_px) / Float32(camera.height_px)
    projection, mode = _camera_projection(
        camera.projection,
        aspect,
        camera.near_plane_m,
        camera.far_plane_m,
    )
    return CameraFrame(
        position,
        right,
        up,
        forward,
        projection,
        mode,
        Float32(camera.width_px),
        Float32(camera.height_px),
    )
end

"""Shadow frame fitted to the camera frustum truncated at `view_distance`.

The historical fit encloses the whole terrain and every instance, which ties
shadow resolution to world size. This fit encloses only the eight corners of
the camera frustum slice [near, min(far, view_distance)]: the bounding sphere
is centred on their mean and spans their farthest corner, and the orthographic
extent is exactly that sphere's diameter (the historical fit's 2.5x margin was
headroom for a sphere that did not bound the casters tightly; this one does).
Casters up to two radii toward the light stay inside the near plane, so a tree
outside the slice can still shadow ground inside it.
"""
function _view_fitted_shadow_frame(
    view_camera::WGEGraphics.CameraPacket,
    light_direction::Vec4f,
    right::Vec4f,
    up::Vec4f,
    view_distance::Float32,
)::CameraFrame
    camera = _camera_frame(view_camera)
    near = camera.projection[3]
    far = min(camera.projection[4], view_distance)
    far > near || throw(AdapterError(
        "unsupported_render_policy",
        "render_policy.shadow_fit.view_distance_m=$(view_distance) does not reach past the camera near plane",
    ))
    corners = Vec4f[]
    for depth in (near, far)
        half_width = camera.mode < 0.5f0 ? camera.projection[1] : camera.projection[1] * depth
        half_height = camera.mode < 0.5f0 ? camera.projection[2] : camera.projection[2] * depth
        for sx in (-1.0f0, 1.0f0), sy in (-1.0f0, 1.0f0)
            push!(corners, Vec4f(
                camera.position[1] + camera.forward[1] * depth + camera.right[1] * sx * half_width + camera.up[1] * sy * half_height,
                camera.position[2] + camera.forward[2] * depth + camera.right[2] * sx * half_width + camera.up[2] * sy * half_height,
                camera.position[3] + camera.forward[3] * depth + camera.right[3] * sx * half_width + camera.up[3] * sy * half_height,
                0.0f0,
            ))
        end
    end
    center = Vec4f(
        sum(corner[1] for corner in corners) / 8.0f0,
        sum(corner[2] for corner in corners) / 8.0f0,
        sum(corner[3] for corner in corners) / 8.0f0,
        0.0f0,
    )
    radius = 1.0f0
    for corner in corners
        offset = Vec4f(corner[1] - center[1], corner[2] - center[2], corner[3] - center[3], 0.0f0)
        radius = max(radius, sqrt(_dot_vector(offset, offset)))
    end
    distance = max(2.0f0 * radius, 32.0f0)
    position = Vec4f(
        center[1] - light_direction[1] * distance,
        center[2] - light_direction[2] * distance,
        center[3] - light_direction[3] * distance,
        0.0f0,
    )
    span = 2.0f0 * radius
    far_plane = 2.0f0 * distance + radius
    return CameraFrame(
        position,
        right,
        up,
        light_direction,
        Vec4f(0.5f0 * span, 0.5f0 * span, 0.1f0, far_plane),
        0.0f0,
        1.0f0,
        1.0f0,
    )
end

function _shadow_frame(
    packet::WGEGraphics.GraphicsScenePacket,
    lighting::DirectionalLighting,
)::CameraFrame
    raw_light_direction = lighting.direction
    up_hint = _shadow_up_hint(raw_light_direction)
    light_direction, right, up = _basis_vectors(
        raw_light_direction,
        up_hint,
        "directional light basis",
    )

    fit = packet.render_policy.shadow_fit
    if fit !== nothing
        return _view_fitted_shadow_frame(packet.camera, light_direction, right, up, Float32(fit.view_distance_m))
    end

    minimum_height = minimum(packet.terrain.heights_m)
    maximum_height = maximum(packet.terrain.heights_m)
    center = Vec4f(0.0f0, (minimum_height + maximum_height) * 0.5f0, 0.0f0, 0.0f0)
    horizontal_radius =
        0.5f0 * sqrt(packet.terrain.width_m * packet.terrain.width_m + packet.terrain.length_m * packet.terrain.length_m)
    radius = max(
        sqrt(
            horizontal_radius * horizontal_radius +
                0.25f0 * (maximum_height - minimum_height) * (maximum_height - minimum_height),
        ),
        1.0f0,
    )
    for instance in packet.instances
        world_center, bound_radius = _instance_world_bound(packet, instance)
        offset = Vec4f(
            world_center[1] - center[1],
            world_center[2] - center[2],
            world_center[3] - center[3],
            0.0f0,
        )
        radius = max(radius, sqrt(_dot_vector(offset, offset)) + bound_radius)
    end

    distance = max(2.0f0 * radius, 32.0f0)
    position = Vec4f(
        center[1] - light_direction[1] * distance,
        center[2] - light_direction[2] * distance,
        center[3] - light_direction[3] * distance,
        0.0f0,
    )
    span = max(2.5f0 * radius, 2.0f0)
    far_plane = 2.0f0 * distance + radius
    return CameraFrame(
        position,
        right,
        up,
        light_direction,
        Vec4f(0.5f0 * span, 0.5f0 * span, 0.1f0, far_plane),
        0.0f0,
        1.0f0,
        1.0f0,
    )
end

function _transition_color_to_sampled!(state::LavaBackend, framebuffer::Lava.LavaFramebuffer)
    batch = Lava.ensure_active_batch!(state.queue)
    Lava.transition_image!(
        batch.cmd_buf,
        framebuffer.color_image,
        Vulkan.IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        Vulkan.IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
        Vulkan.PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
        Vulkan.PIPELINE_STAGE_FRAGMENT_SHADER_BIT,
        Vulkan.ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
        Vulkan.ACCESS_SHADER_READ_BIT,
    )
    return nothing
end

const GPU_FRAME_TIMING_LABEL = "wge.graphics.frame"
const GPU_PASS_TIMING_LABELS = (
    prepare="wge.graphics.pass.prepare",
    scene_raster="wge.graphics.pass.scene-raster",
    resolve="wge.graphics.pass.resolve",
    overlay="wge.graphics.pass.overlay",
)

function _begin_gpu_frame_timing(state::LavaBackend)::Union{Nothing,Int}
    state.gpu_timestamp_supported || return nothing
    try
        Lava.reset_dispatch_timing!(state.context)
        Lava.enable_dispatch_timing!(true, state.context)
        batch = Lava.ensure_active_batch!(state.queue)
        slot = Lava.maybe_write_dispatch_start_timestamp!(
            state.context,
            batch.cmd_buf,
            GPU_FRAME_TIMING_LABEL;
            stage=Vulkan.PIPELINE_STAGE_TOP_OF_PIPE_BIT,
        )
        if slot < 0
            Lava.enable_dispatch_timing!(false, state.context)
            return nothing
        end
        return slot
    catch
        state.gpu_timestamp_supported = false
        Lava.enable_dispatch_timing!(false, state.context)
        return nothing
    end
end

function _begin_gpu_pass_timing(
    state::LavaBackend,
    label::AbstractString,
)::Union{Nothing,Int}
    state.gpu_timestamp_supported || return nothing
    try
        batch = Lava.ensure_active_batch!(state.queue)
        slot = Lava.maybe_write_dispatch_start_timestamp!(
            state.context,
            batch.cmd_buf,
            label;
            stage=Vulkan.PIPELINE_STAGE_TOP_OF_PIPE_BIT,
        )
        slot < 0 && return nothing
        return slot
    catch
        state.gpu_timestamp_supported = false
        return nothing
    end
end

function _end_gpu_frame_timing!(state::LavaBackend, ::Nothing)
    return nothing
end

function _end_gpu_frame_timing!(state::LavaBackend, start_slot::Int)
    try
        batch = Lava.ensure_active_batch!(state.queue)
        Lava.maybe_write_dispatch_end_timestamp!(
            state.context,
            batch.cmd_buf,
            start_slot,
            C_NULL;
            stage=Vulkan.PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,
            stage_mask=UInt32(Vulkan.PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT),
        )
    catch
        state.gpu_timestamp_supported = false
    end
    return nothing
end

function _end_gpu_pass_timing!(state::LavaBackend, ::Nothing)
    return nothing
end

function _end_gpu_pass_timing!(state::LavaBackend, start_slot::Int)
    try
        batch = Lava.ensure_active_batch!(state.queue)
        Lava.maybe_write_dispatch_end_timestamp!(
            state.context,
            batch.cmd_buf,
            start_slot,
            C_NULL;
            stage=Vulkan.PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,
            stage_mask=UInt32(Vulkan.PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT),
        )
    catch
        state.gpu_timestamp_supported = false
    end
    return nothing
end

function _read_gpu_timing_reports(state::LavaBackend, ::Nothing)::Nothing
    return nothing
end

function _read_gpu_timing_reports(state::LavaBackend, ::Int)::Union{Nothing,Vector}
    try
        return Lava.dispatch_timing_report(state.context; flush_first=false)
    catch
        state.gpu_timestamp_supported = false
        return nothing
    end
end

function _timing_report_us(
    ::Nothing,
    ::AbstractString,
)::Nothing
    return nothing
end

function _timing_report_us(
    reports::AbstractVector,
    label::AbstractString,
)::Union{Nothing,Int}
    try
        index = findfirst(report -> report.name == label, reports)
        index === nothing && return nothing
        elapsed_ns = reports[index].total_ns
        isfinite(elapsed_ns) && elapsed_ns >= 0.0 || return nothing
        return ceil(Int, elapsed_ns / 1_000.0)
    catch
        return nothing
    end
end

@inline function _elapsed_us(started_ns::UInt64)::Int
    return Int(cld(time_ns() - started_ns, UInt64(1_000)))
end

function _framebuffer_texture(framebuffer::Lava.LavaFramebuffer, state::LavaBackend)
    return Lava.LavaTexture2D{NTuple{4,Float32}}(
        framebuffer.color_image,
        framebuffer.color_memory,
        framebuffer.color_view,
        framebuffer.width,
        framebuffer.height,
        framebuffer.color_format,
        state.context,
    )
end

function _resolve_resources!(
    state::LavaBackend,
    framebuffer::Lava.LavaFramebuffer,
)::ResolveResources
    source_size = (framebuffer.width, framebuffer.height)
    current = state.resolve_resources
    current !== nothing && current.source_size == source_size && return current
    texture = _framebuffer_texture(framebuffer, state)
    sampler = Lava.LavaSampler(ctx=state.context, filter=:linear, wrap=:clamp)
    bindings = Lava.bind_textures([texture * sampler])
    created = ResolveResources(source_size, framebuffer, texture, sampler, bindings)
    state.resolve_resources = created
    return created
end

function _project_point(frame::CameraFrame, point::NTuple{3,<:Real})::Vec4f
    return _project_world(
        Vec4f(Float32(point[1]), Float32(point[2]), Float32(point[3]), 1.0f0),
        frame.position,
        frame.right,
        frame.up,
        frame.forward,
        frame.projection,
        frame.mode,
    )
end

function _terrain_material(packet::WGEGraphics.GraphicsScenePacket)::WGEGraphics.MaterialPacket
    return _material(packet, packet.terrain.material_id)
end

function _directional_lighting(
    kind::WGEGraphics.DirectionalLightPacket,
    color_rgb::NTuple{3,Float32},
    intensity::Float32,
)::DirectionalLighting
    return DirectionalLighting(
        Vec4f(kind.direction_xyz..., 0.0f0),
        Vec4f(color_rgb..., 1.0f0),
        intensity,
    )
end

function _directional_lighting(
    ::WGEGraphics.PointLightPacket,
    ::NTuple{3,Float32},
    ::Float32,
)::DirectionalLighting
    throw(AdapterError("unsupported_light", "native material path currently requires a directional light"))
end

function _lighting(packet::WGEGraphics.GraphicsScenePacket)::DirectionalLighting
    length(packet.lights) == 1 ||
        throw(AdapterError("unsupported_lighting", "native material path requires exactly one light intent"))
    light = only(packet.lights)
    return _directional_lighting(light.kind, light.color_rgb, light.intensity)
end

function _environment_lighting(environment::WGEGraphics.EnvironmentPacket)::EnvironmentLighting
    return EnvironmentLighting(
        Vec4f(environment.sky_top_rgb..., 1.0f0),
        Vec4f(environment.sky_horizon_rgb..., 1.0f0),
        Vec4f(environment.ground_rgb..., 1.0f0),
    )
end

function _validate_material(material::WGEGraphics.MaterialPacket)::Nothing
    material.alpha_mode == :blend && throw(
        AdapterError(
            "unsupported_material",
            "native path does not blend; condition BLEND materials to MASK with an explicit cutoff",
        ),
    )
    return nothing
end

"""True when a material needs the cutout pipelines (mask or double-sided).
Opaque single-sided materials keep the historical pipelines, byte-identical."""
_material_cutout(material::WGEGraphics.MaterialPacket)::Bool =
    material.alpha_mode == :mask || material.double_sided

function _material(packet::WGEGraphics.GraphicsScenePacket, material_id::String)::WGEGraphics.MaterialPacket
    for material in packet.materials
        if material.material_id == material_id
            _validate_material(material)
            return material
        end
    end
    throw(AdapterError("provenance", "mesh references an absent material"))
end

function _rotate_vector(rotation::Vec4f, vector::Vec4f)::Vec4f
    norm_squared =
        rotation[1] * rotation[1] +
            rotation[2] * rotation[2] +
            rotation[3] * rotation[3] +
            rotation[4] * rotation[4]
    inverse_norm = inv(sqrt(max(norm_squared, 1.0f-8)))
    x = rotation[1] * inverse_norm
    y = rotation[2] * inverse_norm
    z = rotation[3] * inverse_norm
    w = rotation[4] * inverse_norm
    tx = 2.0f0 * (y * vector[3] - z * vector[2])
    ty = 2.0f0 * (z * vector[1] - x * vector[3])
    tz = 2.0f0 * (x * vector[2] - y * vector[1])
    return Vec4f(
        vector[1] + w * tx + (y * tz - z * ty),
        vector[2] + w * ty + (z * tx - x * tz),
        vector[3] + w * tz + (x * ty - y * tx),
        vector[4],
    )
end

function _transform_tangent(
    rotation::Vec4f,
    local_tangent::Vec4f,
    scale::Vec4f,
)::Vec4f
    tangent = _normalize_vector(_rotate_vector(rotation, Vec4f(
        local_tangent[1] * scale[1],
        local_tangent[2] * scale[2],
        local_tangent[3] * scale[3],
        0.0f0,
    )))
    return Vec4f(tangent[1], tangent[2], tangent[3], local_tangent[4])
end

function _terrain_resources!(state::LavaBackend, packet::WGEGraphics.GraphicsScenePacket)
    current = state.terrain_resources
    current !== nothing && current.cache_key == packet.content_sha256 && return current
    heights = Lava.LavaArray{Float32,1}(packet.terrain.heights_m; bq=state.queue)
    slopes = Lava.LavaArray{Float32,1}(packet.terrain.slope_grade; bq=state.queue)
    regions = Lava.LavaArray{UInt8,1}(packet.terrain.region_codes; bq=state.queue)
    state.upload_bytes += UInt64(
        sizeof(Float32) * (length(packet.terrain.heights_m) + length(packet.terrain.slope_grade)) +
            sizeof(UInt8) * length(packet.terrain.region_codes),
    )
    created = TerrainResources(packet.content_sha256, heights, slopes, regions, Int32(packet.terrain.resolution))
    state.terrain_resources = created
    return created
end

function _clip_overlay_segment(
    frame::CameraFrame,
    first::NTuple{3,Float32},
    last::NTuple{3,Float32},
)::Union{Nothing,Tuple{NTuple{3,Float32},NTuple{3,Float32}}}
    first_relative = Vec4f(
        first[1] - frame.position[1],
        first[2] - frame.position[2],
        first[3] - frame.position[3],
        0.0f0,
    )
    last_relative = Vec4f(
        last[1] - frame.position[1],
        last[2] - frame.position[2],
        last[3] - frame.position[3],
        0.0f0,
    )
    first_depth = _dot_vector(frame.forward, first_relative)
    last_depth = _dot_vector(frame.forward, last_relative)
    depth_delta = last_depth - first_depth
    if depth_delta == 0.0f0
        frame.projection[3] <= first_depth <= frame.projection[4] || return nothing
        return first, last
    end
    lower = (frame.projection[3] - first_depth) / depth_delta
    upper = (frame.projection[4] - first_depth) / depth_delta
    lower > upper && ((lower, upper) = (upper, lower))
    start_amount = max(0.0f0, lower)
    end_amount = min(1.0f0, upper)
    start_amount <= end_amount || return nothing
    clipped_first = ntuple(
        index -> first[index] + (last[index] - first[index]) * start_amount,
        3,
    )
    clipped_last = ntuple(
        index -> first[index] + (last[index] - first[index]) * end_amount,
        3,
    )
    return clipped_first, clipped_last
end

function _overlay_pixels_per_meter(frame::CameraFrame, position::NTuple{3,Float32})::Float32
    relative = Vec4f(
        position[1] - frame.position[1],
        position[2] - frame.position[2],
        position[3] - frame.position[3],
        0.0f0,
    )
    depth = max(_dot_vector(frame.forward, relative), frame.projection[3])
    return frame.mode < 0.5f0 ?
        frame.height_px / (2.0f0 * frame.projection[2]) :
        frame.height_px / (2.0f0 * depth * frame.projection[2])
end

function _overlay_offset(
    frame::CameraFrame,
    projected::Vec4f,
    normal_x::Float32,
    normal_y::Float32,
    half_width_px::Float32,
)
    return Vec4f(
        projected[1] + normal_x * half_width_px / (0.5f0 * frame.width_px),
        projected[2] + normal_y * half_width_px / (0.5f0 * frame.height_px),
        projected[3],
        1.0f0,
    )
end

function _append_overlay_segment!(
    positions::Vector{Vec4f},
    colors::Vector{Vec4f},
    frame::CameraFrame,
    first::NTuple{3,Float32},
    last::NTuple{3,Float32},
    color::Vec4f,
    thickness_m::Float32,
)
    clipped = _clip_overlay_segment(frame, first, last)
    clipped === nothing && return nothing
    clipped_first, clipped_last = clipped
    projected_first = _project_overlay_point(frame, clipped_first)
    projected_last = _project_overlay_point(frame, clipped_last)
    projected_first === nothing && return nothing
    projected_last === nothing && return nothing
    delta_x = (projected_last[1] - projected_first[1]) * 0.5f0 * frame.width_px
    delta_y = (projected_last[2] - projected_first[2]) * 0.5f0 * frame.height_px
    segment_length = sqrt(delta_x * delta_x + delta_y * delta_y)
    normal_x, normal_y = segment_length > 1.0f-6 ?
        (-delta_y / segment_length, delta_x / segment_length) :
        (0.0f0, 1.0f0)
    half_width_first = 0.5f0 * thickness_m * _overlay_pixels_per_meter(frame, clipped_first)
    half_width_last = 0.5f0 * thickness_m * _overlay_pixels_per_meter(frame, clipped_last)
    first_left = _overlay_offset(frame, projected_first, normal_x, normal_y, half_width_first)
    first_right = _overlay_offset(frame, projected_first, -normal_x, -normal_y, half_width_first)
    last_left = _overlay_offset(frame, projected_last, normal_x, normal_y, half_width_last)
    last_right = _overlay_offset(frame, projected_last, -normal_x, -normal_y, half_width_last)
    append!(positions, (first_left, first_right, last_left, last_left, first_right, last_right))
    append!(colors, (color, color, color, color, color, color))
    return nothing
end

function _overlay_line_thickness(radius_m::Float32)::Float32
    return max(0.05f0, 0.08f0 * radius_m)
end

function _append_overlay_disc!(
    positions::Vector{Vec4f},
    colors::Vector{Vec4f},
    frame::CameraFrame,
    center::NTuple{3,Float32},
    radius_m::Float32,
    color::Vec4f,
)
    projected = _project_overlay_point(frame, center)
    projected === nothing && return nothing
    radius_px = max(radius_m * _overlay_pixels_per_meter(frame, center), 1.0f0)
    radius_ndc_x = radius_px / (0.5f0 * frame.width_px)
    radius_ndc_y = radius_px / (0.5f0 * frame.height_px)
    center_vertex = Vec4f(projected[1], projected[2], projected[3], 1.0f0)
    for step in 0:7
        first_angle = Float32(2.0 * pi * step / 8.0)
        last_angle = Float32(2.0 * pi * (step + 1) / 8.0)
        first_vertex = Vec4f(
            projected[1] + radius_ndc_x * cos(first_angle),
            projected[2] + radius_ndc_y * sin(first_angle),
            projected[3],
            1.0f0,
        )
        last_vertex = Vec4f(
            projected[1] + radius_ndc_x * cos(last_angle),
            projected[2] + radius_ndc_y * sin(last_angle),
            projected[3],
            1.0f0,
        )
        append!(positions, (center_vertex, first_vertex, last_vertex))
        append!(colors, (color, color, color))
    end
    return nothing
end

function _overlay_color(overlay::WGEGraphics.OverlayPacket)::Vec4f
    return Vec4f(overlay.color_rgba...)
end

function _append_overlay!(
    positions::Vector{Vec4f},
    colors::Vector{Vec4f},
    overlay::WGEGraphics.PointOverlay,
    frame::CameraFrame,
)
    center = overlay.position_xyz_m
    radius = overlay.radius_m
    color = _overlay_color(overlay)
    # The authored radius is the marker's semantic footprint.  A filled
    # center keeps that footprint visible even when the physical cross arms
    # fall between raster pixel centers at a distant camera.
    _append_overlay_disc!(positions, colors, frame, center, radius, color)
    _append_overlay_segment!(
        positions,
        colors,
        frame,
        (center[1] - radius, center[2], center[3]),
        (center[1] + radius, center[2], center[3]),
        color,
        _overlay_line_thickness(radius),
    )
    _append_overlay_segment!(
        positions,
        colors,
        frame,
        (center[1], center[2], center[3] - radius),
        (center[1], center[2], center[3] + radius),
        color,
        _overlay_line_thickness(radius),
    )
    return nothing
end

function _append_overlay!(
    positions::Vector{Vec4f},
    colors::Vector{Vec4f},
    overlay::WGEGraphics.CircleOverlay,
    frame::CameraFrame,
)
    center = overlay.center_xyz_m
    color = _overlay_color(overlay)
    steps = 16
    for index in 0:(steps-1)
        first_angle = 2.0f0 * Float32(pi) * Float32(index) / Float32(steps)
        last_angle = 2.0f0 * Float32(pi) * Float32(index + 1) / Float32(steps)
        first = (
            center[1] + overlay.radius_m * cos(first_angle),
            center[2],
            center[3] + overlay.radius_m * sin(first_angle),
        )
        last = (
            center[1] + overlay.radius_m * cos(last_angle),
            center[2],
            center[3] + overlay.radius_m * sin(last_angle),
        )
        _append_overlay_segment!(
            positions,
            colors,
            frame,
            first,
            last,
            color,
            _overlay_line_thickness(overlay.radius_m),
        )
    end
    return nothing
end

function _append_overlay!(
    positions::Vector{Vec4f},
    colors::Vector{Vec4f},
    overlay::WGEGraphics.PolylineOverlay,
    frame::CameraFrame,
)
    color = _overlay_color(overlay)
    for index in 1:(length(overlay.points_xyz_m)-1)
        _append_overlay_segment!(
            positions,
            colors,
            frame,
            overlay.points_xyz_m[index],
            overlay.points_xyz_m[index+1],
            color,
            overlay.thickness_m,
        )
    end
    return nothing
end

function _mesh_bound_sphere(mesh::WGEGraphics.MeshPacket)
    minimums = ntuple(index -> minimum(position[index] for position in mesh.positions_m), 3)
    maximums = ntuple(index -> maximum(position[index] for position in mesh.positions_m), 3)
    center = ntuple(index -> 0.5f0 * (minimums[index] + maximums[index]), 3)
    radius = maximum(
        sqrt(sum((position[index] - center[index])^2 for index in 1:3))
        for position in mesh.positions_m
    )
    return center, Float32(radius)
end

function _instance_world_bound(
    packet::WGEGraphics.GraphicsScenePacket,
    instance::WGEGraphics.InstancePacket,
)
    mesh_index = findfirst(mesh -> mesh.mesh_id == instance.mesh_id, packet.meshes)
    mesh_index === nothing &&
        throw(AdapterError("provenance", "instance references an absent mesh"))
    local_center, local_radius = _mesh_bound_sphere(packet.meshes[mesh_index])
    scale = instance.transform.scale_xyz
    rotated_center = _rotate_vector(
        Vec4f(instance.transform.rotation_xyzw...),
        Vec4f(
            local_center[1] * scale[1],
            local_center[2] * scale[2],
            local_center[3] * scale[3],
            0.0f0,
        ),
    )
    world_center = Vec4f(
        rotated_center[1] + instance.transform.translation_xyz_m[1],
        rotated_center[2] + instance.transform.translation_xyz_m[2],
        rotated_center[3] + instance.transform.translation_xyz_m[3],
        0.0f0,
    )
    return world_center, max(scale...) * local_radius
end

function _sphere_visible(frame::CameraFrame, world_center::Vec4f, radius::Float32)::Bool
    relative = Vec4f(
        world_center[1] - frame.position[1],
        world_center[2] - frame.position[2],
        world_center[3] - frame.position[3],
        0.0f0,
    )
    forward_distance = _dot_vector(frame.forward, relative)
    horizontal_distance = _dot_vector(frame.right, relative)
    vertical_distance = _dot_vector(frame.up, relative)
    if frame.mode < 0.5f0
        return forward_distance + radius >= frame.projection[3] &&
               forward_distance - radius <= frame.projection[4] &&
               abs(horizontal_distance) <= frame.projection[1] + radius &&
               abs(vertical_distance) <= frame.projection[2] + radius
    end
    horizontal_plane_radius = radius * sqrt(1.0f0 + frame.projection[1] * frame.projection[1])
    vertical_plane_radius = radius * sqrt(1.0f0 + frame.projection[2] * frame.projection[2])
    return forward_distance + radius >= frame.projection[3] &&
           forward_distance - radius <= frame.projection[4] &&
           horizontal_distance - forward_distance * frame.projection[1] <= horizontal_plane_radius &&
           -horizontal_distance - forward_distance * frame.projection[1] <= horizontal_plane_radius &&
           vertical_distance - forward_distance * frame.projection[2] <= vertical_plane_radius &&
           -vertical_distance - forward_distance * frame.projection[2] <= vertical_plane_radius
end

function _instance_visible(
    frame::CameraFrame,
    packet::WGEGraphics.GraphicsScenePacket,
    instance::WGEGraphics.InstancePacket,
)
    world_center, mesh_radius = _instance_world_bound(packet, instance)
    radius = mesh_radius + _importance_margin(instance.importance)
    return _sphere_visible(frame, world_center, radius)
end

_importance_margin(::WGEGraphics.BackgroundImportance)::Float32 = 0.0f0

_importance_margin(::WGEGraphics.LandmarkImportance)::Float32 = 0.75f0

_importance_margin(::WGEGraphics.GameplayCriticalImportance)::Float32 = 1.5f0

function _mesh_visibility(
    packet::WGEGraphics.GraphicsScenePacket,
    frame::CameraFrame,
)::MeshVisibility
    instance_count = length(packet.instances)
    visible_instance_count = 0
    background_visible_count = 0
    background_culled_count = 0
    landmark_visible_count = 0
    landmark_culled_count = 0
    gameplay_critical_visible_count = 0
    gameplay_critical_culled_count = 0
    for instance in packet.instances
        visible = _instance_visible(frame, packet, instance)
        visible && (visible_instance_count += 1)
        if instance.importance isa WGEGraphics.BackgroundImportance
            visible ? (background_visible_count += 1) : (background_culled_count += 1)
        elseif instance.importance isa WGEGraphics.LandmarkImportance
            visible ? (landmark_visible_count += 1) : (landmark_culled_count += 1)
        elseif instance.importance isa WGEGraphics.GameplayCriticalImportance
            visible ? (gameplay_critical_visible_count += 1) : (gameplay_critical_culled_count += 1)
        else
            throw(AdapterError("unsupported_importance", "instance importance is unsupported"))
        end
    end
    return MeshVisibility(
        instance_count,
        visible_instance_count,
        instance_count - visible_instance_count,
        background_visible_count,
        background_culled_count,
        landmark_visible_count,
        landmark_culled_count,
        gameplay_critical_visible_count,
        gameplay_critical_culled_count,
    )
end

function _mesh_triangle_tangent(
    mesh::WGEGraphics.MeshPacket,
    first_index::Int,
    second_index::Int,
    third_index::Int,
)
    first_position = Vec4f(mesh.positions_m[first_index]..., 0.0f0)
    second_position = Vec4f(mesh.positions_m[second_index]..., 0.0f0)
    third_position = Vec4f(mesh.positions_m[third_index]..., 0.0f0)
    edge_one = Vec4f(
        second_position[1] - first_position[1],
        second_position[2] - first_position[2],
        second_position[3] - first_position[3],
        0.0f0,
    )
    edge_two = Vec4f(
        third_position[1] - first_position[1],
        third_position[2] - first_position[2],
        third_position[3] - first_position[3],
        0.0f0,
    )
    first_uv = mesh.uv0[first_index]
    second_uv = mesh.uv0[second_index]
    third_uv = mesh.uv0[third_index]
    delta_one = (second_uv[1] - first_uv[1], second_uv[2] - first_uv[2])
    delta_two = (third_uv[1] - first_uv[1], third_uv[2] - first_uv[2])
    geometric_normal = _normalize_vector(_cross_vector(edge_one, edge_two))
    determinant = delta_one[1] * delta_two[2] - delta_one[2] * delta_two[1]
    abs(determinant) > 1.0f-8 || return _stable_tangent(geometric_normal)
    inverse_determinant = inv(determinant)
    tangent = _normalize_vector(Vec4f(
        (edge_one[1] * delta_two[2] - edge_two[1] * delta_one[2]) * inverse_determinant,
        (edge_one[2] * delta_two[2] - edge_two[2] * delta_one[2]) * inverse_determinant,
        (edge_one[3] * delta_two[2] - edge_two[3] * delta_one[2]) * inverse_determinant,
        0.0f0,
    ))
    bitangent = Vec4f(
        (edge_two[1] * delta_one[1] - edge_one[1] * delta_two[1]) * inverse_determinant,
        (edge_two[2] * delta_one[1] - edge_one[2] * delta_two[1]) * inverse_determinant,
        (edge_two[3] * delta_one[1] - edge_one[3] * delta_two[1]) * inverse_determinant,
        0.0f0,
    )
    handedness = _dot_vector(_cross_vector(geometric_normal, tangent), bitangent) < 0.0f0 ? -1.0f0 : 1.0f0
    return Vec4f(tangent[1], tangent[2], tangent[3], handedness)
end

function _mesh_resources!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    frame::CameraFrame,
    visibility::MeshVisibility=_mesh_visibility(packet, frame),
    ;
    shadow::Bool=false,
)::Union{Nothing,MeshResources}
    current = shadow ? state.shadow_mesh_resources : state.mesh_resources
    current !== nothing && current.cache_key == packet.content_sha256 && return current
    groups = Dict{Tuple{String,String},Vector{WGEGraphics.InstancePacket}}()
    for instance in packet.instances
        _instance_visible(frame, packet, instance) || continue
        key = (instance.mesh_id, instance.material_id)
        instances = get!(groups, key) do
            WGEGraphics.InstancePacket[]
        end
        push!(instances, instance)
    end
    isempty(groups) && return nothing

    batches = MeshBatchResources[]
    total_vertices = 0
    for (mesh_id, material_id) in sort!(collect(keys(groups)))
        mesh_index = findfirst(mesh -> mesh.mesh_id == mesh_id, packet.meshes)
        mesh_index === nothing && throw(AdapterError("provenance", "instance references an absent mesh"))
        mesh_packet = packet.meshes[mesh_index]
        material = _material(packet, material_id)
        positions = Vec4f[]
        normals = Vec4f[]
        uvs = Vec2f[]
        tangents = Vec4f[]
        for triangle_start in 1:3:length(mesh_packet.indices)
            first_index = Int(mesh_packet.indices[triangle_start]) + 1
            second_index = Int(mesh_packet.indices[triangle_start + 1]) + 1
            third_index = Int(mesh_packet.indices[triangle_start + 2]) + 1
            triangle_tangent = _mesh_triangle_tangent(
                mesh_packet,
                first_index,
                second_index,
                third_index,
            )
            for vertex in (first_index, second_index, third_index)
                push!(positions, Vec4f(mesh_packet.positions_m[vertex]..., 0.0f0))
                push!(normals, Vec4f(mesh_packet.normals[vertex]..., 0.0f0))
                push!(uvs, Vec2f(mesh_packet.uv0[vertex]...))
                push!(
                    tangents,
                    isempty(mesh_packet.tangents) ?
                    triangle_tangent :
                    Vec4f(mesh_packet.tangents[vertex]...),
                )
            end
        end
        batch_instances = groups[(mesh_id, material_id)]
        translations = Vec4f[
            Vec4f(instance.transform.translation_xyz_m..., 0.0f0) for instance in batch_instances
        ]
        rotations = Vec4f[Vec4f(instance.transform.rotation_xyzw...) for instance in batch_instances]
        scales = Vec4f[Vec4f(instance.transform.scale_xyz..., 0.0f0) for instance in batch_instances]
        colors = Vec4f[Vec4f(material.base_color_rgba...) for _ in batch_instances]
        material_parameters = Vec4f[
            Vec4f(material.metallic, material.roughness, material.normal_scale, material.occlusion_strength) for _ in batch_instances
        ]
        surface_parameters = Vec4f[
            Vec4f(
                material.clearcoat,
                material.clearcoat_roughness,
                material.alpha_cutoff === nothing ? 0.0f0 : material.alpha_cutoff,
                material.double_sided ? 1.0f0 : 0.0f0,
            ) for _ in batch_instances
        ]
        emissive_parameters = Vec4f[
            Vec4f(material.emissive_factor_rgb..., 1.0f0) for _ in batch_instances
        ]
        gpu_positions = Lava.LavaArray{Vec4f,1}(positions; bq=state.queue)
        gpu_normals = Lava.LavaArray{Vec4f,1}(normals; bq=state.queue)
        gpu_uvs = Lava.LavaArray{Vec2f,1}(uvs; bq=state.queue)
        gpu_tangents = Lava.LavaArray{Vec4f,1}(tangents; bq=state.queue)
        gpu_translations = Lava.LavaArray{Vec4f,1}(translations; bq=state.queue)
        gpu_rotations = Lava.LavaArray{Vec4f,1}(rotations; bq=state.queue)
        gpu_scales = Lava.LavaArray{Vec4f,1}(scales; bq=state.queue)
        gpu_colors = Lava.LavaArray{Vec4f,1}(colors; bq=state.queue)
        gpu_material_parameters = Lava.LavaArray{Vec4f,1}(material_parameters; bq=state.queue)
        gpu_surface_parameters = Lava.LavaArray{Vec4f,1}(surface_parameters; bq=state.queue)
        gpu_emissive_parameters = Lava.LavaArray{Vec4f,1}(emissive_parameters; bq=state.queue)
        state.upload_bytes += UInt64(
            sizeof(Vec4f) *
                (length(positions) + length(normals) + length(tangents) + length(translations) +
                length(rotations) + length(scales) + length(colors) +
                length(material_parameters) + length(surface_parameters) +
                length(emissive_parameters)),
        )
        state.upload_bytes += UInt64(sizeof(Vec2f) * length(uvs))
        push!(
            batches,
            MeshBatchResources(
                material.material_id,
                gpu_positions,
                gpu_normals,
                gpu_uvs,
                gpu_tangents,
                gpu_translations,
                gpu_rotations,
                gpu_scales,
                gpu_colors,
                gpu_material_parameters,
                gpu_surface_parameters,
                gpu_emissive_parameters,
                length(positions),
                length(batch_instances),
            ),
        )
        total_vertices += length(positions)
    end
    created = MeshResources(
        packet.content_sha256,
        batches,
        visibility.instance_count,
        visibility.visible_instance_count,
        visibility.culled_instance_count,
        total_vertices,
    )
    if shadow
        state.shadow_mesh_resources = created
    else
        state.mesh_resources = created
    end
    return created
end

const SHADOW_MAP_SIZE = 512

function _render_shadow_map!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    shadow::ShadowResources,
    terrain::TerrainResources,
    mesh_resources::Union{Nothing,MeshResources},
)
    target = OffscreenTarget(shadow.framebuffer)
    terrain_vertices = 6 * (packet.terrain.resolution - 1)^2
    frame = shadow.light_frame
    draw!(
        state.queue,
        state.terrain_shadow_pipeline,
        target,
        terrain_vertices;
        args=(
            terrain.heights,
            terrain.resolution,
            Float32(packet.terrain.width_m),
            Float32(packet.terrain.length_m),
            frame.position,
            frame.right,
            frame.up,
            frame.forward,
            frame.projection,
            frame.mode,
        ),
        clear_color=(1.0f0, 1.0f0, 1.0f0, 1.0f0),
    )
    state.draw_calls += 1
    if !state.terrain_shadow_compiled
        state.terrain_shadow_compiled = true
        state.pipeline_compilations += 1
    end
    if mesh_resources !== nothing
        for batch in mesh_resources.batches
            material = _material(packet, batch.material_id)
            if material.alpha_mode == :mask
                _draw_cutout_shadow!(state, packet, target, frame, batch, material)
                continue
            end
            draw!(
                state.queue,
                state.mesh_shadow_pipeline,
                target,
                batch.vertex_count;
                args=(
                    batch.positions,
                    batch.translations,
                    batch.rotations,
                    batch.scales,
                    frame.position,
                    frame.right,
                    frame.up,
                    frame.forward,
                    frame.projection,
                    frame.mode,
                ),
                instances=batch.instance_count,
                clear_color=nothing,
                depth_clear=nothing,
            )
            state.draw_calls += 1
            if !state.mesh_shadow_compiled
                state.mesh_shadow_compiled = true
                state.pipeline_compilations += 1
            end
        end
    end
    _transition_color_to_sampled!(state, shadow.framebuffer)
    return nothing
end

function _cutout_shadow_bindings!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    material::WGEGraphics.MaterialPacket,
)::Lava.TextureBindings
    cache = state.cutout_shadow_bindings
    any(key -> first(key) != packet.content_sha256, keys(cache)) && empty!(cache)
    return get!(cache, (packet.content_sha256, material.material_id)) do
        textures = _material_textures!(state, packet, material)
        sampler = _surface_sampler!(state, _surface_sampler_spec(packet.render_policy, :mesh), textures.max_sampler_lod)
        Lava.bind_textures([textures.albedo_texture * sampler])
    end
end

function _draw_cutout_shadow!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    target,
    frame::CameraFrame,
    batch::MeshBatchResources,
    material::WGEGraphics.MaterialPacket,
)
    bindings = _cutout_shadow_bindings!(state, packet, material)
    draw!(
        state.queue,
        state.mesh_cutout_shadow_pipeline,
        target,
        batch.vertex_count;
        args=(
            batch.positions,
            batch.uvs,
            batch.translations,
            batch.rotations,
            batch.scales,
            frame.position,
            frame.right,
            frame.up,
            frame.forward,
            frame.projection,
            frame.mode,
            Vec4f(material.base_color_rgba[4], _material_texture_enabled(material), material.alpha_cutoff, 0.0f0),
        ),
        instances=batch.instance_count,
        descriptor_set_layout=bindings.layout,
        descriptor_set=bindings.set,
        clear_color=nothing,
        depth_clear=nothing,
    )
    state.draw_calls += 1
    if !state.mesh_cutout_shadow_compiled
        state.mesh_cutout_shadow_compiled = true
        state.pipeline_compilations += 1
    end
    return nothing
end

function _shadow_resources!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    lighting::DirectionalLighting,
)::ShadowResources
    current = state.shadow_resources
    current !== nothing && current.cache_key == packet.content_sha256 && return current
    light_frame = _shadow_frame(packet, lighting)
    framebuffer = _framebuffer!(state, SHADOW_MAP_SIZE, SHADOW_MAP_SIZE, true, :shadow)
    texture = _framebuffer_texture(framebuffer, state)
    # Linear depth filtering softens the bounded PCF taps without changing
    # the typed shadow contract. Nearest filtering made hero-scale shadows
    # visibly stair-stepped in the native showcase.
    sampler = Lava.LavaSampler(ctx=state.context, filter=:linear, wrap=:clamp)
    created = ShadowResources(packet.content_sha256, framebuffer, texture, sampler, light_frame)
    terrain = _terrain_resources!(state, packet)
    visibility = _mesh_visibility(packet, light_frame)
    mesh_resources = _mesh_resources!(state, packet, light_frame, visibility; shadow=true)
    _render_shadow_map!(state, packet, created, terrain, mesh_resources)
    state.shadow_resources = created
    return created
end

function _overlay_resources!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    frame::CameraFrame,
)
    current = state.overlay_resources
    current !== nothing && current.cache_key == packet.content_sha256 && return current
    positions = Vec4f[]
    colors = Vec4f[]
    for overlay in packet.overlays
        _append_overlay!(positions, colors, overlay, frame)
    end
    isempty(positions) && return nothing
    gpu_positions = Lava.LavaArray{Vec4f,1}(positions; bq=state.queue)
    gpu_colors = Lava.LavaArray{Vec4f,1}(colors; bq=state.queue)
    state.upload_bytes += UInt64(sizeof(Vec4f) * (length(positions) + length(colors)))
    created = OverlayResources(packet.content_sha256, gpu_positions, gpu_colors, length(positions))
    state.overlay_resources = created
    return created
end

function _pixel_luminance(pixel::NTuple{4,<:Real})::Float64
    return 0.2126 * Float64(pixel[1]) + 0.7152 * Float64(pixel[2]) + 0.0722 * Float64(pixel[3])
end

function _pixel_luminance(pixel::NTuple{4,UInt8})::Float64
    return (
        0.2126 * Float64(pixel[1]) +
            0.7152 * Float64(pixel[2]) +
            0.0722 * Float64(pixel[3])
    ) / 255.0
end

function _capture_pixels(capture_bytes::Vector{UInt8})::Vector{NTuple{4,UInt8}}
    length(capture_bytes) % 4 == 0 ||
        throw(AdapterError("invalid_capture", "capture byte length is not RGBA-aligned"))
    pixels = Vector{NTuple{4,UInt8}}(undef, length(capture_bytes) ÷ 4)
    for (pixel_index, offset) in enumerate(1:4:length(capture_bytes))
        pixels[pixel_index] = ntuple(index -> capture_bytes[offset+index-1], 4)
    end
    return pixels
end

function _measurement_screen_projection(
    frame::CameraFrame,
    camera::WGEGraphics.CameraPacket,
    position::NTuple{3,Float32},
)::Union{Nothing,NTuple{3,Float32}}
    projected = _project_overlay_point(frame, position)
    projected === nothing && return nothing
    relative = Vec4f(
        position[1] - frame.position[1],
        position[2] - frame.position[2],
        position[3] - frame.position[3],
        0.0f0,
    )
    forward_distance = _dot_vector(frame.forward, relative)
    pixels_per_meter = frame.mode < 0.5f0 ?
        Float32(camera.height_px) / (2.0f0 * frame.projection[2]) :
        Float32(camera.height_px) / (2.0f0 * forward_distance * frame.projection[2])
    return (
        (projected[1] * 0.5f0 + 0.5f0) * Float32(camera.width_px),
        (projected[2] * 0.5f0 + 0.5f0) * Float32(camera.height_px),
        pixels_per_meter,
    )
end

function _measurement_overlay_samples(
    packet::WGEGraphics.GraphicsScenePacket,
    frame::CameraFrame,
    overlay::WGEGraphics.PointOverlay,
)
    camera = packet.camera
    radius = Float32(overlay.radius_m)
    center = overlay.position_xyz_m
    projection = _measurement_screen_projection(frame, camera, center)
    sample = projection === nothing ? nothing : (
        projection[1],
        projection[2],
        max(radius * projection[3] + 3.0f0, 4.0f0),
    )
    sample === nothing ? NTuple{3,Float32}[] : [sample]
end

function _measurement_overlay_samples(
    packet::WGEGraphics.GraphicsScenePacket,
    frame::CameraFrame,
    overlay::WGEGraphics.CircleOverlay,
)
    camera = packet.camera
    samples = NTuple{3,Float32}[]
    center = overlay.center_xyz_m
    for step in 0:15
        angle = 2.0f0 * Float32(pi) * Float32(step) / 16.0f0
        position = (
            center[1] + overlay.radius_m * cos(angle),
            center[2],
            center[3] + overlay.radius_m * sin(angle),
        )
        projection = _measurement_screen_projection(frame, camera, position)
        sample = projection === nothing ? nothing : (
            projection[1],
            projection[2],
            max(4.0f0 + projection[3] * 0.08f0, 4.0f0),
        )
        sample === nothing || push!(samples, sample)
    end
    return samples
end

function _measurement_overlay_samples(
    packet::WGEGraphics.GraphicsScenePacket,
    frame::CameraFrame,
    overlay::WGEGraphics.PolylineOverlay,
)
    camera = packet.camera
    samples = NTuple{3,Float32}[]
    for index in 1:(length(overlay.points_xyz_m)-1)
        clipped = _clip_overlay_segment(
            frame,
            overlay.points_xyz_m[index],
            overlay.points_xyz_m[index + 1],
        )
        clipped === nothing && continue
        first, last = clipped
        for step in 0:8
            amount = Float32(step) / 8.0f0
            position = (
                first[1] + (last[1] - first[1]) * amount,
                first[2] + (last[2] - first[2]) * amount,
                first[3] + (last[3] - first[3]) * amount,
            )
            projection = _measurement_screen_projection(frame, camera, position)
            sample = projection === nothing ? nothing : (
                projection[1],
                projection[2],
                max(overlay.thickness_m * projection[3] + 3.0f0, 4.0f0),
            )
            sample === nothing || push!(samples, sample)
        end
    end
    return samples
end

function _scene_measurements(
    pixels::AbstractVector{<:NTuple{4,<:Real}},
    packet::WGEGraphics.GraphicsScenePacket,
)
    bytes = Vector{UInt8}(undef, 4 * length(pixels))
    offset = 1
    for pixel in pixels
        rgba = _rgba8(pixel)
        for component in 1:4
            bytes[offset + component - 1] = rgba[component]
        end
        offset += 4
    end
    return _scene_measurements(bytes, packet)
end

function _scene_measurements(
    capture_bytes::Vector{UInt8},
    packet::WGEGraphics.GraphicsScenePacket,
    frame::CameraFrame=_camera_frame(packet.camera),
)
    length(capture_bytes) % 4 == 0 ||
        throw(AdapterError("invalid_capture", "capture byte length is not RGBA-aligned"))
    role_colors = ntuple(_ -> Set{NTuple{4,UInt8}}(), 5)
    role_samples = ntuple(_ -> NTuple{3,Float32}[], 5)
    for overlay in packet.overlays
        role_index = _measurement_role_index(overlay.role)
        push!(role_colors[role_index], _rgba8(overlay.color_rgba))
        append!(role_samples[role_index], _measurement_overlay_samples(packet, frame, overlay))
    end
    distinct_colors = Set{NTuple{3,UInt8}}()
    role_pixels = zeros(Int, 5)
    mean = 0.0
    sum_squared_delta = 0.0
    sample_count = 0
    width = Int(packet.width_px)
    for (pixel_index, offset) in enumerate(1:4:length(capture_bytes))
        rgba = (
            capture_bytes[offset],
            capture_bytes[offset + 1],
            capture_bytes[offset + 2],
            capture_bytes[offset + 3],
        )
        push!(distinct_colors, (rgba[1], rgba[2], rgba[3]))
        luminance = _pixel_luminance(rgba)
        sample_count += 1
        delta = luminance - mean
        mean += delta / sample_count
        sum_squared_delta += delta * (luminance - mean)
        for role_index in eachindex(role_colors)
            if rgba in role_colors[role_index]
                pixel_x = Float32((pixel_index - 1) % width) + 0.5f0
                pixel_y = Float32((pixel_index - 1) ÷ width) + 0.5f0
                any(
                    sample ->
                        (pixel_x - sample[1])^2 + (pixel_y - sample[2])^2 <= sample[3]^2,
                    role_samples[role_index],
                ) && (role_pixels[role_index] += 1)
            end
        end
    end
    return (
        terrain_luminance_stddev=sqrt(sum_squared_delta / sample_count),
        distinct_terrain_colors=length(distinct_colors),
        route_visible_pixels=role_pixels[1],
        player_spawn_visible_pixels=role_pixels[2],
        opponent_spawn_visible_pixels=role_pixels[3],
        encounter_visible_pixels=role_pixels[4],
        objective_visible_pixels=role_pixels[5],
    )
end

function _measurement_role_index(role::Symbol)::Int
    role === :route && return 1
    role === :player_spawn && return 2
    role === :opponent_spawn && return 3
    role === :encounter && return 4
    role === :objective && return 5
    throw(AdapterError("unsupported_overlay", "overlay role $role is unsupported"))
end

function render_scene(
    packet::WGEGraphics.GraphicsScenePacket,
    state::LavaBackend=backend(),
)
    gpu_timing_slot = _begin_gpu_frame_timing(state)
    try
        return _render_scene(packet, state, gpu_timing_slot)
    finally
        gpu_timing_slot === nothing || Lava.enable_dispatch_timing!(false, state.context)
    end
end

"""
Record the camera-dependent scene passes (sky, terrain, mesh instances) into an
HDR scene framebuffer. Shared verbatim by the offscreen capture path and the
presented window path so the two paths cannot drift.
"""
function _record_scene_passes!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    camera_frame::CameraFrame,
    lighting::DirectionalLighting,
    environment_lighting::EnvironmentLighting,
    material::WGEGraphics.MaterialPacket,
    texture_enabled::Float32,
    scene_framebuffer::Lava.LavaFramebuffer,
    scene_target::Lava.OffscreenTarget,
)
    prepare_started_ns = time_ns()
    prepare_gpu_timing_slot = _begin_gpu_pass_timing(state, GPU_PASS_TIMING_LABELS.prepare)
    resources = _terrain_resources!(state, packet)
    shadow_resources = _shadow_resources!(state, packet, lighting)
    # Refuse an unhonourable policy axis before any GPU work, so the failure is
    # a clean typed error rather than a frame that quietly ignored its policy.
    _assert_render_policy_supported(
        packet.render_policy,
        _probe_gpu_capabilities(state.context);
        layered_terrain=packet.terrain.layers !== nothing,
    )
    shadow_uniform = _shadow_uniform(packet.render_policy)
    sky_parameters = _sky_parameters(packet.render_policy, lighting, packet.environment)
    atmosphere_parameters = _atmosphere_parameters(packet.render_policy)
    surface_options = _surface_options(packet.render_policy)
    texture_resources = _material_texture_resources!(
        state,
        packet,
        shadow_resources,
        material,
        _surface_sampler_spec(packet.render_policy, :terrain),
    )
    visibility = _mesh_visibility(packet, camera_frame)
    mesh_resources = _mesh_resources!(state, packet, camera_frame, visibility)
    _end_gpu_pass_timing!(state, prepare_gpu_timing_slot)
    prepare_time_us = _elapsed_us(prepare_started_ns)
    terrain_vertices = 6 * (packet.terrain.resolution - 1)^2

    scene_raster_started_ns = time_ns()
    scene_raster_gpu_timing_slot = _begin_gpu_pass_timing(state, GPU_PASS_TIMING_LABELS.scene_raster)
    sky_policy = packet.render_policy.sky
    if sky_policy === nothing
        draw!(
            state.queue,
            state.sky_pipeline,
            scene_target,
            3;
            args=(
                Vec4f(packet.environment.sky_top_rgb..., 1.0f0),
                Vec4f(packet.environment.sky_horizon_rgb..., 1.0f0),
                environment_lighting.ground,
                Vec4f(packet.environment.fog_color_rgb..., 1.0f0),
            ),
            clear_color=(0.02f0, 0.03f0, 0.05f0, 1.0f0),
        )
        state.draw_calls += 1
        if !state.sky_compiled
            state.sky_compiled = true
            state.pipeline_compilations += 1
        end
    else
        draw!(
            state.queue,
            state.sky_view_pipeline,
            scene_target,
            3;
            args=_sky_view_arguments(camera_frame, packet, lighting, sky_policy),
            clear_color=(0.02f0, 0.03f0, 0.05f0, 1.0f0),
        )
        state.draw_calls += 1
        if !state.sky_view_compiled
            state.sky_view_compiled = true
            state.pipeline_compilations += 1
        end
    end
    terrain_arguments = (
            resources.heights,
            resources.slopes,
            resources.regions,
            resources.resolution,
            Float32(packet.terrain.width_m),
            Float32(packet.terrain.length_m),
            camera_frame.position,
            camera_frame.right,
            camera_frame.up,
            camera_frame.forward,
            camera_frame.projection,
            camera_frame.mode,
            shadow_resources.light_frame.position,
            shadow_resources.light_frame.right,
            shadow_resources.light_frame.up,
            shadow_resources.light_frame.forward,
            shadow_resources.light_frame.projection,
            shadow_resources.light_frame.mode,
            Vec4f(material.base_color_rgba...),
            material.metallic,
            material.roughness,
            material.clearcoat,
            material.clearcoat_roughness,
            material.normal_scale,
            material.occlusion_strength,
            lighting.direction,
            lighting.color,
            Float32(lighting.intensity),
            environment_lighting.sky_top,
            environment_lighting.sky_horizon,
            environment_lighting.ground,
            Vec4f(packet.environment.fog_color_rgb..., 1.0f0),
            packet.environment.fog_density,
            packet.environment.exposure,
            texture_enabled,
            Vec4f(material.emissive_factor_rgb..., 1.0f0),
            shadow_uniform,
            Float32(packet.render_policy.terrain_surface.uv_repeat_scale_milli) / 1000.0f0,
            sky_parameters,
            atmosphere_parameters,
            surface_options,
    )
    if packet.terrain.layers === nothing
        draw!(
            state.queue,
            state.terrain_pipeline,
            scene_target,
            terrain_vertices;
            args=terrain_arguments,
            descriptor_set_layout=texture_resources.bindings.layout,
            descriptor_set=texture_resources.bindings.set,
            clear_color=nothing,
        )
        state.draw_calls += 1
        if !state.terrain_compiled
            state.terrain_compiled = true
            state.pipeline_compilations += 1
        end
    else
        layer_resources = _terrain_layer_resources!(state, packet, shadow_resources)
        draw!(
            state.queue,
            state.terrain_layered_pipeline,
            scene_target,
            terrain_vertices;
            args=(terrain_arguments..., layer_resources.uniforms...),
            descriptor_set_layout=layer_resources.bindings.layout,
            descriptor_set=layer_resources.bindings.set,
            clear_color=nothing,
        )
        state.draw_calls += 1
        if !state.terrain_layered_compiled
            state.terrain_layered_compiled = true
            state.pipeline_compilations += 1
        end
    end
    if mesh_resources !== nothing
        for batch in mesh_resources.batches
            batch_material = _material(packet, batch.material_id)
            batch_texture_resources = _material_texture_resources!(
                state,
                packet,
                shadow_resources,
                batch_material,
                _surface_sampler_spec(packet.render_policy, :mesh),
            )
            batch_texture_enabled = _material_texture_enabled(batch_material)
            cutout = _material_cutout(batch_material)
            draw!(
                state.queue,
                cutout ? state.mesh_cutout_pipeline : state.mesh_pipeline,
                scene_target,
                batch.vertex_count;
                args=(
                    batch.positions,
                    batch.normals,
                    batch.uvs,
                    batch.tangents,
                    batch.translations,
                    batch.rotations,
                    batch.scales,
                    batch.colors,
                    batch.material_parameters,
                    batch.surface_parameters,
                    batch.emissive_parameters,
                    camera_frame.position,
                    camera_frame.right,
                    camera_frame.up,
                    camera_frame.forward,
                    camera_frame.projection,
                    camera_frame.mode,
                    shadow_resources.light_frame.position,
                    shadow_resources.light_frame.right,
                    shadow_resources.light_frame.up,
                    shadow_resources.light_frame.forward,
                    shadow_resources.light_frame.projection,
                    shadow_resources.light_frame.mode,
                    lighting.direction,
                    lighting.color,
                    Float32(lighting.intensity),
                    environment_lighting.sky_top,
                    environment_lighting.sky_horizon,
                    environment_lighting.ground,
                    Vec4f(packet.environment.fog_color_rgb..., 1.0f0),
                    packet.environment.fog_density,
                    packet.environment.exposure,
                    batch_texture_enabled,
                    shadow_uniform,
                    sky_parameters,
                    atmosphere_parameters,
                    surface_options,
                ),
                instances=batch.instance_count,
                descriptor_set_layout=batch_texture_resources.bindings.layout,
                descriptor_set=batch_texture_resources.bindings.set,
                clear_color=nothing,
                depth_clear=nothing,
            )
            state.draw_calls += 1
            if cutout && !state.mesh_cutout_compiled
                state.mesh_cutout_compiled = true
                state.pipeline_compilations += 1
            elseif !cutout && !state.mesh_compiled
                state.mesh_compiled = true
                state.pipeline_compilations += 1
            end
        end
    end
    _transition_color_to_sampled!(state, scene_framebuffer)
    _end_gpu_pass_timing!(state, scene_raster_gpu_timing_slot)
    scene_raster_time_us = _elapsed_us(scene_raster_started_ns)
    return (
        terrain_vertices=terrain_vertices,
        mesh_vertex_count=mesh_resources === nothing ? 0 : mesh_resources.vertex_count,
        visibility=visibility,
        prepare_time_us=prepare_time_us,
        scene_raster_time_us=scene_raster_time_us,
    )
end

"""
Shared composite tail for both render paths: resolve the supersampled HDR scene
framebuffer into the capture framebuffer, then draw the overlay on top. The
presented-window path records exactly these draws, so what the window presents
is the certified composite by construction rather than a second approximation
of it.
"""
function _composite_capture!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    scene_framebuffer::Lava.LavaFramebuffer,
    overlay_resources::Union{Nothing,OverlayResources},
    width::Int,
    height::Int,
)
    render_width = 2 * width
    render_height = 2 * height

    resolve_started_ns = time_ns()
    resolve_gpu_timing_slot = _begin_gpu_pass_timing(state, GPU_PASS_TIMING_LABELS.resolve)
    capture_framebuffer = _framebuffer!(state, width, height, false, :capture)
    capture_target = OffscreenTarget(capture_framebuffer)
    resolve_resources = _resolve_resources!(state, scene_framebuffer)
    resolve_policy = _resolve_policy(packet.render_policy)
    draw!(
        state.queue,
        state.resolve_pipeline,
        capture_target,
        3;
        args=(
            Vec2f(1.0f0 / Float32(render_width), 1.0f0 / Float32(render_height)),
            packet.environment.exposure,
            resolve_policy.grade_lift_rgb,
            resolve_policy.grade_gain_rgb,
            resolve_policy.grade_gamma_saturation,
            resolve_policy.bloom,
            resolve_policy.vignette,
            resolve_policy.dither,
        ),
        descriptor_set_layout=resolve_resources.bindings.layout,
        descriptor_set=resolve_resources.bindings.set,
        clear_color=(0.0f0, 0.0f0, 0.0f0, 1.0f0),
    )
    state.draw_calls += 1
    if !state.resolve_compiled
        state.resolve_compiled = true
        state.pipeline_compilations += 1
    end
    _end_gpu_pass_timing!(state, resolve_gpu_timing_slot)
    resolve_time_us = _elapsed_us(resolve_started_ns)

    overlay_started_ns = time_ns()
    overlay_gpu_timing_slot = overlay_resources === nothing ?
        nothing : _begin_gpu_pass_timing(state, GPU_PASS_TIMING_LABELS.overlay)
    if overlay_resources !== nothing
        draw!(
            state.queue,
            state.overlay_pipeline,
            capture_target,
            overlay_resources.vertex_count;
            args=(overlay_resources.positions, overlay_resources.colors),
            clear_color=nothing,
        )
        state.draw_calls += 1
        if !state.overlay_compiled
            state.overlay_compiled = true
            state.pipeline_compilations += 1
        end
    end
    _end_gpu_pass_timing!(state, overlay_gpu_timing_slot)
    overlay_time_us = _elapsed_us(overlay_started_ns)

    return (
        capture_framebuffer=capture_framebuffer,
        resolve_time_us=resolve_time_us,
        overlay_time_us=overlay_time_us,
    )
end

function _render_scene(
    packet::WGEGraphics.GraphicsScenePacket,
    state::LavaBackend,
    gpu_timing_slot::Union{Nothing,Int},
)
    started_ns = time_ns()
    upload_bytes_start = state.upload_bytes
    draw_calls_start = state.draw_calls
    readback_bytes_start = state.readback_bytes
    pipeline_compilations_start = state.pipeline_compilations
    width, height = _validate_dimensions(packet.width_px, packet.height_px)
    material = _terrain_material(packet)
    for material_intent in packet.materials
        _validate_material(material_intent)
    end
    environment_lighting = _environment_lighting(packet.environment)
    texture_enabled = _material_texture_enabled(material)
    camera_frame = _camera_frame(packet.camera)
    lighting = _lighting(packet)

    render_width = 2 * width
    render_height = 2 * height
    scene_framebuffer = _framebuffer!(state, render_width, render_height, true, :scene_hdr)
    scene_target = OffscreenTarget(scene_framebuffer)
    scene = _record_scene_passes!(
        state,
        packet,
        camera_frame,
        lighting,
        environment_lighting,
        material,
        texture_enabled,
        scene_framebuffer,
        scene_target,
    )

    overlay_resources = _overlay_resources!(state, packet, camera_frame)
    composite = _composite_capture!(
        state,
        packet,
        scene_framebuffer,
        overlay_resources,
        width,
        height,
    )
    capture_framebuffer = composite.capture_framebuffer
    prepare_time_us = scene.prepare_time_us
    scene_raster_time_us = scene.scene_raster_time_us
    resolve_time_us = composite.resolve_time_us
    overlay_time_us = composite.overlay_time_us

    flush_readback_started_ns = time_ns()
    _end_gpu_frame_timing!(state, gpu_timing_slot)
    pixels = readback_framebuffer(capture_framebuffer)
    gpu_timing_reports = _read_gpu_timing_reports(state, gpu_timing_slot)
    gpu_frame_time_us = _timing_report_us(gpu_timing_reports, GPU_FRAME_TIMING_LABEL)
    gpu_pass_timings = (
        prepare_us=_timing_report_us(
            gpu_timing_reports,
            GPU_PASS_TIMING_LABELS.prepare,
        ),
        scene_raster_us=_timing_report_us(
            gpu_timing_reports,
            GPU_PASS_TIMING_LABELS.scene_raster,
        ),
        resolve_us=_timing_report_us(
            gpu_timing_reports,
            GPU_PASS_TIMING_LABELS.resolve,
        ),
        overlay_us=_timing_report_us(
            gpu_timing_reports,
            GPU_PASS_TIMING_LABELS.overlay,
        ),
    )
    flush_readback_time_us = _elapsed_us(flush_readback_started_ns)
    capture_bytes = _capture_bytes(
        pixels,
        _resolve_policy(packet.render_policy).dither[1],
    )
    state.readback_bytes += length(capture_bytes)
    return (
        schema="wge.lava-frame/v1",
        backend_id="lava-vulkan",
        adapter_revision=ADAPTER_REVISION,
        lava_revision=LAVA_REVISION,
        packet_sha256=packet.packet_sha256,
        capture_id=packet.capture_id,
        width_px=width,
        height_px=height,
        capture_sha256="sha256:" * bytes2hex(sha256(capture_bytes)),
        capture_base64=base64encode(capture_bytes),
        measurements=_scene_measurements(capture_bytes, packet, camera_frame),
        telemetry=(
            upload_bytes=Int(state.upload_bytes - upload_bytes_start),
            readback_bytes=Int(state.readback_bytes - readback_bytes_start),
            draw_calls=Int(state.draw_calls - draw_calls_start),
            dispatch_calls=0,
            pipeline_compilations=Int(state.pipeline_compilations - pipeline_compilations_start),
            instance_count=scene.visibility.instance_count,
            visible_instance_count=scene.visibility.visible_instance_count,
            culled_instance_count=scene.visibility.culled_instance_count,
            background_visible_instance_count=scene.visibility.background_visible_count,
            background_culled_instance_count=scene.visibility.background_culled_count,
            landmark_visible_instance_count=scene.visibility.landmark_visible_count,
            landmark_culled_instance_count=scene.visibility.landmark_culled_count,
            gameplay_critical_visible_instance_count=scene.visibility.gameplay_critical_visible_count,
            gameplay_critical_culled_instance_count=scene.visibility.gameplay_critical_culled_count,
            terrain_vertex_count=scene.terrain_vertices,
            mesh_vertex_count=scene.mesh_vertex_count,
            texture_residency=_packet_residency_summary(packet),
            frame_time_us=_elapsed_us(started_ns),
            gpu_frame_time_us=gpu_frame_time_us,
            pass_timings=(
                prepare_us=prepare_time_us,
                scene_raster_us=scene_raster_time_us,
                resolve_us=resolve_time_us,
                overlay_us=overlay_time_us,
                flush_readback_us=flush_readback_time_us,
                gpu_prepare_us=gpu_pass_timings.prepare_us,
                gpu_scene_raster_us=gpu_pass_timings.scene_raster_us,
                gpu_resolve_us=gpu_pass_timings.resolve_us,
                gpu_overlay_us=gpu_pass_timings.overlay_us,
            ),
        ),
    )
end

"""
A persistent presented-session window. The GLFW window, Vulkan surface, and
swapchain are owned by Lava's pinned revision; the adapter owns the session
lifetime and never destroys swapchain resources manually. Rendering reuses the
same scene passes as the offscreen path; only the final resolve target and the
present differ.
"""
mutable struct WindowSession
    window::Lava.RenderWindow
    presented_frames::UInt64
    # Window-sized RGBA buffer holding the certified composite for the present
    # blit. Allocated lazily at the camera extent so the blit source and the
    # window are always the same size.
    present_pixels::Union{Nothing,Lava.LavaArray{Vec4f,1}}
    present_width::Int
    present_height::Int
end

const WINDOW_SESSION_REF = Ref{Union{Nothing,WindowSession}}(nothing)

"""Sample raw HID state for the presented window: held keys (plus the
reserved negative mouse-button codes), normalized cursor position while
focused, and joystick-1 axes/buttons. Device-raw by contract — semantic
meaning (sprint, guard, look) is applied only by Rust's input-session layer,
so the worker never derives gameplay state."""
function _sample_window_input(window::Lava.RenderWindow)
    glfw_window = window.handle
    keys = Int[]
    cursor = nothing
    if GLFW.GetWindowAttrib(glfw_window, GLFW.FOCUSED)
        for key in (
            GLFW.KEY_W,
            GLFW.KEY_A,
            GLFW.KEY_S,
            GLFW.KEY_D,
            GLFW.KEY_LEFT_SHIFT,
            GLFW.KEY_SPACE,
            GLFW.KEY_E,
            GLFW.KEY_LEFT_CONTROL,
        )
            GLFW.GetKey(glfw_window, key) && push!(keys, Int(key))
        end
        GLFW.GetMouseButton(glfw_window, GLFW.MOUSE_BUTTON_LEFT) && push!(keys, -1)
        GLFW.GetMouseButton(glfw_window, GLFW.MOUSE_BUTTON_RIGHT) && push!(keys, -2)
        win_width, win_height = GLFW.GetWindowSize(glfw_window)
        if win_width > 0 && win_height > 0
            x, y = GLFW.GetCursorPos(glfw_window)
            cursor = [Float64(x / win_width), Float64(y / win_height)]
        end
    end
    axes_vec = Float32[]
    buttons_vec = Bool[]
    if GLFW.JoystickPresent(GLFW.Joystick(0))
        stick_axes = GLFW.GetJoystickAxes(GLFW.Joystick(0))
        stick_axes === nothing || append!(axes_vec, Float32.(collect(stick_axes)))
        stick_buttons = GLFW.GetJoystickButtons(GLFW.Joystick(0))
        stick_buttons === nothing || append!(buttons_vec, Bool.(collect(stick_buttons)))
    end
    return (
        schema_version="wge.input-sample/v1",
        timestamp_ms=Float64(time_ns() / 1e6),
        held_keys=keys,
        cursor=cursor,
        gamepad_axes=axes_vec,
        gamepad_buttons=buttons_vec,
    )
end

"""Open the persistent presented-session window on the shared device context."""
function open_window_session(
    state::LavaBackend,
    width_px::Integer,
    height_px::Integer;
    vsync::Bool=true,
)
    WINDOW_SESSION_REF[] === nothing ||
        throw(AdapterError("window_open_conflict", "a window session is already open"))
    width, height = _validate_dimensions(width_px, height_px)
    window = Lava.RenderWindow(
        width,
        height;
        title="WGE native session",
        vsync=vsync,
        ctx=state.context,
    )
    session = WindowSession(window, UInt64(0), nothing, 0, 0)
    WINDOW_SESSION_REF[] = session
    win_width, win_height = size(window)
    return (
        window_width_px=Int(win_width),
        window_height_px=Int(win_height),
        vsync=vsync,
    )
end

"""Close the persistent window session. Closing an already-closed session is
reported honestly rather than treated as a fault."""
function close_window_session!(state::LavaBackend)
    session = WINDOW_SESSION_REF[]
    session === nothing && return (closed=false, presented_frames=UInt64(0))
    WINDOW_SESSION_REF[] = nothing
    Lava.close(session.window)
    return (closed=true, presented_frames=session.presented_frames)
end

function _window_session(state::LavaBackend)::WindowSession
    session = WINDOW_SESSION_REF[]
    session === nothing &&
        throw(AdapterError("window_not_open", "no window session is open"))
    return session
end

function _window_target(
    session::WindowSession,
    width::Int,
    height::Int,
)::Lava.WindowTarget
    win_width, win_height = size(session.window)
    (win_width, win_height) == (width, height) || throw(AdapterError(
        "window_extent_mismatch",
        "camera extent ($width, $height) does not match the window ($win_width, $win_height)",
    ))
    return Lava.WindowTarget(session.window)
end

"""Convert a swapchain readback (BGRA8, sRGB-encoded) into contract RGBA8 row
order, mirroring the offscreen capture's row orientation. Reserved for the
C3.4 presented-frame evidence slice; Lava's mid-frame readback flushes the
active batch, so a presented-frame capture cannot share the frame's batch."""
function _window_capture_bytes(pixels::Matrix{NTuple{4,UInt8}})::Vector{UInt8}
    bytes = Vector{UInt8}(undef, 4 * length(pixels))
    offset = 0
    # Lava readback is indexed as (x, y); the contract stores rows top-to-bottom.
    for row in axes(pixels, 2), column in axes(pixels, 1)
        bgra = pixels[column, row]
        bytes[offset+1] = bgra[3]
        bytes[offset+2] = bgra[2]
        bytes[offset+3] = bgra[1]
        bytes[offset+4] = bgra[4]
        offset += 4
    end
    return bytes
end

"""Lazily allocate (or resize) the persistent present buffer for the session."""
function _present_pixels!(session::WindowSession, width::Int, height::Int)
    if session.present_pixels === nothing ||
       session.present_width != width ||
       session.present_height != height
        session.present_pixels = Lava.LavaArray{Vec4f,1}(undef, (width * height,))
        session.present_width = width
        session.present_height = height
    end
    return session.present_pixels
end

"""
Rebuild a capture readback into the flat layout `Lava.blit!` expects: pixel
`(x, y)` at linear index `x * height + y + 1`. Vulkan packs image copies row by
row, so the readback's column-major flattening is the transpose of the blit
layout; the conversion is exact (no resampling) and keeps the presented frame
faithful to the certified capture.
"""
function _present_pixel_data(
    pixels::AbstractMatrix{<:NTuple{4,<:Real}},
    width::Int,
    height::Int,
)::Vector{Vec4f}
    data = Vector{Vec4f}(undef, width * height)
    for column in 0:(width-1)
        base = column * height
        for row in 1:height
            pixel = pixels[column+1, row]
            data[base+row] = Vec4f(
                clamp(Float32(pixel[1]), 0.0f0, 1.0f0),
                clamp(Float32(pixel[2]), 0.0f0, 1.0f0),
                clamp(Float32(pixel[3]), 0.0f0, 1.0f0),
                clamp(Float32(pixel[4]), 0.0f0, 1.0f0),
            )
        end
    end
    return data
end

"""Render `frame_count` presented frames of the packet scene through the open
window session. `camera_override` re-aims the Rust-owned camera for every frame
without changing the packet identity or invalidating GPU resource caches.
Presented frames carry no capture: Tier-A evidence is independently promoted
from the offscreen path over the same packet, so the presented loop never
carries un-promoted evidence and never reads back mid-batch."""
function render_window_frames!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    camera_override::Union{Nothing,WGEGraphics.CameraPacket},
    frame_count::Integer,
    report_input::Bool=false,
)
    session = _window_session(state)
    frame_count >= 1 ||
        throw(AdapterError("invalid_request", "frame_count must be at least one"))
    material = _terrain_material(packet)
    for material_intent in packet.materials
        _validate_material(material_intent)
    end
    lighting = _lighting(packet)
    environment_lighting = _environment_lighting(packet.environment)
    texture_enabled = _material_texture_enabled(material)
    frame_times_us = Int[]
    input_samples = report_input ? Any[] : nothing
    presented = 0
    for frame_index in 1:Int(frame_count)
        frame_started_ns = time_ns()
        Lava.GLFW.PollEvents()
        report_input && push!(input_samples, _sample_window_input(session.window))
        # `Lava.checkopen` is a throwing assertion, not a predicate; the loop
        # stop condition is `Base.isopen` (handle alive and no close request).
        if !isopen(session.window)
            frame_index == 1 && throw(AdapterError(
                "window_closed",
                "the presented window was closed before any frame was presented",
            ))
            break
        end
        active_camera = camera_override === nothing ? packet.camera : camera_override
        width, height = _validate_dimensions(active_camera.width_px, active_camera.height_px)
        window_target = _window_target(session, width, height)
        camera_frame = _camera_frame(active_camera)

        Lava.acquire_next_image!(session.window)
        render_width = 2 * width
        render_height = 2 * height
        scene_framebuffer = _framebuffer!(state, render_width, render_height, true, :scene_hdr)
        scene_target = OffscreenTarget(scene_framebuffer)
        _record_scene_passes!(
            state,
            packet,
            camera_frame,
            lighting,
            environment_lighting,
            material,
            texture_enabled,
            scene_framebuffer,
            scene_target,
        )
        overlay_resources = _overlay_resources!(state, packet, camera_frame)
        composite = _composite_capture!(
            state,
            packet,
            scene_framebuffer,
            overlay_resources,
            width,
            height,
        )

        # A window draw cannot use descriptor sets (Lava's `WindowTarget`
        # `draw!` has no depth attachment and no descriptor kwargs), so the
        # composite reaches the swapchain through Lava's own fullscreen blit of
        # a window-sized RGBA buffer. `readback_framebuffer` flushes the scene
        # batch, which carries no swapchain writes; the upload opens the next
        # batch and `present_frame!` submits it with the acquire-semaphore wait,
        # so the mid-frame readback hazard does not apply to this ordering.
        pixels = readback_framebuffer(composite.capture_framebuffer)
        present_array = _present_pixels!(session, width, height)
        Lava.upload!(present_array, _present_pixel_data(pixels, width, height))
        Lava.blit!(state.queue, window_target, present_array; clear=false)
        state.draw_calls += 1

        Lava.present_frame!(state.queue, session.window)
        session.presented_frames += UInt64(1)
        presented += 1
        push!(frame_times_us, Int(_elapsed_us(frame_started_ns)))
    end
    return (
        frames_presented=presented,
        frame_times_us=frame_times_us,
        window_presented_frames=Int(session.presented_frames),
        input_samples=input_samples,
    )
end

"""Self-contained presented-frame capability probe: three tiny diagnostic
triangle presents through the same window path the session uses. No packet, no
capture, no certification claim."""
function render_window_probe_frame(state::LavaBackend; vsync::Bool=false)
    window = Lava.RenderWindow(
        96,
        64;
        title="WGE window probe",
        vsync=vsync,
        ctx=state.context,
    )
    try
        bq = state.queue
        pipeline = GraphicsPipeline(
            ;
            vertex=_probe_vertex,
            fragment=_probe_fragment,
            cull=NoCull(),
            depth=DepthOff(),
        )
        target = Lava.WindowTarget(window)
        for _ in 1:3
            Lava.acquire_next_image!(window)
            draw!(bq, pipeline, target, 3; clear_color=(0.02f0, 0.03f0, 0.05f0, 1.0f0))
            Lava.present_frame!(bq, window)
            state.draw_calls += 1
        end
        return (presents_requested=3, frame_slots=length(window.in_flight))
    finally
        Lava.close(window)
    end
end

end
