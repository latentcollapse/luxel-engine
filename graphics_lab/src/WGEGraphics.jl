module WGEGraphics

using Base64
using JSON3
using SHA

export GraphicsScenePacket, ProtocolError, validate_scene_packet, packet_summary

const SCENE_PACKET_SCHEMA = "wge.graphics-scene-packet/v6"
# Scene packet v7 adds the TetCage deformation section (P1). v7 and v6 are
# MUTUALLY EXCLUSIVE: a receiver that ignores `deformation` renders a static
# mesh, one that honours it does not, so they must not share a version string.
# Kept in lockstep with native_graphics_contract::deformation.
const SCENE_PACKET_SCHEMA_V7 = "wge.graphics-scene-packet/v7"
const DEFORMATION_V7_ENV = "WGE_TETCAGE_DEFORM_V7"
const MAX_PACKET_ELEMENTS = 16 * 1024 * 1024
const MAX_CAPTURE_DIMENSION = 8192
const MAX_CAPTURE_BYTES = 32 * 1024 * 1024
const MAX_NATIVE_COORDINATE_M = 1.0f6
const MIN_NATIVE_DISTANCE_M = 1.0f-4
const MIN_NATIVE_SCALE = 1.0f-4
const MIN_VECTOR_LENGTH_SQUARED = 1.0f-8

struct ProtocolError <: Exception
    code::String
    detail::String
end

Base.showerror(io::IO, error::ProtocolError) = print(io, error.code, ": ", error.detail)

# N-4 layered terrain surface. Mirrors native_graphics_contract::terrain_layers;
# a ramp (a, b) rises from a to b and falls when a > b.
struct TerrainLayerCoverage
    slope_bp::Union{Nothing,NTuple{2,Int32}}
    height_mm::Union{Nothing,NTuple{2,Int32}}
    macro_ramp::Union{Nothing,NTuple{2,Int32}}  # (threshold_bp, softness_bp)
end

struct TerrainLayerPacket
    layer_id::String
    material_id::String
    metres_per_repeat_milli::UInt32
    coverage::Union{Nothing,TerrainLayerCoverage}
end

struct TerrainLayersPacket
    set_id::String
    set_sha256::String
    layers::Vector{TerrainLayerPacket}
    macro_texture_id::String
end

const MAX_TERRAIN_LAYERS = 3

struct TerrainPacket
    terrain_id::String
    width_m::Float32
    length_m::Float32
    resolution::Int
    material_id::String
    heights_m::Vector{Float32}
    slope_grade::Vector{Float32}
    region_codes::Vector{UInt8}
    layers::Union{Nothing,TerrainLayersPacket}
end

# The eight-field form predates N-4: no layers, the single-material terrain.
TerrainPacket(terrain_id, width_m, length_m, resolution, material_id, heights_m, slope_grade, region_codes) =
    TerrainPacket(terrain_id, width_m, length_m, resolution, material_id, heights_m, slope_grade, region_codes, nothing)

abstract type CameraProjection end

struct OrthographicProjection <: CameraProjection
    span_m::Float32
end

struct PerspectiveProjection <: CameraProjection
    fov_y_degrees::Float32
end

const CameraProjectionValue = Union{OrthographicProjection,PerspectiveProjection}

struct CameraPacket
    camera_id::String
    projection::CameraProjectionValue
    position_xyz_m::NTuple{3,Float32}
    forward_xyz::NTuple{3,Float32}
    up_xyz::NTuple{3,Float32}
    near_plane_m::Float32
    far_plane_m::Float32
    width_px::UInt32
    height_px::UInt32
end

struct MaterialPacket
    material_id::String
    base_color_rgba::NTuple{4,Float32}
    metallic::Float32
    roughness::Float32
    clearcoat::Float32
    clearcoat_roughness::Float32
    alpha_mode::Symbol
    texture_ids::Vector{String}
    normal_texture_id::Union{Nothing,String}
    roughness_texture_id::Union{Nothing,String}
    occlusion_texture_id::Union{Nothing,String}
    emissive_texture_id::Union{Nothing,String}
    normal_scale::Float32
    occlusion_strength::Float32
    emissive_factor_rgb::NTuple{3,Float32}
end

struct TextureMipPacket
    width_px::UInt32
    height_px::UInt32
    bytes::Vector{UInt8}
end

struct TexturePacket
    texture_id::String
    source_artifact_id::String
    sha256::String
    width_px::UInt32
    height_px::UInt32
    mip_levels::UInt32
    color_space::Symbol
    payload::Union{Nothing,Vector{UInt8}}
    mip_chain::Vector{TextureMipPacket}
end

struct MeshPacket
    mesh_id::String
    positions_m::Vector{NTuple{3,Float32}}
    normals::Vector{NTuple{3,Float32}}
    uv0::Vector{NTuple{2,Float32}}
    indices::Vector{UInt32}
    material_id::String
    tangents::Vector{NTuple{4,Float32}}
end

# Procedural/reference packet producers that predate canonical tangent
# carry-through remain valid. Imported projections use the full constructor.
MeshPacket(
    mesh_id::String,
    positions_m::Vector{NTuple{3,Float32}},
    normals::Vector{NTuple{3,Float32}},
    uv0::Vector{NTuple{2,Float32}},
    indices::Vector{UInt32},
    material_id::String,
) = MeshPacket(mesh_id, positions_m, normals, uv0, indices, material_id, NTuple{4,Float32}[])

struct TransformPacket
    translation_xyz_m::NTuple{3,Float32}
    rotation_xyzw::NTuple{4,Float32}
    scale_xyz::NTuple{3,Float32}
end

abstract type InstanceImportance end

struct BackgroundImportance <: InstanceImportance end

struct LandmarkImportance <: InstanceImportance end

struct GameplayCriticalImportance <: InstanceImportance end

const InstanceImportanceValue = Union{
    BackgroundImportance,
    LandmarkImportance,
    GameplayCriticalImportance,
}

struct InstancePacket
    instance_id::String
    mesh_id::String
    material_id::String
    importance::InstanceImportanceValue
    transform::TransformPacket
end

struct DirectionalLightPacket
    direction_xyz::NTuple{3,Float32}
end

struct PointLightPacket
    position_xyz_m::NTuple{3,Float32}
    range_m::Float32
end

const LightKindPacket = Union{DirectionalLightPacket,PointLightPacket}

struct LightPacket
    light_id::String
    kind::LightKindPacket
    color_rgb::NTuple{3,Float32}
    intensity::Float32
end

struct EnvironmentPacket
    sky_top_rgb::NTuple{3,Float32}
    sky_horizon_rgb::NTuple{3,Float32}
    ground_rgb::NTuple{3,Float32}
    fog_color_rgb::NTuple{3,Float32}
    fog_density::Float32
    exposure::Float32
end

# ---------------------------------------------------------------------------
# RenderPolicy (graphical parity sprint §4)
#
# Kept in lockstep with native_graphics_contract::render_policy. Absent means
# "the renderer used its declared defaults", so a packet without a policy
# section must behave EXACTLY as before this type existed — the defaults below
# are the frozen baseline renderer behaviour, and they are asserted against
# the Rust side by tests/render_policy.rs. A default that lives only in the
# adapter is a constant nobody can audit.
# ---------------------------------------------------------------------------

const POLICY_SCALE = 10_000

struct GradePolicy
    lift_rgb_bp::NTuple{3,Int32}
    gamma_bp::Int32
    gain_rgb_bp::NTuple{3,Int32}
    saturation_bp::Int32
end

GradePolicy() = GradePolicy((0, 0, 0), POLICY_SCALE, (POLICY_SCALE, POLICY_SCALE, POLICY_SCALE), POLICY_SCALE)

struct BloomPolicy
    threshold_bp::Int32
    intensity_bp::Int32
end

BloomPolicy() = BloomPolicy(9000, 0)

struct VignettePolicy
    strength_bp::Int32
    radius_bp::Int32
    softness_bp::Int32
end

VignettePolicy() = VignettePolicy(0, 6000, 3000)

struct DitherPolicy
    amplitude_milli_lsb::Int32
end

DitherPolicy() = DitherPolicy(0)

struct TerrainSurfacePolicy
    uv_repeat_scale_milli::Int32
    wrap_repeat::Bool
    macro_variation_bp::Int32
    macro_frequency_milli::Int32
end

# 1000 milli == exactly one repeat across the whole extent == the baseline
# whole-world 0..1 terrain UV, and clamp == the baseline CLAMP_TO_EDGE sampler.
# Both defaults together are byte-identical to the frozen captures.
#
# The name changed from `uv_scale_milli`, which was documented as "repeats per
# world metre" while the adapter applied it as a multiplier on the whole-field
# UV. The two are different units: the documented 1000 would have meant 96 tiles
# on a 96 m field, while the real baseline is one stretch. See the Rust
# `TerrainSurfacePolicy` doc for the full derivation.
TerrainSurfacePolicy() = TerrainSurfacePolicy(1000, false, 0, 40)

struct SamplerPolicy
    anisotropy::Int32
end

SamplerPolicy() = SamplerPolicy(1)

struct ShadowPolicy
    darkness_bp::Int32
    filter_radius_milli::Int32
end

