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

struct MaterialTextureResources
    cache_key::String
    material_id::String
    albedo_texture::Lava.LavaTexture2D
    normal_texture::Lava.LavaTexture2D
    roughness_texture::Lava.LavaTexture2D
    occlusion_texture::Lava.LavaTexture2D
    emissive_texture::Lava.LavaTexture2D
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

mutable struct LavaBackend{C,Q,PP,SP,TP,OP,MP,TXP,DP,TSP,MSP,RP}
    context::C
    queue::Q
    probe_pipeline::PP
    sky_pipeline::SP
    terrain_pipeline::TP
    overlay_pipeline::OP
    mesh_pipeline::MP
    texture_pipeline::TXP
    depth_pipeline::DP
    terrain_shadow_pipeline::TSP
    mesh_shadow_pipeline::MSP
    resolve_pipeline::RP
    framebuffers::Dict{Tuple{Int,Int,Bool,Symbol},Lava.LavaFramebuffer}
    terrain_resources::Union{Nothing,TerrainResources}
    overlay_resources::Union{Nothing,OverlayResources}
    mesh_resources::Union{Nothing,MeshResources}
    shadow_mesh_resources::Union{Nothing,MeshResources}
    shadow_resources::Union{Nothing,ShadowResources}
    resolve_resources::Union{Nothing,ResolveResources}
    texture_resources::Union{Nothing,TextureProbeResources}
    material_texture_resources::Dict{String,MaterialTextureResources}
    upload_bytes::UInt64
    draw_calls::UInt64
    readback_bytes::UInt64
    pipeline_compilations::UInt64
    gpu_timestamp_supported::Bool
    probe_compiled::Bool
    terrain_compiled::Bool
    overlay_compiled::Bool
    sky_compiled::Bool
    mesh_compiled::Bool
    texture_compiled::Bool
    depth_compiled::Bool
    terrain_shadow_compiled::Bool
    mesh_shadow_compiled::Bool
    resolve_compiled::Bool
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
    environment_diffuse = _environment_color(
        surface_normal,
        environment_top,
        environment_horizon,
        environment_ground,
    )
    reflection = _reflect_vector(
        Vec4f(-view_vector[1], -view_vector[2], -view_vector[3], 0.0f0),
        surface_normal,
    )
    environment_specular = _environment_color(
        reflection,
        environment_top,
        environment_horizon,
        environment_ground,
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
        base_color[4] * (1.0f0 - weight + weight * sampled[4]),
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
    uv = Vec2f(normalized_x, normalized_z)
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

function _shadow_fragment()
    light_space = Lava.gfx_input(Vec4f, 0)
    depth = clamp(light_space[3], 0.0f0, 1.0f0)
    Lava.gfx_output(0, Vec4f(depth, depth, depth, 1.0f0))
    return nothing
end

function _shadow_depth(u::Float32, v::Float32)::Float32
    return Lava.sample_texture_2d(UInt32(5), u, v, UInt32(0))
end

function _shadow_visibility(light_space::Vec4f)::Float32
    inside =
        -1.0f0 <= light_space[1] <= 1.0f0 &&
            -1.0f0 <= light_space[2] <= 1.0f0 &&
            0.0f0 <= light_space[3] <= 1.0f0
    inside || return 1.0f0
    uv_x = light_space[1] * 0.5f0 + 0.5f0
    uv_y = light_space[2] * 0.5f0 + 0.5f0
    texel = 1.0f0 / 512.0f0
    bias = 0.0035f0
    depth = light_space[3] - bias
    visible = 0.0f0
    visible += depth <= _shadow_depth(uv_x - texel, uv_y - texel) ? 1.0f0 : 0.0f0
    visible += depth <= _shadow_depth(uv_x + texel, uv_y - texel) ? 1.0f0 : 0.0f0
    visible += depth <= _shadow_depth(uv_x - texel, uv_y + texel) ? 1.0f0 : 0.0f0
    visible += depth <= _shadow_depth(uv_x + texel, uv_y + texel) ? 1.0f0 : 0.0f0
    return 0.25f0 + 0.75f0 * (visible * 0.25f0)
end

function _resolve_vertex(texel_size::Vec2f, exposure::Float32)
    vertex_id = Lava.vertex_index() - Int32(1)
    x = Float32(Int32(vertex_id & Int32(1)) * 4 - 1)
    y = Float32(Int32((vertex_id >> Int32(1)) & Int32(1)) * 4 - 1)
    Lava.set_position!(Vec4f(x, y, 0.5f0, 1.0f0))
    Lava.gfx_output(0, Vec2f((x + 1.0f0) * 0.5f0, (y + 1.0f0) * 0.5f0))
    Lava.gfx_output(1, texel_size)
    Lava.gfx_output(2, Vec4f(exposure, 0.0f0, 0.0f0, 0.0f0))
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

function _resolve_fragment()
    uv = Lava.gfx_input(Vec2f, 0)
    texel_size = Lava.gfx_input(Vec2f, 1)
    exposure = Lava.gfx_input(Vec4f, 2)[1]
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
    Lava.gfx_output(0, _tone_map(hdr, exposure))
    return nothing
end

function _terrain_fragment()
    base_color = Lava.gfx_input(Vec4f, 0)
    normal = Lava.gfx_input(Vec4f, 1)
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
    tangent = Lava.gfx_input(Vec4f, 16)
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
    roughness_sample = _sample_texture(UInt32(2), uv)[1]
    occlusion_sample = _sample_texture(UInt32(3), uv)[1]
    emissive_sample = _sample_texture(UInt32(4), uv)
    lit_color = _material_response(
        _textured_color(base_color, uv, texture_enabled),
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
        _shadow_visibility(light_space),
        view_direction,
        roughness_sample,
        occlusion_sample,
        material[4],
        material_emissive,
        emissive_sample,
    )
    fogged_color = _apply_fog(
        lit_color,
        fog_color,
        _distance_between(world_position, camera_position),
        fog_density,
    )
    Lava.gfx_output(0, fogged_color)
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
        terrain_pipeline = GraphicsPipeline(
            ;
            vertex=_terrain_vertex,
            fragment=_terrain_fragment,
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
            terrain_pipeline,
            overlay_pipeline,
            mesh_pipeline,
            texture_pipeline,
            depth_pipeline,
            terrain_shadow_pipeline,
            mesh_shadow_pipeline,
            resolve_pipeline,
            Dict{Tuple{Int,Int,Bool,Symbol},Lava.LavaFramebuffer}(),
            nothing,
            nothing,
            nothing,
            nothing,
            nothing,
            nothing,
            nothing,
            Dict{String,MaterialTextureResources}(),
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

function _gpu_timestamp_capable(context::Lava.VkContext)::Bool
    try
        properties = Vulkan.get_physical_device_properties(context.physical_device)
        queue_properties = Vulkan.get_physical_device_queue_family_properties(
            context.physical_device,
        )[Int(context.queue_family_index) + 1]
        return isfinite(properties.limits.timestamp_period) &&
               properties.limits.timestamp_period > 0.0f0 &&
               queue_properties.timestamp_valid_bits > 0
    catch
        return false
    end
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
    return ntuple(
        index -> begin
            channel = Float32(value[index])
            isfinite(channel) && 0.0f0 <= channel <= 1.0f0 ||
                throw(AdapterError("invalid_capture", "RGBA channel is outside [0, 1]"))
            encoded = index == 4 ? channel : _linear_to_srgb(channel)
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
    bytes = Vector{UInt8}(undef, 4 * length(pixels))
    offset = 1
    # Lava readback is indexed as (x, y); the contract stores rows top-to-bottom.
    for row in axes(pixels, 2), column in axes(pixels, 1)
        rgba = _rgba8(pixels[column, row])
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
        width = max(width ÷ 2, UInt32(1))
        height = max(height ÷ 2, UInt32(1))
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

"""Create a Vulkan image with one mip level per validated CPU level, copy each
level through a single shared staging buffer, and return the resident texture.
The view spans every level so sampler LOD range 0..levels-1 is backed by real
data. Levels are RGBA32F — the adapter's proven sample format — until a
narrower-format seam is validated. Image, memory, and view are owned by the
texture; the pinned Lava rev owns destruction through finalizers, so nothing
is destroyed manually."""
function _lava_texture2d!(
    state::LavaBackend,
    levels::Vector{Matrix{NTuple{4,Float32}}};
    filter::Symbol=:linear,
    wrap::Symbol=:clamp,
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
        # A Julia column-major Matrix{NTuple{4,Float32}} is already an
        # interleaved RGBA32F buffer, so its pointer is the upload payload.
        # The matrix must be GC-preserved across the raw copy; a temporary
        # view whose pointer is the last use would be collectible mid-copy.
        # unsafe_copyto! requires both pointers to share an element type, so
        # the staging slice is reinterpreted to the matrix's own element type
        # and the copy length is in 16-byte RGBA elements (staging offsets are
        # 16-byte multiples, so alignment holds).
        GC.@preserve level unsafe_copyto!(
            Ptr{NTuple{4,Float32}}(write_ptr), pointer(level), length(level)
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

function _material_texture_resources!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    shadow::ShadowResources,
    material::WGEGraphics.MaterialPacket,
)
    cache = state.material_texture_resources
    if any(resource -> resource.cache_key != packet.content_sha256, values(cache))
        empty!(cache)
    end
    haskey(cache, material.material_id) && return cache[material.material_id]
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
    textures = map(
        texture_levels -> _lava_texture2d!(state, texture_levels; filter=:linear, wrap=:clamp),
        levels,
    )
    max_sampler_lod = UInt32(maximum(
        maximum(length(texture_levels) - 1, init=0) for texture_levels in levels
    ))
    sampler = _lod_sampler!(state, max_sampler_lod; filter=:linear, wrap=:clamp)
    bindings = Lava.bind_textures([
        textures[1] * sampler,
        textures[2] * sampler,
        textures[3] * sampler,
        textures[4] * sampler,
        textures[5] * sampler,
        shadow.texture * shadow.sampler,
    ])
    for texture_levels in levels
        state.upload_bytes += UInt64(sum(
            16 * size(level, 2) * size(level, 1) for level in texture_levels; init=0
        ))
    end
    created = MaterialTextureResources(
        packet.content_sha256,
        material.material_id,
        textures[1],
        textures[2],
        textures[3],
        textures[4],
        textures[5],
        sampler,
        bindings,
        max_sampler_lod,
    )
    cache[material.material_id] = created
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

function _shadow_up_hint(direction::Vec4f)::Vec4f
    return abs(direction[3]) < 0.9f0 ?
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
    material.alpha_mode == :opaque ||
        throw(AdapterError("unsupported_material", "native path requires opaque materials"))
    return nothing
end

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
            Vec4f(material.clearcoat, material.clearcoat_roughness, 0.0f0, 0.0f0) for _ in batch_instances
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

    prepare_started_ns = time_ns()
    prepare_gpu_timing_slot = _begin_gpu_pass_timing(state, GPU_PASS_TIMING_LABELS.prepare)
    resources = _terrain_resources!(state, packet)
    shadow_resources = _shadow_resources!(state, packet, lighting)
    texture_resources = _material_texture_resources!(state, packet, shadow_resources, material)
    visibility = _mesh_visibility(packet, camera_frame)
    mesh_resources = _mesh_resources!(state, packet, camera_frame, visibility)
    overlay_resources = _overlay_resources!(state, packet, camera_frame)
    _end_gpu_pass_timing!(state, prepare_gpu_timing_slot)
    prepare_time_us = _elapsed_us(prepare_started_ns)
    render_width = 2 * width
    render_height = 2 * height
    scene_framebuffer = _framebuffer!(state, render_width, render_height, true, :scene_hdr)
    scene_target = OffscreenTarget(scene_framebuffer)
    terrain_vertices = 6 * (packet.terrain.resolution - 1)^2

    scene_raster_started_ns = time_ns()
    scene_raster_gpu_timing_slot = _begin_gpu_pass_timing(state, GPU_PASS_TIMING_LABELS.scene_raster)
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
    draw!(
        state.queue,
        state.terrain_pipeline,
        scene_target,
        terrain_vertices;
        args=(
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
        ),
        descriptor_set_layout=texture_resources.bindings.layout,
        descriptor_set=texture_resources.bindings.set,
        clear_color=nothing,
    )
    state.draw_calls += 1
    if !state.terrain_compiled
        state.terrain_compiled = true
        state.pipeline_compilations += 1
    end
    if mesh_resources !== nothing
        for batch in mesh_resources.batches
            batch_material = _material(packet, batch.material_id)
            batch_texture_resources = _material_texture_resources!(
                state,
                packet,
                shadow_resources,
                batch_material,
            )
            batch_texture_enabled = _material_texture_enabled(batch_material)
            draw!(
                state.queue,
                state.mesh_pipeline,
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
                ),
                instances=batch.instance_count,
                descriptor_set_layout=batch_texture_resources.bindings.layout,
                descriptor_set=batch_texture_resources.bindings.set,
                clear_color=nothing,
                depth_clear=nothing,
            )
            state.draw_calls += 1
            if !state.mesh_compiled
                state.mesh_compiled = true
                state.pipeline_compilations += 1
            end
        end
    end
    _transition_color_to_sampled!(state, scene_framebuffer)
    _end_gpu_pass_timing!(state, scene_raster_gpu_timing_slot)
    scene_raster_time_us = _elapsed_us(scene_raster_started_ns)

    resolve_started_ns = time_ns()
    resolve_gpu_timing_slot = _begin_gpu_pass_timing(state, GPU_PASS_TIMING_LABELS.resolve)
    capture_framebuffer = _framebuffer!(state, width, height, false, :capture)
    capture_target = OffscreenTarget(capture_framebuffer)
    resolve_resources = _resolve_resources!(state, scene_framebuffer)
    draw!(
        state.queue,
        state.resolve_pipeline,
        capture_target,
        3;
        args=(
            Vec2f(1.0f0 / Float32(render_width), 1.0f0 / Float32(render_height)),
            packet.environment.exposure,
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
    capture_bytes = _capture_bytes(pixels)
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
            instance_count=visibility.instance_count,
            visible_instance_count=visibility.visible_instance_count,
            culled_instance_count=visibility.culled_instance_count,
            background_visible_instance_count=visibility.background_visible_count,
            background_culled_instance_count=visibility.background_culled_count,
            landmark_visible_instance_count=visibility.landmark_visible_count,
            landmark_culled_instance_count=visibility.landmark_culled_count,
            gameplay_critical_visible_instance_count=visibility.gameplay_critical_visible_count,
            gameplay_critical_culled_instance_count=visibility.gameplay_critical_culled_count,
            terrain_vertex_count=terrain_vertices,
            mesh_vertex_count=mesh_resources === nothing ? 0 : mesh_resources.vertex_count,
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

end