# darkness 7500 reproduces the adapter's historical `0.25 + 0.75 * pcf`
# exactly, so an absent policy keeps the frozen baseline shadow.
ShadowPolicy() = ShadowPolicy(7500, 1000)

# Mesh repeat wrap. Default clamp == the historical CLAMPED_LINEAR_SPEC for
# every mesh material, so an absent policy is byte-identical.
struct MeshSurfacePolicy
    wrap_repeat::Bool
end

MeshSurfacePolicy() = MeshSurfacePolicy(false)

# View-relative shadow fit. There is deliberately no default VALUE: absence
# selects the historical whole-world fit, a different code path.
struct ShadowFitPolicy
    view_distance_m::Int32
end

# View-direction sky. Absence selects the historical screen-space sky draw.
struct SkyPolicy
    sun_disc_radius_milli_deg::Int32
    sun_disc_gain_bp::Int32
    sun_glow_gain_bp::Int32
end

struct RenderPolicy
    grade::GradePolicy
    bloom::BloomPolicy
    vignette::VignettePolicy
    dither::DitherPolicy
    terrain_surface::TerrainSurfacePolicy
    sampler::SamplerPolicy
    shadow::ShadowPolicy
    mesh_surface::MeshSurfacePolicy
    shadow_fit::Union{Nothing,ShadowFitPolicy}
    sky::Union{Nothing,SkyPolicy}
end

RenderPolicy() = RenderPolicy(
    GradePolicy(),
    BloomPolicy(),
    VignettePolicy(),
    DitherPolicy(),
    TerrainSurfacePolicy(),
    SamplerPolicy(),
    ShadowPolicy(),
    MeshSurfacePolicy(),
    nothing,
    nothing,
)

# The seven-axis form predates CONVERGE-0; the three newer axes default to
# their absent meaning (clamp, historical shadow fit, screen-space sky).
RenderPolicy(
    grade::GradePolicy,
    bloom::BloomPolicy,
    vignette::VignettePolicy,
    dither::DitherPolicy,
    terrain_surface::TerrainSurfacePolicy,
    sampler::SamplerPolicy,
    shadow::ShadowPolicy,
) = RenderPolicy(
    grade,
    bloom,
    vignette,
    dither,
    terrain_surface,
    sampler,
    shadow,
    MeshSurfacePolicy(),
    nothing,
    nothing,
)

const RENDER_POLICY_KEYS = (
    "grade",
    "bloom",
    "vignette",
    "dither",
    "terrain_surface",
    "sampler",
    "shadow",
    "mesh_surface",
    "shadow_fit",
    "sky",
)

abstract type OverlayPacket end

struct PointOverlay <: OverlayPacket
    marker_id::String
    role::Symbol
    position_xyz_m::NTuple{3,Float32}
    radius_m::Float32
    color_rgba::NTuple{4,Float32}
end

struct CircleOverlay <: OverlayPacket
    marker_id::String
    role::Symbol
    center_xyz_m::NTuple{3,Float32}
    radius_m::Float32
    color_rgba::NTuple{4,Float32}
end

struct PolylineOverlay <: OverlayPacket
    marker_id::String
    role::Symbol
    points_xyz_m::Vector{NTuple{3,Float32}}
    thickness_m::Float32
    color_rgba::NTuple{4,Float32}
end

const OverlayValue = Union{PointOverlay,CircleOverlay,PolylineOverlay}

struct GraphicsScenePacket
    packet_sha256::String
    content_sha256::String
    packet_id::String
    world_artifact_id::String
    world_artifact_sha256::String
    spatial_fields_sha256::String
    frame_seed::UInt64
    camera::CameraPacket
    terrain::TerrainPacket
    materials::Vector{MaterialPacket}
    textures::Vector{TexturePacket}
    meshes::Vector{MeshPacket}
    instances::Vector{InstancePacket}
    lights::Vector{LightPacket}
    environment::EnvironmentPacket
    overlays::Vector{OverlayValue}
    capture_id::String
    width_px::UInt32
    height_px::UInt32
    include_depth::Bool
    render_policy::RenderPolicy
end

struct PacketSummary
    packet_sha256::String
    world_artifact_id::String
    spatial_fields_sha256::String
    resolution::Int
    terrain_samples::Int
    capture_id::String
    width_px::UInt32
    height_px::UInt32
    overlay_count::Int
end

packet_summary(packet::GraphicsScenePacket)::PacketSummary = PacketSummary(
    packet.packet_sha256,
    packet.world_artifact_id,
    packet.spatial_fields_sha256,
    packet.terrain.resolution,
    length(packet.terrain.heights_m),
    packet.capture_id,
    packet.width_px,
    packet.height_px,
    length(packet.overlays),
)

function validate_scene_packet(payload::AbstractString)::GraphicsScenePacket
    value = try
        JSON3.read(payload)
    catch error
        throw(ProtocolError("malformed_packet", "JSON decode failed: $(sprint(showerror, error))"))
    end
    value isa JSON3.Object || throw(ProtocolError("malformed_packet", "packet must be an object"))
    return _validate_scene_packet(value)
end

function validate_scene_packet(value::JSON3.Object)::GraphicsScenePacket
    return _validate_scene_packet(value)
end

function _validate_scene_packet(value::JSON3.Object)::GraphicsScenePacket
    _exact_keys(value, ("body", "packet_sha256"), "packet")
    body = _object(value["body"], "body")
    _exact_keys(
        body,
        (
            "schema_version",
            "packet_id",
            "world_artifact_id",
            "world_artifact_sha256",
            "spatial_fields_sha256",
            "frame_seed",
            "coordinate_system",
            "camera",
            "terrain",
            "materials",
            "textures",
            "meshes",
            "instances",
            "lights",
            "environment",
            "overlays",
            "capture",
        ),
        ("scene_artifact_id", "scene_artifact_sha256", "deformation", "render_policy"),
        "packet.body",
    )
    schema = _string(body["schema_version"], "packet.body.schema_version")
    schema in (SCENE_PACKET_SCHEMA, SCENE_PACKET_SCHEMA_V7) ||
        throw(ProtocolError("unsupported_schema", "scene packet schema is unsupported"))
    # P1: v7 exists to carry a deformation section and v6 must not, so a
    # receiver never has to guess whether `deformation` was ignored. Enforced
    # BEFORE content validation so every later message about the deformation
    # actually means something.
    _validate_deformation_presence(schema, body)
    packet_sha256 = _sha(body_value=value, path="packet_sha256")
    content_sha256 = _packet_content_sha256(value)
    _valid_sha(packet_sha256, "packet_sha256")
    _valid_id(_string(body["packet_id"], "packet.body.packet_id"), "packet_id")
    world_id = _string(body["world_artifact_id"], "packet.body.world_artifact_id")
    spatial_sha = _string(body["spatial_fields_sha256"], "packet.body.spatial_fields_sha256")
    _valid_id(world_id, "world_artifact_id")
    _valid_sha(_string(body["world_artifact_sha256"], "world_artifact_sha256"), "world_artifact_sha256")
    _valid_sha(spatial_sha, "spatial_fields_sha256")
    scene_artifact_id = _optional_string(body, "scene_artifact_id", "packet.body.scene_artifact_id")
    scene_artifact_sha256 = _optional_string(
        body,
        "scene_artifact_sha256",
        "packet.body.scene_artifact_sha256",
    )
    (scene_artifact_id === nothing) == (scene_artifact_sha256 === nothing) ||
        throw(ProtocolError(
            "provenance",
            "scene artifact identity must include both id and digest",
        ))
    scene_artifact_sha256 === nothing ||
        _valid_sha(scene_artifact_sha256, "packet.body.scene_artifact_sha256")
    frame_seed = _uint64(body["frame_seed"], "frame_seed")

    materials = _parse_materials(body["materials"])
    material_ids = Set(material.material_id for material in materials)
    _validate_coordinate_system(_object(body["coordinate_system"], "coordinate_system"))
    camera = _parse_camera(_object(body["camera"], "camera"))
    terrain = _parse_terrain(_object(body["terrain"], "terrain"), materials)
    textures = _parse_textures(body["textures"])
    _validate_material_texture_links(materials, textures)
    meshes = _parse_meshes(body["meshes"], material_ids)
    instances = _parse_instances(body["instances"], meshes, material_ids)
    lights = _parse_lights(body["lights"])
    environment = _parse_environment(_object(body["environment"], "environment"))
    overlays = _parse_overlays(body["overlays"])
    capture = _object(body["capture"], "capture")
    _validate_capture(capture, body["camera"])

    return GraphicsScenePacket(
        packet_sha256,
        content_sha256,
        _string(body["packet_id"], "packet_id"),
        world_id,
        _string(body["world_artifact_sha256"], "world_artifact_sha256"),
        spatial_sha,
        frame_seed,
        camera,
        terrain,
        materials,
        textures,
        meshes,
        instances,
        lights,
        environment,
        Vector{OverlayValue}(overlays),
        _string(capture["capture_id"], "capture_id"),
        UInt32(_integer(capture["width_px"], "capture.width_px")),
        UInt32(_integer(capture["height_px"], "capture.height_px")),
        _bool(capture["include_depth"], "capture.include_depth"),
        _parse_render_policy(body),
    )
end

function _parse_render_policy(body::JSON3.Object)::RenderPolicy
    haskey(body, "render_policy") || return RenderPolicy()
    value = _object(body["render_policy"], "packet.body.render_policy")
    # Fail closed on unknown axes. Before CONVERGE-0 this object was read
    # key-by-key and an unknown axis (a newer producer's `sky`, a typo) was
    # silently ignored — a frame that claimed a policy it never honoured.
    _exact_keys(value, (), RENDER_POLICY_KEYS, "render_policy")
    grade = if haskey(value, "grade")
        g = _object(value["grade"], "render_policy.grade")
        _exact_keys(
            g,
            ("lift_r_bp", "lift_g_bp", "lift_b_bp", "gamma_bp", "gain_r_bp", "gain_g_bp", "gain_b_bp", "saturation_bp"),
            "render_policy.grade",
        )
        GradePolicy(
            (
                _bounded_bp(g["lift_r_bp"], "grade.lift_r_bp", -1000, 1000),
                _bounded_bp(g["lift_g_bp"], "grade.lift_g_bp", -1000, 1000),
                _bounded_bp(g["lift_b_bp"], "grade.lift_b_bp", -1000, 1000),
            ),
            _bounded_bp(g["gamma_bp"], "grade.gamma_bp", 2500, 40000),
            (
                _bounded_bp(g["gain_r_bp"], "grade.gain_r_bp", 0, 20000),
                _bounded_bp(g["gain_g_bp"], "grade.gain_g_bp", 0, 20000),
                _bounded_bp(g["gain_b_bp"], "grade.gain_b_bp", 0, 20000),
            ),
            _bounded_bp(g["saturation_bp"], "grade.saturation_bp", 0, 20000),
        )
    else
        GradePolicy()
    end
    bloom = if haskey(value, "bloom")
        b = _object(value["bloom"], "render_policy.bloom")
        _exact_keys(b, ("threshold_bp", "intensity_bp"), "render_policy.bloom")
        BloomPolicy(
            _bounded_bp(b["threshold_bp"], "bloom.threshold_bp", 0, POLICY_SCALE),
            _bounded_bp(b["intensity_bp"], "bloom.intensity_bp", 0, 8000),
        )
    else
        BloomPolicy()
    end
    vignette = if haskey(value, "vignette")
        v = _object(value["vignette"], "render_policy.vignette")
        _exact_keys(v, ("strength_bp", "radius_bp", "softness_bp"), "render_policy.vignette")
        parsed = VignettePolicy(
            _bounded_bp(v["strength_bp"], "vignette.strength_bp", 0, 8000),
            _bounded_bp(v["radius_bp"], "vignette.radius_bp", 1000, 10000),
            _bounded_bp(v["softness_bp"], "vignette.softness_bp", 0, 8000),
        )
        (parsed.strength_bp > 0 && parsed.radius_bp >= 10000) && throw(ProtocolError(
            "malformed",
            "render policy vignette is fully engaged but its radius leaves no falloff",
        ))
        parsed
    else
        VignettePolicy()
    end
    dither = if haskey(value, "dither")
        d = _object(value["dither"], "render_policy.dither")
        _exact_keys(d, ("amplitude_milli_lsb",), "render_policy.dither")
        DitherPolicy(
            _bounded_bp(
                d["amplitude_milli_lsb"],
                "dither.amplitude_milli_lsb",
                0,
                2000,
            ),
        )
    else
        DitherPolicy()
    end
    terrain_surface = if haskey(value, "terrain_surface")
        t = _object(value["terrain_surface"], "render_policy.terrain_surface")
        _exact_keys(t, ("uv_repeat_scale_milli", "wrap_repeat", "macro_variation_bp", "macro_frequency_milli"), "render_policy.terrain_surface")
        TerrainSurfacePolicy(
            _bounded_bp(
                t["uv_repeat_scale_milli"],
                "terrain_surface.uv_repeat_scale_milli",
                1,
                32000,
            ),
            _boolean(t["wrap_repeat"], "terrain_surface.wrap_repeat"),
            _bounded_bp(t["macro_variation_bp"], "terrain_surface.macro_variation_bp", 0, 5000),
            _bounded_bp(
                t["macro_frequency_milli"],
                "terrain_surface.macro_frequency_milli",
                1,
                4000,
            ),
        )
    else
        TerrainSurfacePolicy()
    end
    sampler = if haskey(value, "sampler")
        s = _object(value["sampler"], "render_policy.sampler")
        _exact_keys(s, ("anisotropy",), "render_policy.sampler")
        anisotropy = Int32(_integer(s["anisotropy"], "sampler.anisotropy"))
        (anisotropy < 1 || anisotropy > 16) && throw(ProtocolError(
            "malformed",
            "render policy sampler.anisotropy is $anisotropy, outside [1, 16]",
        ))
        SamplerPolicy(anisotropy)
    else
        SamplerPolicy()
    end
    shadow = if haskey(value, "shadow")
        sh = _object(value["shadow"], "render_policy.shadow")
        _exact_keys(sh, ("darkness_bp", "filter_radius_milli"), "render_policy.shadow")
        ShadowPolicy(
            _bounded_bp(sh["darkness_bp"], "shadow.darkness_bp", 0, POLICY_SCALE),
            _bounded_bp(sh["filter_radius_milli"], "shadow.filter_radius_milli", 100, 8000),
        )
    else
        ShadowPolicy()
    end
    mesh_surface = if haskey(value, "mesh_surface")
        m = _object(value["mesh_surface"], "render_policy.mesh_surface")
        _exact_keys(m, ("wrap_repeat",), "render_policy.mesh_surface")
        MeshSurfacePolicy(_boolean(m["wrap_repeat"], "mesh_surface.wrap_repeat"))
    else
        MeshSurfacePolicy()
    end
    shadow_fit = if haskey(value, "shadow_fit")
        f = _object(value["shadow_fit"], "render_policy.shadow_fit")
        _exact_keys(f, ("view_distance_m",), "render_policy.shadow_fit")
        ShadowFitPolicy(_bounded_bp(f["view_distance_m"], "shadow_fit.view_distance_m", 8, 2000))
    else
        nothing
    end
    sky = if haskey(value, "sky")
        k = _object(value["sky"], "render_policy.sky")
        _exact_keys(
            k,
            ("sun_disc_radius_milli_deg", "sun_disc_gain_bp", "sun_glow_gain_bp"),
            "render_policy.sky",
        )
        SkyPolicy(
            _bounded_bp(k["sun_disc_radius_milli_deg"], "sky.sun_disc_radius_milli_deg", 0, 5000),
            _bounded_bp(k["sun_disc_gain_bp"], "sky.sun_disc_gain_bp", 0, 400_000),
            _bounded_bp(k["sun_glow_gain_bp"], "sky.sun_glow_gain_bp", 0, 20000),
        )
    else
        nothing
    end
    return RenderPolicy(
        grade,
        bloom,
        vignette,
        dither,
        terrain_surface,
        sampler,
        shadow,
        mesh_surface,
        shadow_fit,
        sky,
    )
end

"""Strictly-typed boolean reader for policy fields.

`Bool(x)` in Julia would happily accept `1`, `0`, `"true"` and `2`, so a packet
carrying `wrap_repeat: 2` would validate as `true` on the Julia side while Rust
rejected it — a receiver/producer disagreement, which is the exact class of bug
the duplicated Julia validation exists to prevent.
"""
function _boolean(value, label::String)::Bool
    value isa Bool && return value
    throw(ProtocolError(
        "malformed",
        "render policy $label is $(repr(value)), not a JSON boolean",
    ))
end

function _bounded_bp(value, label::String, low::Integer, high::Integer)::Int32
    parsed = Int32(_integer(value, "render policy $label"))
    (parsed < low || parsed > high) && throw(ProtocolError(
        "malformed",
        "render policy $label is $parsed, outside [$low, $high]",
    ))
    return parsed
end

function _parse_terrain(value::JSON3.Object, materials::Vector{MaterialPacket})::TerrainPacket
    _exact_keys(
        value,
        (
            "terrain_id",
            "width_m",
            "length_m",
            "resolution",
            "material_id",
            "heights_m",
            "slope_grade",
            "region_codes",
        ),
        ("layers",),
        "terrain",
    )
    resolution = _integer(value["resolution"], "terrain.resolution")
    3 <= resolution <= 2049 ||
        throw(ProtocolError("malformed_packet", "terrain resolution is outside the native range"))
    resolution * resolution <= MAX_PACKET_ELEMENTS ||
        throw(ProtocolError("malformed_packet", "terrain resolution exceeds packet bound"))
    width = _finite_float32(value["width_m"], "terrain.width_m")
    length = _finite_float32(value["length_m"], "terrain.length_m")
    width >= MIN_NATIVE_DISTANCE_M && length >= MIN_NATIVE_DISTANCE_M ||
        throw(ProtocolError("malformed_packet", "terrain dimensions must be positive"))
    width <= MAX_NATIVE_COORDINATE_M && length <= MAX_NATIVE_COORDINATE_M ||
        throw(ProtocolError("malformed_packet", "terrain dimensions exceed native physical bounds"))
    terrain_id = _string(value["terrain_id"], "terrain.terrain_id")
    material_id = _string(value["material_id"], "terrain.material_id")
    _valid_id(terrain_id, "terrain_id")
    _valid_id(material_id, "terrain.material_id")
    heights = _parse_buffer(value["heights_m"], resolution * resolution, "terrain.heights_m", :f32)
    slope = _parse_buffer(value["slope_grade"], resolution * resolution, "terrain.slope_grade", :f32)
    regions = _parse_buffer(value["region_codes"], resolution * resolution, "terrain.region_codes", :u8)
    any(material.material_id == material_id for material in materials) ||
        throw(ProtocolError("provenance", "terrain references an unknown material"))
    layers = haskey(value, "layers") ? _parse_terrain_layers(value["layers"], materials) : nothing
    return TerrainPacket(terrain_id, width, length, resolution, material_id, heights, slope, regions, layers)
end

function _parse_ramp(value, label::String, low::Integer, high::Integer)::Union{Nothing,NTuple{2,Int32}}
    value === nothing && return nothing
    ramp = (_bounded_bp(value[1], "$label[0]", low, high), _bounded_bp(value[2], "$label[1]", low, high))
    length(value) == 2 && ramp[1] != ramp[2] ||
        throw(ProtocolError("malformed_packet", "$label must be two distinct values"))
    return ramp
end

"""Receiver-side check of the N-4 layer schema. A receiver must be able to
refuse a packet it did not produce, so the structural rules are duplicated
here rather than trusted from the producer."""
function _parse_terrain_layers(value, materials::Vector{MaterialPacket})::TerrainLayersPacket
    object = _object(value, "terrain.layers")
    _exact_keys(object, ("set_id", "set_sha256", "layers", "macro_texture_id"), "terrain.layers")
    entries = object["layers"]
    2 <= length(entries) <= MAX_TERRAIN_LAYERS ||
        throw(ProtocolError("malformed_packet", "terrain carries $(length(entries)) layers; expected 2..=$MAX_TERRAIN_LAYERS"))
    layers = TerrainLayerPacket[]
    for (index, entry) in enumerate(entries)
        layer = _object(entry, "terrain.layers[]")
        _exact_keys(layer, ("layer_id", "material_id", "metres_per_repeat_milli"), ("coverage",), "terrain layer")
        material_id = _string(layer["material_id"], "terrain layer material_id")
        any(material.material_id == material_id for material in materials) ||
            throw(ProtocolError("provenance", "terrain layer references unknown material $material_id"))
        metres = _bounded_bp(layer["metres_per_repeat_milli"], "terrain layer metres_per_repeat_milli", 100, 100_000)
        coverage = if haskey(layer, "coverage")
            index == 1 && throw(ProtocolError("malformed_packet", "terrain layer 0 is the base and cannot carry coverage"))
            c = _object(layer["coverage"], "terrain layer coverage")
            _exact_keys(c, (), ("slope_bp", "height_mm", "macro_ramp"), "terrain layer coverage")
            macro_ramp = if haskey(c, "macro_ramp")
                m = _object(c["macro_ramp"], "terrain layer macro_ramp")
                _exact_keys(m, ("threshold_bp", "softness_bp"), "terrain layer macro_ramp")
                (_bounded_bp(m["threshold_bp"], "macro_ramp.threshold_bp", 0, 10_000),
                 _bounded_bp(m["softness_bp"], "macro_ramp.softness_bp", 1, 5_000))
            else
                nothing
            end
            parsed = TerrainLayerCoverage(
                _parse_ramp(get(c, "slope_bp", nothing), "coverage.slope_bp", 0, 10_000),
                _parse_ramp(get(c, "height_mm", nothing), "coverage.height_mm", -1_000_000_000, 1_000_000_000),
                macro_ramp,
            )
            parsed.slope_bp === nothing && parsed.height_mm === nothing && parsed.macro_ramp === nothing &&
                throw(ProtocolError("malformed_packet", "terrain layer coverage has no terms"))
            parsed
        else
            index == 1 || throw(ProtocolError("malformed_packet", "terrain layer $index needs a coverage rule"))
            nothing
        end
        push!(layers, TerrainLayerPacket(
            _string(layer["layer_id"], "terrain layer_id"),
            material_id,
            UInt32(metres),
            coverage,
        ))
    end
    return TerrainLayersPacket(
        _string(object["set_id"], "terrain.layers.set_id"),
        _string(object["set_sha256"], "terrain.layers.set_sha256"),
        layers,
        _string(object["macro_texture_id"], "terrain.layers.macro_texture_id"),
    )
end

function _parse_buffer(
    value::JSON3.Object,
    expected_count::Int,
    label::String,
    expected_encoding::Symbol,
)
    object = _object(value, label)
    required_keys = ("buffer_id", "byte_length", "count", "stride_bytes", "sha256", "payload")
    _exact_keys(object, required_keys, ("source_artifact_id",), label)
    buffer_id = _string(object["buffer_id"], "$label.buffer_id")
    _valid_id(buffer_id, "$label.buffer_id")
    haskey(object, "source_artifact_id") && object["source_artifact_id"] !== nothing &&
        _valid_id(_string(object["source_artifact_id"], "$label.source_artifact_id"), "$label.source_artifact_id")
    count = _integer(object["count"], "$label.count")
    count == expected_count || throw(ProtocolError("malformed_packet", "$label count does not match terrain"))
    payload = _object(object["payload"], "$label.payload")
    _exact_keys(payload, ("encoding", "values"), "$label.payload")
    encoding = Symbol(_string(payload["encoding"], "$label.payload.encoding"))
    encoding == expected_encoding || throw(ProtocolError("malformed_packet", "$label encoding is not expected"))
    values = _values(payload["values"], encoding, label)
    length(values) == count || throw(ProtocolError("malformed_packet", "$label payload count mismatch"))
    byte_length = _integer(object["byte_length"], "$label.byte_length")
    stride = _integer(object["stride_bytes"], "$label.stride_bytes")
    expected_stride = encoding == :f32 || encoding == :u32 ? 4 : 1
    byte_length == count * expected_stride || throw(ProtocolError("provenance", "$label byte length mismatch"))
    stride == expected_stride || throw(ProtocolError("provenance", "$label stride mismatch"))
    digest = _string(object["sha256"], "$label.sha256")
    _valid_sha(digest, "$label.sha256")
    digest == _payload_sha256(values) || throw(ProtocolError("provenance", "$label digest mismatch"))
    return values
end

function _values(value, ::Val{:f32}, label::String)
    array = _array(value, "$label.values")
    values = Float32[_finite_float32(item, "$label.values[$index]") for (index, item) in enumerate(array)]
    all(item -> abs(item) <= MAX_NATIVE_COORDINATE_M, values) ||
        throw(ProtocolError("malformed_packet", "$label exceeds native physical bounds"))
    return values
end

function _values(value, ::Val{:u8}, label::String)
    array = _array(value, "$label.values")
    return UInt8[_uint8(item, "$label.values[$index]") for (index, item) in enumerate(array)]
end

function _values(value, ::Val{:u32}, label::String)
    array = _array(value, "$label.values")
    return UInt32[_uint32(item, "$label.values[$index]") for (index, item) in enumerate(array)]
end

_values(value, encoding::Symbol, label::String) = _values(value, Val(encoding), label)

function _payload_sha256(values::Vector{Float32})
    bytes = UInt8[]
    sizehint!(bytes, 4 * length(values))
    for value in values
        bits = reinterpret(UInt32, value)
        append!(
            bytes,
            UInt8[
                bits&0xff,
                (bits>>8)&0xff,
                (bits>>16)&0xff,
                (bits>>24)&0xff,
            ],
        )
    end
    return _sha256(bytes)
end

function _payload_sha256(values::Vector{UInt8})
    return _sha256(values)
end

function _payload_sha256(values::Vector{UInt32})
    bytes = UInt8[]
    sizehint!(bytes, 4 * length(values))
    for value in values
        append!(
            bytes,
            UInt8[
                value&0xff,
                (value>>8)&0xff,
                (value>>16)&0xff,
                (value>>24)&0xff,
            ],
        )
    end
    return _sha256(bytes)
end

function _parse_materials(value::JSON3.Array)::Vector{MaterialPacket}
    array = _array(value, "materials")
    ids = Set{String}()
    materials = MaterialPacket[]
    sizehint!(materials, length(array))
    for material in array
        object = _object(material, "material")
        required_keys = (
            "material_id",
            "base_color_rgba",
            "metallic",
            "roughness",
            "clearcoat",
            "clearcoat_roughness",
            "alpha_mode",
            "texture_ids",
            "normal_scale",
            "occlusion_strength",
            "emissive_factor_rgb",
        )
        optional_keys = (
            "normal_texture_id",
            "roughness_texture_id",
            "occlusion_texture_id",
            "emissive_texture_id",
        )
        _exact_keys(object, required_keys, optional_keys, "material")
        id = _string(object["material_id"], "material.material_id")
        _valid_id(id, "material_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate material $id"))
        push!(ids, id)
        color = _tuple(object["base_color_rgba"], Val(4), "material.base_color_rgba")
        all(channel -> 0.0f0 <= channel <= 1.0f0, color) ||
            throw(ProtocolError("malformed_packet", "material color is outside [0, 1]"))
        metallic = _finite_float32(object["metallic"], "material.metallic")
        roughness = _finite_float32(object["roughness"], "material.roughness")
        0.0f0 <= metallic <= 1.0f0 ||
            throw(ProtocolError("malformed_packet", "material metallic is outside [0, 1]"))
        0.0f0 <= roughness <= 1.0f0 ||
            throw(ProtocolError("malformed_packet", "material roughness is outside [0, 1]"))
        clearcoat = _finite_float32(object["clearcoat"], "material.clearcoat")
        clearcoat_roughness = _finite_float32(object["clearcoat_roughness"], "material.clearcoat_roughness")
        0.0f0 <= clearcoat <= 1.0f0 ||
            throw(ProtocolError("malformed_packet", "material clearcoat is outside [0, 1]"))
        0.045f0 <= clearcoat_roughness <= 1.0f0 ||
            throw(ProtocolError("malformed_packet", "material clearcoat roughness is outside [0.045, 1]"))
        alpha_mode = Symbol(_string(object["alpha_mode"], "material.alpha_mode"))
        alpha_mode in (:opaque, :mask, :blend) ||
            throw(ProtocolError("unsupported", "material alpha mode is unsupported"))
        texture_ids = String[]
        for item in _array(object["texture_ids"], "material.texture_ids")
            texture_id = _string(item, "material.texture_id")
            _valid_id(texture_id, "material.texture_id")
            push!(texture_ids, texture_id)
        end
        normal_texture_id = _optional_string(object, "normal_texture_id", "material.normal_texture_id")
        roughness_texture_id = _optional_string(object, "roughness_texture_id", "material.roughness_texture_id")
        occlusion_texture_id = _optional_string(object, "occlusion_texture_id", "material.occlusion_texture_id")
        emissive_texture_id = _optional_string(object, "emissive_texture_id", "material.emissive_texture_id")
        normal_scale = _finite_float32(object["normal_scale"], "material.normal_scale")
        0.0f0 <= normal_scale <= 2.0f0 ||
            throw(ProtocolError("malformed_packet", "material normal scale is outside [0, 2]"))
        occlusion_strength = _finite_float32(object["occlusion_strength"], "material.occlusion_strength")
        0.0f0 <= occlusion_strength <= 1.0f0 ||
            throw(ProtocolError("malformed_packet", "material occlusion strength is outside [0, 1]"))
        emissive_factor_rgb = _tuple(object["emissive_factor_rgb"], Val(3), "material.emissive_factor_rgb")
        all(channel -> 0.0f0 <= channel <= 16.0f0, emissive_factor_rgb) ||
            throw(ProtocolError("malformed_packet", "material emissive factor is outside [0, 16]"))
        push!(
            materials,
            MaterialPacket(
                id,
                color,
                metallic,
                roughness,
                clearcoat,
                clearcoat_roughness,
                alpha_mode,
                texture_ids,
                normal_texture_id,
                roughness_texture_id,
                occlusion_texture_id,
                emissive_texture_id,
                normal_scale,
                occlusion_strength,
                emissive_factor_rgb,
            ),
        )
    end
    return materials
end

function _parse_textures(value::JSON3.Array)::Vector{TexturePacket}
    array = _array(value, "textures")
    ids = Set{String}()
    textures = TexturePacket[]
    sizehint!(textures, length(array))
    for texture in array
        object = _object(texture, "texture")
        required_keys = (
            "texture_id",
            "source_artifact_id",
            "sha256",
            "width_px",
            "height_px",
            "mip_levels",
            "color_space",
        )
        _exact_keys(object, required_keys, ("payload",), "texture")
        id = _string(object["texture_id"], "texture.texture_id")
        _valid_id(id, "texture_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate texture $id"))
        push!(ids, id)
        source_artifact_id = _string(object["source_artifact_id"], "texture.source_artifact_id")
        _valid_id(source_artifact_id, "texture.source_artifact_id")
        sha256 = _string(object["sha256"], "texture.sha256")
        _valid_sha(sha256, "texture.sha256")
        width = _uint32(object["width_px"], "texture.width_px")
        height = _uint32(object["height_px"], "texture.height_px")
        mip_levels = _uint32(object["mip_levels"], "texture.mip_levels")
        width > 0 && height > 0 && mip_levels > 0 ||
            throw(ProtocolError("malformed_packet", "texture dimensions or mip count are invalid"))
        mip_levels <= 32 ||
            throw(ProtocolError("unsupported", "texture mip count exceeds the native bound"))
        width <= MAX_CAPTURE_DIMENSION && height <= MAX_CAPTURE_DIMENSION ||
            throw(ProtocolError("malformed_packet", "texture dimensions exceed the native range"))
        color_space = Symbol(_string(object["color_space"], "texture.color_space"))
        color_space in (:srgb, :linear, :normal_map, :data) ||
            throw(ProtocolError("unsupported", "texture color space is unsupported"))
        payload = nothing
        mip_chain = TextureMipPacket[]
        if haskey(object, "payload") && object["payload"] !== nothing
            payload_object = _object(object["payload"], "texture.payload")
            _exact_keys(payload_object, ("encoding", "base64"), "texture.payload")
            encoding = _string(payload_object["encoding"], "texture.payload.encoding")
            if encoding == "rgba8"
                mip_levels == 1 ||
                    throw(ProtocolError("unsupported", "single-level texture payload cannot claim multiple mip levels"))
                encoded = _string(payload_object["base64"], "texture.payload.base64")
                payload = _decode_texture_level(encoded, width, height, 0, id)
                _sha256(payload) == sha256 ||
                    throw(ProtocolError("provenance", "texture payload digest does not match metadata"))
            elseif encoding == "rgba8_mip_chain"
                levels_object = _object(payload_object["base64"], "texture.payload.base64")
                _exact_keys(levels_object, ("levels",), "texture.payload.base64")
                level_values = _array(levels_object["levels"], "texture.payload.base64.levels")
                length(level_values) == Int(mip_levels) ||
                    throw(ProtocolError("provenance", "texture mip payload level count does not match metadata"))
                all_bytes = UInt8[]
                for (level_index, level_value) in enumerate(level_values)
                    level_object = _object(level_value, "texture mip level")
                    _exact_keys(level_object, ("width_px", "height_px", "base64"), "texture mip level")
                    level_width = _uint32(level_object["width_px"], "texture mip level.width_px")
                    level_height = _uint32(level_object["height_px"], "texture mip level.height_px")
                    expected_width, expected_height = _mip_dimensions(width, height, level_index - 1)
                    (level_width, level_height) == (expected_width, expected_height) ||
                        throw(ProtocolError("provenance", "texture mip level dimensions do not match metadata"))
                    encoded = _string(level_object["base64"], "texture mip level.base64")
                    level_bytes = _decode_texture_level(encoded, level_width, level_height, level_index - 1, id)
                    append!(all_bytes, level_bytes)
                    if level_index == 1
                        payload = level_bytes
                    else
                        push!(mip_chain, TextureMipPacket(level_width, level_height, level_bytes))
                    end
                end
                _sha256(all_bytes) == sha256 ||
                    throw(ProtocolError("provenance", "texture mip payload digest does not match metadata"))
            else
                throw(ProtocolError("unsupported", "texture payload encoding is unsupported"))
            end
        end
        push!(
            textures,
            TexturePacket(id, source_artifact_id, sha256, width, height, mip_levels, color_space, payload, mip_chain),
        )
    end
    return textures
end

function _decode_texture_level(
    encoded::String,
    width::UInt32,
    height::UInt32,
    level::Int,
    texture_id::String,
)
    bytes = try
        base64decode(encoded)
    catch error
        throw(ProtocolError("malformed_packet", "texture $texture_id mip $level payload is not valid base64: $(sprint(showerror, error))"))
    end
    length(bytes) == Int(width) * Int(height) * 4 ||
        throw(ProtocolError("provenance", "texture $texture_id mip $level payload byte length does not match dimensions"))
    return bytes
end

function _mip_dimensions(width::UInt32, height::UInt32, level::Int)
    level >= 0 || throw(ProtocolError("malformed_packet", "texture mip level must be nonnegative"))
    current_width = width
    current_height = height
    for _ in 1:level
        current_width = max(UInt32(1), current_width ÷ UInt32(2))
        current_height = max(UInt32(1), current_height ÷ UInt32(2))
    end
    return current_width, current_height
end

function _validate_material_texture_links(materials::Vector{MaterialPacket}, textures::Vector{TexturePacket})
    isempty(materials) && throw(ProtocolError("malformed_packet", "packet needs a material"))
    texture_ids = Set(texture.texture_id for texture in textures)
    for material in materials
        for texture_id in material.texture_ids
            texture_id in texture_ids ||
                throw(ProtocolError("provenance", "material references unknown texture $texture_id"))
        end
        for texture_id in (
            material.normal_texture_id,
            material.roughness_texture_id,
            material.occlusion_texture_id,
            material.emissive_texture_id,
        )
            texture_id === nothing && continue
            texture_id in texture_ids ||
                throw(ProtocolError("provenance", "material references unknown texture $texture_id"))
        end
    end
end

function _parse_meshes(value::JSON3.Array, material_ids::Set{String})::Vector{MeshPacket}
    array = _array(value, "meshes")
    ids = Set{String}()
    meshes = MeshPacket[]
    sizehint!(meshes, length(array))
    for mesh in array
        object = _object(mesh, "mesh")
        _exact_keys(
            object,
            ("mesh_id", "positions_m", "normals", "uv0", "indices", "material_id"),
            ("tangents",),
            "mesh",
        )
        id = _string(object["mesh_id"], "mesh.mesh_id")
        _valid_id(id, "mesh_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate mesh $id"))
        push!(ids, id)
        positions = NTuple{3,Float32}[
            _tuple(position, Val(3), "mesh.positions_m") for
            position in _array(object["positions_m"], "mesh.positions_m")
        ]
        all(position -> _bounded_values(position, "mesh.positions_m"), positions) ||
            throw(ProtocolError("malformed_packet", "mesh positions exceed native physical bounds"))
        normals = NTuple{3,Float32}[
            _tuple(normal, Val(3), "mesh.normals") for
            normal in _array(object["normals"], "mesh.normals")
        ]
        all(normal -> _bounded_values(normal, "mesh.normals"), normals) ||
            throw(ProtocolError("malformed_packet", "mesh normals exceed native physical bounds"))
        uv0 = NTuple{2,Float32}[
            _tuple(uv, Val(2), "mesh.uv0") for
            uv in _array(object["uv0"], "mesh.uv0")
        ]
        all(uv -> _bounded_values(uv, "mesh.uv0"), uv0) ||
            throw(ProtocolError("malformed_packet", "mesh uv0 exceeds native physical bounds"))
        !isempty(positions) && length(positions) == length(normals) == length(uv0) ||
            throw(ProtocolError("malformed_packet", "mesh position, normal, and uv0 counts differ"))
        tangents = haskey(object, "tangents") ?
            NTuple{4,Float32}[
                _tuple(tangent, Val(4), "mesh.tangents") for
                tangent in _array(object["tangents"], "mesh.tangents")
            ] :
            NTuple{4,Float32}[]
        isempty(tangents) || length(tangents) == length(positions) ||
            throw(ProtocolError("malformed_packet", "mesh tangent count must be zero or match positions"))
        all(tangent -> _bounded_values(tangent, "mesh.tangents"), tangents) ||
            throw(ProtocolError("malformed_packet", "mesh tangents exceed native physical bounds"))
        all(
            tangent -> isfinite(_squared_norm((tangent[1], tangent[2], tangent[3]))) &&
                _squared_norm((tangent[1], tangent[2], tangent[3])) > MIN_VECTOR_LENGTH_SQUARED &&
                abs(abs(tangent[4]) - 1.0f0) <= 1.0f-3,
            tangents,
        ) || throw(ProtocolError("malformed_packet", "mesh tangents must be non-degenerate with unit handedness"))
        indices = UInt32[
            _uint32(index, "mesh.indices") for index in _array(object["indices"], "mesh.indices")
        ]
        isempty(positions) && throw(ProtocolError("malformed_packet", "mesh needs positions"))
        all(
            normal -> isfinite(_squared_norm(normal)) && _squared_norm(normal) > MIN_VECTOR_LENGTH_SQUARED,
            normals,
        ) ||
            throw(ProtocolError("malformed_packet", "mesh normals must be non-degenerate"))
        !isempty(indices) && length(indices) % 3 == 0 ||
            throw(ProtocolError("malformed_packet", "mesh indices must form triangles"))
        all(index -> Int(index) < length(positions), indices) ||
            throw(ProtocolError("malformed_packet", "mesh index is out of range"))
        material_id = _string(object["material_id"], "mesh.material_id")
        _valid_id(material_id, "mesh.material_id")
        material_id in material_ids ||
            throw(ProtocolError("provenance", "mesh references unknown material $material_id"))
        push!(meshes, MeshPacket(id, positions, normals, uv0, indices, material_id, tangents))
    end
    return meshes
end

function _parse_instances(
    value::JSON3.Array,
    meshes::Vector{MeshPacket},
    material_ids::Set{String},
)::Vector{InstancePacket}
    array = _array(value, "instances")
    mesh_ids = Set(mesh.mesh_id for mesh in meshes)
    ids = Set{String}()
    instances = InstancePacket[]
    sizehint!(instances, length(array))
    for instance in array
        object = _object(instance, "instance")
        _exact_keys(object, ("instance_id", "mesh_id", "material_id", "importance", "transform"), "instance")
        id = _string(object["instance_id"], "instance.instance_id")
        _valid_id(id, "instance_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate instance $id"))
        push!(ids, id)
        mesh_id = _string(object["mesh_id"], "instance.mesh_id")
        material_id = _string(object["material_id"], "instance.material_id")
        _valid_id(mesh_id, "mesh_id")
        _valid_id(material_id, "material_id")
        mesh_id in mesh_ids ||
            throw(ProtocolError("provenance", "instance references unknown mesh $mesh_id"))
        material_id in material_ids ||
            throw(ProtocolError("provenance", "instance references unknown material $material_id"))
        importance = _parse_instance_importance(_string(object["importance"], "instance.importance"))
        transform = _object(object["transform"], "instance.transform")
        _exact_keys(transform, ("translation_xyz_m", "rotation_xyzw", "scale_xyz"), "instance.transform")
        translation = _tuple(transform["translation_xyz_m"], Val(3), "instance.transform.translation_xyz_m")
        _bounded_values(translation, "instance.transform.translation_xyz_m") ||
            throw(ProtocolError("malformed_packet", "instance translation exceeds native physical bounds"))
        rotation = _tuple(transform["rotation_xyzw"], Val(4), "instance.transform.rotation_xyzw")
        scale = _tuple(transform["scale_xyz"], Val(3), "instance.transform.scale_xyz")
        rotation_norm_squared = _squared_norm(rotation)
        isfinite(rotation_norm_squared) && rotation_norm_squared > eps(Float32) ||
            throw(ProtocolError("malformed_packet", "instance rotation is degenerate"))
        abs(rotation_norm_squared - 1.0f0) <= 1.0f-3 ||
            throw(ProtocolError("malformed_packet", "instance rotation must be a unit quaternion"))
        all(value -> value >= MIN_NATIVE_SCALE, scale) ||
            throw(ProtocolError("malformed_packet", "instance scale must be positive"))
        all(value -> value <= MAX_NATIVE_COORDINATE_M, scale) ||
            throw(ProtocolError("malformed_packet", "instance scale exceeds native physical bounds"))
        push!(instances, InstancePacket(id, mesh_id, material_id, importance, TransformPacket(translation, rotation, scale)))
    end
    return instances
end

function _parse_instance_importance(value::String)::InstanceImportanceValue
    return _parse_instance_importance(Val(Symbol(value)))
end

_parse_instance_importance(::Val{:background}) = BackgroundImportance()

_parse_instance_importance(::Val{:landmark}) = LandmarkImportance()

_parse_instance_importance(::Val{:gameplay_critical}) = GameplayCriticalImportance()

function _parse_instance_importance(::Val{kind}) where {kind}
    throw(ProtocolError("unsupported", "instance importance $(kind) is unsupported"))
end

function _parse_lights(value::JSON3.Array)::Vector{LightPacket}
    array = _array(value, "lights")
    isempty(array) && throw(ProtocolError("malformed_packet", "packet needs a light"))
    ids = Set{String}()
    lights = LightPacket[]
    sizehint!(lights, length(array))
    for light in array
        object = _object(light, "light")
        _exact_keys(object, ("light_id", "kind", "color_rgb", "intensity"), "light")
        id = _string(object["light_id"], "light.light_id")
        _valid_id(id, "light_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate light $id"))
        push!(ids, id)
        color = _tuple(object["color_rgb"], Val(3), "light.color_rgb")
        all(channel -> 0.0f0 <= channel <= MAX_NATIVE_COORDINATE_M, color) ||
            throw(ProtocolError("malformed_packet", "light color is outside native bounds"))
        intensity = _finite_float32(object["intensity"], "light.intensity")
        0.0f0 <= intensity <= MAX_NATIVE_COORDINATE_M ||
            throw(ProtocolError("malformed_packet", "light intensity is outside native bounds"))
        push!(lights, LightPacket(id, _parse_light_kind(_object(object["kind"], "light.kind")), color, intensity))
    end
    return lights
end

function _parse_environment(value::JSON3.Object)::EnvironmentPacket
    _exact_keys(
        value,
        ("sky_top_rgb", "sky_horizon_rgb", "ground_rgb", "fog_color_rgb", "fog_density", "exposure"),
        "environment",
    )
    sky_top = _tuple(value["sky_top_rgb"], Val(3), "environment.sky_top_rgb")
    sky_horizon = _tuple(value["sky_horizon_rgb"], Val(3), "environment.sky_horizon_rgb")
    ground = _tuple(value["ground_rgb"], Val(3), "environment.ground_rgb")
    fog_color = _tuple(value["fog_color_rgb"], Val(3), "environment.fog_color_rgb")
    for (color, label) in ((sky_top, "sky_top_rgb"), (sky_horizon, "sky_horizon_rgb"), (ground, "ground_rgb"), (fog_color, "fog_color_rgb"))
        all(channel -> 0.0f0 <= channel <= 1.0f0, color) ||
            throw(ProtocolError("malformed_packet", "environment.$label is outside [0, 1]"))
    end
    fog_density = _finite_float32(value["fog_density"], "environment.fog_density")
    exposure = _finite_float32(value["exposure"], "environment.exposure")
    0.0f0 <= fog_density <= 1.0f0 ||
        throw(ProtocolError("malformed_packet", "environment fog density is outside [0, 1]"))
    0.01f0 <= exposure <= 16.0f0 ||
        throw(ProtocolError("malformed_packet", "environment exposure is outside [0.01, 16]"))
    return EnvironmentPacket(sky_top, sky_horizon, ground, fog_color, fog_density, exposure)
end

function _parse_light_kind(value::JSON3.Object)::LightKindPacket
    kind = Symbol(_string(value["kind"], "light.kind.kind"))
    return _parse_light_kind(Val(kind), value)
end

function _parse_light_kind(::Val{:directional}, value::JSON3.Object)::DirectionalLightPacket
    _exact_keys(value, ("kind", "direction_xyz"), "light.kind")
    direction = _tuple(value["direction_xyz"], Val(3), "light.kind.direction_xyz")
    direction_norm_squared = _squared_norm(direction)
    isfinite(direction_norm_squared) && direction_norm_squared > MIN_VECTOR_LENGTH_SQUARED ||
        throw(ProtocolError("malformed_packet", "directional light direction is degenerate"))
    return DirectionalLightPacket(direction)
end

function _parse_light_kind(::Val{:point}, value::JSON3.Object)::PointLightPacket
    _exact_keys(value, ("kind", "position_xyz_m", "range_m"), "light.kind")
    range = _finite_float32(value["range_m"], "light.kind.range_m")
    range > 0.0f0 && range <= MAX_NATIVE_COORDINATE_M ||
        throw(ProtocolError("malformed_packet", "point light range is invalid"))
    position = _tuple(value["position_xyz_m"], Val(3), "light.kind.position_xyz_m")
    _bounded_values(position, "light.kind.position_xyz_m") ||
        throw(ProtocolError("malformed_packet", "point light position exceeds native physical bounds"))
    return PointLightPacket(
        position,
        range,
    )
end

function _parse_light_kind(::Val{kind}, ::JSON3.Object) where {kind}
    throw(ProtocolError("unsupported", "light kind $(kind) is unsupported"))
end

function _parse_overlays(value::JSON3.Array)::Vector{OverlayValue}
    array = _array(value, "overlays")
    ids = Set{String}()
    overlays = OverlayValue[]
    sizehint!(overlays, length(array))
    for overlay in array
        object = _object(overlay, "overlay")
        parsed = _parse_overlay(object)
        id = parsed.marker_id
        _valid_id(id, "marker_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate overlay $id"))
        push!(ids, id)
        push!(overlays, parsed)
    end
    return overlays
end

function _parse_overlay(value::JSON3.Object)::OverlayValue
    kind = Symbol(_string(value["kind"], "overlay.kind"))
    return _parse_overlay(Val(kind), value)
end

function _overlay_header(value::JSON3.Object)
    marker_id = _string(value["marker_id"], "overlay.marker_id")
    role = Symbol(_string(value["role"], "overlay.role"))
    role in (:route, :player_spawn, :opponent_spawn, :encounter, :objective) ||
        throw(ProtocolError("unsupported", "overlay role is unsupported"))
    return marker_id, role
end

function _overlay_color(value::JSON3.Object)::NTuple{4,Float32}
    color = _tuple(value["color_rgba"], Val(4), "overlay.color_rgba")
    all(channel -> 0.0f0 <= channel <= 1.0f0, color) ||
        throw(ProtocolError("malformed_packet", "overlay color is outside [0, 1]"))
    return color
end

function _parse_overlay(::Val{:point}, value::JSON3.Object)::PointOverlay
    _exact_keys(
        value,
        ("kind", "marker_id", "role", "position_xyz_m", "radius_m", "color_rgba"),
        "overlay",
    )
    marker_id, role = _overlay_header(value)
    radius = _finite_float32(value["radius_m"], "overlay.radius_m")
    MIN_NATIVE_DISTANCE_M <= radius <= MAX_NATIVE_COORDINATE_M ||
        throw(ProtocolError("malformed_packet", "overlay radius is invalid"))
    return PointOverlay(
        marker_id,
        role,
        begin
            position = _tuple(value["position_xyz_m"], Val(3), "overlay.position_xyz_m")
            _bounded_values(position, "overlay.position_xyz_m") ||
                throw(ProtocolError("malformed_packet", "overlay position exceeds native physical bounds"))
            position
        end,
        radius,
        _overlay_color(value),
    )
end

function _parse_overlay(::Val{:circle}, value::JSON3.Object)::CircleOverlay
    _exact_keys(
        value,
        ("kind", "marker_id", "role", "center_xyz_m", "radius_m", "color_rgba"),
        "overlay",
    )
    marker_id, role = _overlay_header(value)
    radius = _finite_float32(value["radius_m"], "overlay.radius_m")
    MIN_NATIVE_DISTANCE_M <= radius <= MAX_NATIVE_COORDINATE_M ||
        throw(ProtocolError("malformed_packet", "overlay radius is invalid"))
    return CircleOverlay(
        marker_id,
        role,
        begin
            center = _tuple(value["center_xyz_m"], Val(3), "overlay.center_xyz_m")
            _bounded_values(center, "overlay.center_xyz_m") ||
                throw(ProtocolError("malformed_packet", "overlay center exceeds native physical bounds"))
            center
        end,
        radius,
        _overlay_color(value),
    )
end

function _parse_overlay(::Val{:polyline}, value::JSON3.Object)::PolylineOverlay
    _exact_keys(
        value,
        ("kind", "marker_id", "role", "points_xyz_m", "thickness_m", "color_rgba"),
        "overlay",
    )
    marker_id, role = _overlay_header(value)
    point_values = _array(value["points_xyz_m"], "overlay.points_xyz_m")
    length(point_values) >= 2 || throw(ProtocolError("malformed_packet", "polyline needs two points"))
    points = NTuple{3,Float32}[_tuple(point, Val(3), "overlay point") for point in point_values]
    all(point -> _bounded_values(point, "overlay point"), points) ||
        throw(ProtocolError("malformed_packet", "polyline point exceeds native physical bounds"))
    thickness = _finite_float32(value["thickness_m"], "overlay.thickness_m")
    MIN_NATIVE_DISTANCE_M <= thickness <= MAX_NATIVE_COORDINATE_M ||
        throw(ProtocolError("malformed_packet", "overlay thickness is invalid"))
    return PolylineOverlay(marker_id, role, points, thickness, _overlay_color(value))
end

function _parse_overlay(::Val{kind}, ::JSON3.Object) where {kind}
    throw(ProtocolError("unsupported", "overlay kind $(kind) is unsupported"))
end

function _validate_coordinate_system(value::JSON3.Object)::Nothing
    _exact_keys(value, ("up_axis", "handedness", "units_per_meter"), "coordinate_system")
    _string(value["up_axis"], "coordinate_system.up_axis") == "y" ||
        throw(ProtocolError("unsupported", "native graphics requires Y up"))
    _string(value["handedness"], "coordinate_system.handedness") == "right" ||
        throw(ProtocolError("unsupported", "native graphics requires right-handed coordinates"))
    _finite_float32(value["units_per_meter"], "coordinate_system.units_per_meter") == 1.0f0 ||
        throw(ProtocolError("unsupported", "native graphics requires one unit per meter"))
    return nothing
end

function _parse_camera(value::JSON3.Object)::CameraPacket
    _exact_keys(
        value,
        (
            "camera_id",
            "projection",
            "position_xyz_m",
            "forward_xyz",
            "up_xyz",
            "near_plane_m",
            "far_plane_m",
            "width_px",
            "height_px",
        ),
        "camera",
    )
    camera_id = _string(value["camera_id"], "camera.camera_id")
    _valid_id(camera_id, "camera_id")
    position = _tuple(value["position_xyz_m"], Val(3), "camera.position_xyz_m")
    _bounded_values(position, "camera.position_xyz_m") ||
        throw(ProtocolError("malformed_packet", "camera position exceeds native physical bounds"))
    forward = _tuple(value["forward_xyz"], Val(3), "camera.forward_xyz")
    up = _tuple(value["up_xyz"], Val(3), "camera.up_xyz")
    _validate_basis(forward, up, "camera forward/up basis")
    near = _finite_float32(value["near_plane_m"], "camera.near_plane_m")
    far = _finite_float32(value["far_plane_m"], "camera.far_plane_m")
    near >= MIN_NATIVE_DISTANCE_M && far > near && far <= MAX_NATIVE_COORDINATE_M ||
        throw(ProtocolError("malformed_packet", "camera planes are invalid"))
    width = _integer(value["width_px"], "camera.width_px")
    height = _integer(value["height_px"], "camera.height_px")
    _dimensions(width, height, "camera")
    return CameraPacket(
        camera_id,
        _parse_projection(_object(value["projection"], "camera.projection")),
        position,
        forward,
        up,
        near,
        far,
        UInt32(width),
        UInt32(height),
    )
end

function _parse_projection(value::JSON3.Object)::CameraProjectionValue
    kind = Symbol(_string(value["kind"], "camera.projection.kind"))
    return _parse_projection(Val(kind), value)
end

function _parse_projection(::Val{:orthographic}, value::JSON3.Object)::OrthographicProjection
    _exact_keys(value, ("kind", "span_m"), "camera.projection")
    span = _finite_float32(value["span_m"], "camera.projection.span_m")
    MIN_NATIVE_DISTANCE_M <= span <= MAX_NATIVE_COORDINATE_M ||
        throw(ProtocolError("malformed_packet", "orthographic camera span is invalid"))
    return OrthographicProjection(span)
end

function _parse_projection(::Val{:perspective}, value::JSON3.Object)::PerspectiveProjection
    _exact_keys(value, ("kind", "fov_y_degrees"), "camera.projection")
    fov = _finite_float32(value["fov_y_degrees"], "camera.projection.fov_y_degrees")
    1.0f0 <= fov <= 179.0f0 ||
        throw(ProtocolError("malformed_packet", "perspective camera field of view is invalid"))
    return PerspectiveProjection(fov)
end

function _parse_projection(::Val{kind}, ::JSON3.Object) where {kind}
    throw(ProtocolError("unsupported", "camera projection $(kind) is unsupported"))
end

function _validate_capture(value::JSON3.Object, camera::JSON3.Object)::Nothing
    _exact_keys(
        value,
        (
            "capture_id",
            "camera_id",
            "width_px",
            "height_px",
            "format",
            "include_depth",
            "deterministic",
        ),
        "capture",
    )
    _valid_id(_string(value["capture_id"], "capture.capture_id"), "capture_id")
    _string(value["camera_id"], "capture.camera_id") == _string(camera["camera_id"], "camera.camera_id") ||
        throw(ProtocolError("provenance", "capture camera is detached"))
    width = _integer(value["width_px"], "capture.width_px")
    height = _integer(value["height_px"], "capture.height_px")
    width == _integer(camera["width_px"], "camera.width_px") &&
        height == _integer(camera["height_px"], "camera.height_px") ||
        throw(ProtocolError("provenance", "capture dimensions are detached"))
    _dimensions(width, height, "capture")
    width * height * 4 <= MAX_CAPTURE_BYTES ||
        throw(ProtocolError("malformed_packet", "capture byte length exceeds the native frame bound"))
    _string(value["format"], "capture.format") == "rgba8_srgb" ||
        throw(ProtocolError("unsupported", "capture format is unsupported"))
    include_depth = _bool(value["include_depth"], "capture.include_depth")
    !include_depth ||
        throw(ProtocolError(
            "unsupported",
            "native certification currently promotes color-only captures; depth evidence is deferred",
        ))
    _bool(value["deterministic"], "capture.deterministic") ||
        throw(ProtocolError("unsupported", "capture must be deterministic"))
    return nothing
end

function _object(value, label::String)::JSON3.Object
    value isa JSON3.Object || throw(ProtocolError("malformed_packet", "$label must be an object"))
    return value
end

function _array(value, label::String)::JSON3.Array
    value isa JSON3.Array || throw(ProtocolError("malformed_packet", "$label must be an array"))
    return value
end

function _tuple(value, ::Val{N}, label::String)::NTuple{N,Float32} where {N}
    array = _array(value, label)
    length(array) == N || throw(ProtocolError("malformed_packet", "$label has the wrong arity"))
    return ntuple(index -> _finite_float32(array[index], "$label[$index]"), N)
end

function _bounded_values(values::NTuple{N,Float32}, ::String)::Bool where {N}
    return all(value -> abs(value) <= MAX_NATIVE_COORDINATE_M, values)
end

function _squared_norm(values::NTuple{N,Float32})::Float32 where {N}
    return sum(value * value for value in values)
end

function _validate_basis(
    forward::NTuple{3,Float32},
    up::NTuple{3,Float32},
    label::String,
)::Nothing
    forward_norm_squared = _squared_norm(forward)
    up_norm_squared = _squared_norm(up)
    isfinite(forward_norm_squared) && isfinite(up_norm_squared) &&
    forward_norm_squared > MIN_VECTOR_LENGTH_SQUARED && up_norm_squared > MIN_VECTOR_LENGTH_SQUARED ||
        throw(ProtocolError("malformed_packet", "$label contains a degenerate direction"))
    forward_norm = sqrt(forward_norm_squared)
    up_norm = sqrt(up_norm_squared)
    cosine = abs(sum((first / forward_norm) * (second / up_norm) for (first, second) in zip(forward, up)))
    isfinite(cosine) && cosine < 0.999f0 ||
        throw(ProtocolError("malformed_packet", "$label directions must not be collinear"))
    return nothing
end

function _exact_keys(
    value::JSON3.Object,
    expected::Union{Tuple,AbstractSet{String}},
    label::String,
)::Nothing
    length(value) == length(expected) && all(key -> haskey(value, key), expected) ||
        throw(ProtocolError("malformed_packet", "$label has an unexpected or missing field"))
    return nothing
end

function _validate_deformation_presence(schema::String, body::JSON3.Object)::Nothing
    has_deformation = haskey(body, "deformation")
    if schema == SCENE_PACKET_SCHEMA_V7 && !has_deformation
        throw(ProtocolError(
            "malformed_packet",
            "packet declares schema v7 but carries no deformation section; v7 exists to carry one",
        ))
    end
    if schema == SCENE_PACKET_SCHEMA && has_deformation
        throw(ProtocolError(
            "malformed_packet",
            "packet declares schema v6 but carries a deformation section; v7 is required to carry deformation",
        ))
    end
    return nothing
end

function _exact_keys(value::JSON3.Object, required::Tuple, optional::Tuple, label::String)::Nothing
    all(key -> haskey(value, key), required) ||
        throw(ProtocolError("malformed_packet", "$label has an unexpected or missing field"))
    allowed = (required..., optional...)
    all(key -> String(key) in allowed, keys(value)) ||
        throw(ProtocolError("malformed_packet", "$label has an unexpected or missing field"))
    optional_count = count(key -> haskey(value, key), optional)
    length(value) == length(required) + optional_count ||
        throw(ProtocolError("malformed_packet", "$label has an unexpected or missing field"))
    return nothing
end

function _string(value, label::String)::String
    value isa AbstractString || throw(ProtocolError("malformed_packet", "$label must be a string"))
    return String(value)
end

function _optional_string(
    object::JSON3.Object,
    key::String,
    label::String,
)::Union{Nothing,String}
    haskey(object, key) || return nothing
    value = object[key]
    value === nothing && return nothing
    result = _string(value, label)
    _valid_id(result, label)
    return result
end

function _bool(value, label::String)::Bool
    value isa Bool || throw(ProtocolError("malformed_packet", "$label must be a boolean"))
    return value
end

function _integer(value, label::String)::Int
    # Preserve JSON token types: Rust's integer fields reject floats and booleans.
    if value isa Integer && !(value isa Bool)
        try
            return Int(value)
        catch
            throw(ProtocolError("malformed_packet", "$label is outside the host integer range"))
        end
    end
    throw(ProtocolError("malformed_packet", "$label must be an integer"))
end

function _uint64(value, label::String)::UInt64
    if value isa Integer && !(value isa Bool)
        value >= 0 || throw(ProtocolError("malformed_packet", "$label must be non-negative"))
        try
            return UInt64(value)
        catch
            throw(ProtocolError("malformed_packet", "$label is outside UInt64"))
        end
    end
    throw(ProtocolError("malformed_packet", "$label must be an unsigned integer"))
end

function _uint32(value, label::String)::UInt32
    integer = _integer(value, label)
    0 <= integer <= typemax(UInt32) || throw(ProtocolError("malformed_packet", "$label is outside UInt32"))
    return UInt32(integer)
end

function _uint8(value, label::String)::UInt8
    integer = _integer(value, label)
    0 <= integer <= typemax(UInt8) || throw(ProtocolError("malformed_packet", "$label is outside UInt8"))
    return UInt8(integer)
end

function _finite_float32(value, label::String)::Float32
    number = if value isa Integer && !(value isa Bool)
        Float32(value)
    elseif value isa AbstractFloat
        Float32(value)
    else
        throw(ProtocolError("malformed_packet", "$label must be numeric"))
    end
    isfinite(number) || throw(ProtocolError("malformed_packet", "$label must be finite"))
    return number
end

function _valid_id(value::String, label::String)::Nothing
    # Match Rust String::len, which bounds UTF-8 bytes rather than codepoints.
    (!isempty(value) && ncodeunits(value) <= 256) ||
        throw(ProtocolError("malformed_packet", "$label is empty or too long"))
    any(isspace, value) && throw(ProtocolError("malformed_packet", "$label contains whitespace"))
    return nothing
end

function _valid_sha(value::String, label::String)::Nothing
    ncodeunits(value) == 71 && startswith(value, "sha256:") && all(isxdigit, value[8:end]) ||
        throw(ProtocolError("malformed_packet", "$label is not a sha256 digest"))
    return nothing
end

function _dimensions(width::Int, height::Int, label::String)::Nothing
    1 <= width <= MAX_CAPTURE_DIMENSION && 1 <= height <= MAX_CAPTURE_DIMENSION ||
        throw(ProtocolError("malformed_packet", "$label dimensions are outside bounds"))
    return nothing
end

function _sha(; body_value, path::String)
    return _string(body_value[path], path)
end

function _packet_content_sha256(value::JSON3.Object)::String
    return _sha256(Vector{UInt8}(codeunits(JSON3.write(value["body"]))))
end

function _sha256(bytes::Vector{UInt8})::String
    return "sha256:" * bytes2hex(sha256(bytes))
end

end
