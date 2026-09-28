module LavaAdapter

using Base64
using GeometryBasics: Vec2f, Vec4f
using Lava
using SHA
using Statistics
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

const ADAPTER_REVISION = "wge.lava-adapter/v1"
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
    packet_sha256::String
    heights::Lava.LavaArray{Float32,1}
    slopes::Lava.LavaArray{Float32,1}
    regions::Lava.LavaArray{UInt8,1}
    resolution::Int32
end

struct OverlayResources
    packet_sha256::String
    positions::Lava.LavaArray{Vec4f,1}
    colors::Lava.LavaArray{Vec4f,1}
    vertex_count::Int
end

struct MeshBatchResources
    positions::Lava.LavaArray{Vec4f,1}
    normals::Lava.LavaArray{Vec4f,1}
    translations::Lava.LavaArray{Vec4f,1}
    rotations::Lava.LavaArray{Vec4f,1}
    scales::Lava.LavaArray{Vec4f,1}
    colors::Lava.LavaArray{Vec4f,1}
    material_parameters::Lava.LavaArray{Vec4f,1}
    vertex_count::Int
    instance_count::Int
end

struct MeshResources
    packet_sha256::String
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
end

struct TextureProbeResources
    texture::Lava.LavaTexture2D{NTuple{4,Float32}}
    sampler::Lava.LavaSampler
    bindings::Lava.TextureBindings
end

struct MaterialTextureResources
    packet_sha256::String
    texture::Lava.LavaTexture2D{NTuple{4,Float32}}
    sampler::Lava.LavaSampler
    bindings::Lava.TextureBindings
end

struct DirectionalLighting
    direction::Vec4f
    color::Vec4f
    intensity::Float32
end

mutable struct LavaBackend{C,Q,PP,SP,TP,OP,MP,TXP,DP}
    context::C
    queue::Q
    probe_pipeline::PP
    sky_pipeline::SP
    terrain_pipeline::TP
    overlay_pipeline::OP
    mesh_pipeline::MP
    texture_pipeline::TXP
    depth_pipeline::DP
    framebuffers::Dict{Tuple{Int,Int,Bool},Lava.LavaFramebuffer}
    terrain_resources::Union{Nothing,TerrainResources}
    overlay_resources::Union{Nothing,OverlayResources}
    mesh_resources::Union{Nothing,MeshResources}
    texture_resources::Union{Nothing,TextureProbeResources}
    material_texture_resources::Union{Nothing,MaterialTextureResources}
    upload_bytes::UInt64
    draw_calls::UInt64
    readback_bytes::UInt64
    pipeline_compilations::UInt64
    probe_compiled::Bool
    terrain_compiled::Bool
    overlay_compiled::Bool
    sky_compiled::Bool
    mesh_compiled::Bool
    texture_compiled::Bool
    depth_compiled::Bool
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
    fog_color::Vec4f,
    exposure::Float32,
)
    vertex_id = Lava.vertex_index() - Int32(1)
    x = Float32(Int32(vertex_id & Int32(1)) * 4 - 1)
    y = Float32(Int32((vertex_id >> Int32(1)) & Int32(1)) * 4 - 1)
    Lava.set_position!(Vec4f(x, y, 0.99f0, 1.0f0))
    sky_weight = clamp((y + 1.0f0) * 0.5f0, 0.0f0, 1.0f0)
    horizon_weight = 1.0f0 - sky_weight
    sky_color = Vec4f(
        sky_horizon[1] * horizon_weight + sky_top[1] * sky_weight,
        sky_horizon[2] * horizon_weight + sky_top[2] * sky_weight,
        sky_horizon[3] * horizon_weight + sky_top[3] * sky_weight,
        1.0f0,
    )
    atmosphere = 0.08f0 * max(exposure, 0.01f0)
    Lava.gfx_output(
        0,
        _tone_map(
            Vec4f(
                sky_color[1] + fog_color[1] * atmosphere,
                sky_color[2] + fog_color[2] * atmosphere,
                sky_color[3] + fog_color[3] * atmosphere,
                1.0f0,
            ),
            exposure,
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

function _material_response(
    base_color::Vec4f,
    normal::Vec4f,
    light_direction::Vec4f,
    light_color::Vec4f,
    light_intensity::Float32,
    ambient_color::Vec4f,
    metallic::Float32,
    roughness::Float32,
    view_direction::Vec4f,
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
    surface_roughness = clamp(roughness, 0.045f0, 1.0f0)
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
    diffuse_scale = (1.0f0 - metalness) * 0.31830987f0
    direct_scale = light_intensity * normal_light
    ambient_diffuse_scale = (1.0f0 - metalness) * 0.7f0
    ambient_specular_scale = 0.15f0
    red = (
        (base_color[1] * diffuse_scale + fresnel_red * specular_scale) *
            light_color[1] * direct_scale +
            (base_color[1] * ambient_diffuse_scale + fresnel_red * ambient_specular_scale) *
            ambient_color[1]
    )
    green = (
        (base_color[2] * diffuse_scale + fresnel_green * specular_scale) *
            light_color[2] * direct_scale +
            (base_color[2] * ambient_diffuse_scale + fresnel_green * ambient_specular_scale) *
            ambient_color[2]
    )
    blue = (
        (base_color[3] * diffuse_scale + fresnel_blue * specular_scale) *
            light_color[3] * direct_scale +
            (base_color[3] * ambient_diffuse_scale + fresnel_blue * ambient_specular_scale) *
            ambient_color[3]
    )
    return Vec4f(max(red, 0.0f0), max(green, 0.0f0), max(blue, 0.0f0), base_color[4])
end

function _sample_texture(uv::Vec2f)::Vec4f
    return Vec4f(
        Lava.sample_texture_2d(UInt32(0), uv[1], uv[2], UInt32(0)),
        Lava.sample_texture_2d(UInt32(0), uv[1], uv[2], UInt32(1)),
        Lava.sample_texture_2d(UInt32(0), uv[1], uv[2], UInt32(2)),
        Lava.sample_texture_2d(UInt32(0), uv[1], uv[2], UInt32(3)),
    )
end

function _textured_color(
    base_color::Vec4f,
    uv::Vec2f,
    texture_enabled::Float32,
)::Vec4f
    sampled = _sample_texture(uv)
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

function _terrain_vertex(
    heights::Lava.LavaDeviceArray{Float32,1},
    slopes::Lava.LavaDeviceArray{Float32,1},
    regions::Lava.LavaDeviceArray{UInt8,1},
    resolution::Int32,
    width_m::Float32,
    length_m::Float32,
    span_m::Float32,
    aspect::Float32,
    base_color::Vec4f,
    metallic::Float32,
    roughness::Float32,
    light_direction::Vec4f,
    light_color::Vec4f,
    light_intensity::Float32,
    ambient_color::Vec4f,
    fog_color::Vec4f,
    fog_density::Float32,
    camera_position::Vec4f,
    exposure::Float32,
    texture_enabled::Float32,
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
    ndc_x = world_x / (span_m * aspect * 0.5f0)
    ndc_y = -world_z / (span_m * 0.5f0)
    depth = 0.5f0 - height * 0.001f0
    Lava.set_position!(Vec4f(ndc_x, ndc_y, depth, 1.0f0))
    slope_factor = min(max(slope * 0.8f0, 0.0f0), 1.0f0)
    region_tint = 0.82f0 + min(Float32(region) * 0.015f0, 0.18f0)
    shade = (1.0f0 - 0.45f0 * slope_factor) * region_tint
    terrain_color = Vec4f(
        base_color[1] * shade,
        base_color[2] * shade,
        base_color[3] * shade,
        base_color[4],
    )
    uv = Vec2f(normalized_x, normalized_z)
    Lava.gfx_output(0, terrain_color)
    Lava.gfx_output(1, _terrain_normal(heights, resolution, sample_x, sample_z, width_m, length_m))
    Lava.gfx_output(2, Vec4f(world_x, height, world_z, 1.0f0))
    Lava.gfx_output(3, uv)
    Lava.gfx_output(4, Vec4f(metallic, roughness, 0.0f0, 0.0f0))
    Lava.gfx_output(5, light_direction)
    Lava.gfx_output(6, light_color)
    Lava.gfx_output(7, Vec4f(light_intensity, fog_density, exposure, texture_enabled))
    Lava.gfx_output(8, ambient_color)
    Lava.gfx_output(9, fog_color)
    Lava.gfx_output(10, camera_position)
    return nothing
end

function _mesh_vertex(
    positions::Lava.LavaDeviceArray{Vec4f,1},
    normals::Lava.LavaDeviceArray{Vec4f,1},
    translations::Lava.LavaDeviceArray{Vec4f,1},
    rotations::Lava.LavaDeviceArray{Vec4f,1},
    scales::Lava.LavaDeviceArray{Vec4f,1},
    colors::Lava.LavaDeviceArray{Vec4f,1},
    material_parameters::Lava.LavaDeviceArray{Vec4f,1},
    span_m::Float32,
    aspect::Float32,
    light_direction::Vec4f,
    light_color::Vec4f,
    light_intensity::Float32,
    ambient_color::Vec4f,
    fog_color::Vec4f,
    fog_density::Float32,
    camera_position::Vec4f,
    exposure::Float32,
    texture_enabled::Float32,
)
    vertex_id = Lava.vertex_index()
    instance_id = Lava.instance_index()
    local_position = positions[vertex_id]
    local_normal = normals[vertex_id]
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
    normal = _rotate_vector(rotation, local_normal)
    ndc_x = world_position[1] / (span_m * aspect * 0.5f0)
    ndc_y = -world_position[3] / (span_m * 0.5f0)
    depth = 0.5f0 - world_position[2] * 0.001f0
    Lava.set_position!(Vec4f(ndc_x, ndc_y, depth, 1.0f0))
    uv = Vec2f(
        clamp(world_position[1] * 0.02f0 + 0.5f0, 0.0f0, 1.0f0),
        clamp(0.5f0 - world_position[3] * 0.02f0, 0.0f0, 1.0f0),
    )
    Lava.gfx_output(0, base_color)
    Lava.gfx_output(1, normal)
    Lava.gfx_output(2, world_position)
    Lava.gfx_output(3, uv)
    Lava.gfx_output(4, material)
    Lava.gfx_output(5, light_direction)
    Lava.gfx_output(6, light_color)
    Lava.gfx_output(7, Vec4f(light_intensity, fog_density, exposure, texture_enabled))
    Lava.gfx_output(8, ambient_color)
    Lava.gfx_output(9, fog_color)
    Lava.gfx_output(10, camera_position)
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
    ambient_color = Lava.gfx_input(Vec4f, 8)
    fog_color = Lava.gfx_input(Vec4f, 9)
    camera_position = Lava.gfx_input(Vec4f, 10)
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
    lit_color = _material_response(
        _textured_color(base_color, uv, texture_enabled),
        normal,
        light_direction,
        light_color,
        light_intensity,
        ambient_color,
        material[1],
        material[2],
        view_direction,
    )
    fogged_color = _apply_fog(
        lit_color,
        fog_color,
        sqrt(world_position[1] * world_position[1] + world_position[3] * world_position[3]),
        fog_density,
    )
    Lava.gfx_output(0, _tone_map(fogged_color, exposure))
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
            topology=LineList(),
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
            Dict{Tuple{Int,Int,Bool},Lava.LavaFramebuffer}(),
            nothing,
            nothing,
            nothing,
            nothing,
            nothing,
            UInt64(0),
            UInt64(0),
            UInt64(0),
            UInt64(0),
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

function _framebuffer!(state::LavaBackend, width_px::Int, height_px::Int, depth::Bool=false)::Lava.LavaFramebuffer
    key = (width_px, height_px, depth)
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

function _rgba8(value::NTuple{4,<:Real})::NTuple{4,UInt8}
    return ntuple(index -> UInt8(clamp(round(Int, Float32(value[index]) * 255.0f0), 0, 255)), 4)
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
        ),
    )
end

function render_probe(
    width_px::Integer=16,
    height_px::Integer=16,
    state::LavaBackend=backend(),
)
    width, height = _validate_dimensions(width_px, height_px)
    framebuffer = _framebuffer!(state, width, height)
    target = OffscreenTarget(framebuffer)
    draw!(state.queue, state.probe_pipeline, target, 3; clear_color=(0.0f0, 0.0f0, 0.0f0, 1.0f0))
    Lava.vk_flush!(state.context)
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
            readback_bytes=Int(state.readback_bytes),
            draw_calls=Int(state.draw_calls),
            dispatch_calls=0,
            pipeline_compilations=Int(state.pipeline_compilations),
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
    Lava.vk_flush!(state.context)
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

function _texture_matrix(
    bytes::Vector{UInt8},
    width::UInt32,
    height::UInt32,
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
        data[row, column] = ntuple(index -> Float32(bytes[offset+index-1]) / 255.0f0, 4)
        offset += 4
    end
    return data
end

function _material_texture_resources!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
)
    current = state.material_texture_resources
    current !== nothing && current.packet_sha256 == packet.packet_sha256 && return current
    texture_ids = Set(
        texture_id for material in packet.materials for texture_id in material.texture_ids
    )
    length(texture_ids) <= 1 ||
        throw(AdapterError("unsupported_material", "native path currently supports one shared albedo texture"))
    shared_texture_id = isempty(texture_ids) ? nothing : only(texture_ids)
    expected_ids = shared_texture_id === nothing ? String[] : String[shared_texture_id]
    for material in packet.materials
        material.texture_ids == expected_ids ||
            throw(AdapterError(
                "unsupported_material",
                "native path requires every material to use the same albedo texture profile",
            ))
    end

    if isempty(texture_ids)
        bytes = UInt8[255, 255, 255, 255]
        width = UInt32(1)
        height = UInt32(1)
    else
        texture_id = only(texture_ids)
        texture_index = findfirst(texture -> texture.texture_id == texture_id, packet.textures)
        texture_index === nothing &&
            throw(AdapterError("provenance", "material texture is absent from the packet"))
        texture = packet.textures[texture_index]
        texture.color_space in (:srgb, :linear) ||
            throw(AdapterError("unsupported_texture", "native path requires color albedo textures"))
        texture.payload === nothing &&
            throw(AdapterError("unsupported_texture", "native path requires an inline texture payload"))
        bytes = texture.payload::Vector{UInt8}
        width = texture.width_px
        height = texture.height_px
    end
    data = _texture_matrix(bytes, width, height)
    gpu_texture = Lava.LavaTexture2D(data; ctx=state.context, filter=:linear, wrap=:clamp)
    sampler = Lava.LavaSampler(ctx=state.context, filter=:linear, wrap=:clamp)
    bindings = Lava.bind_textures([gpu_texture * sampler])
    created = MaterialTextureResources(packet.packet_sha256, gpu_texture, sampler, bindings)
    state.material_texture_resources = created
    state.upload_bytes += UInt64(length(bytes))
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
    Lava.vk_flush!(state.context)
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

function _orthographic_span(projection::WGEGraphics.OrthographicProjection)::Float32
    span = projection.span_m
    span > 0.0f0 || throw(AdapterError("invalid_projection", "orthographic span must be positive"))
    return span
end

function _orthographic_span(::WGEGraphics.PerspectiveProjection)::Float32
    throw(AdapterError("unsupported_projection", "native terrain path requires orthographic projection"))
end

_orthographic_span(camera::WGEGraphics.CameraPacket) = _orthographic_span(camera.projection)

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

function _ambient_color(environment::WGEGraphics.EnvironmentPacket)::Vec4f
    return Vec4f(
        0.32f0 * environment.sky_horizon_rgb[1] + 0.12f0 * environment.sky_top_rgb[1],
        0.32f0 * environment.sky_horizon_rgb[2] + 0.12f0 * environment.sky_top_rgb[2],
        0.32f0 * environment.sky_horizon_rgb[3] + 0.12f0 * environment.sky_top_rgb[3],
        1.0f0,
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

function _terrain_resources!(state::LavaBackend, packet::WGEGraphics.GraphicsScenePacket)
    current = state.terrain_resources
    current !== nothing && current.packet_sha256 == packet.packet_sha256 && return current
    heights = Lava.LavaArray{Float32,1}(packet.terrain.heights_m; bq=state.queue)
    slopes = Lava.LavaArray{Float32,1}(packet.terrain.slope_grade; bq=state.queue)
    regions = Lava.LavaArray{UInt8,1}(packet.terrain.region_codes; bq=state.queue)
    state.upload_bytes += UInt64(
        sizeof(Float32) * (length(packet.terrain.heights_m) + length(packet.terrain.slope_grade)) +
            sizeof(UInt8) * length(packet.terrain.region_codes),
    )
    created = TerrainResources(packet.packet_sha256, heights, slopes, regions, Int32(packet.terrain.resolution))
    state.terrain_resources = created
    return created
end

function _project_point(
    camera::WGEGraphics.CameraPacket,
    point::NTuple{3,<:Real},
)::Vec4f
    span = _orthographic_span(camera)
    aspect = Float32(camera.width_px) / Float32(camera.height_px)
    x = Float32(point[1]) / (span * aspect * 0.5f0)
    y = -Float32(point[3]) / (span * 0.5f0)
    depth = 0.45f0 - Float32(point[2]) * 0.001f0
    return Vec4f(x, y, depth, 1.0f0)
end

function _append_segment!(positions::Vector{Vec4f}, colors::Vector{Vec4f}, first::Vec4f, last::Vec4f, color::Vec4f)
    push!(positions, first)
    push!(positions, last)
    push!(colors, color)
    push!(colors, color)
    return nothing
end

function _overlay_color(overlay::WGEGraphics.OverlayPacket)::Vec4f
    return Vec4f(overlay.color_rgba...)
end

function _append_overlay!(
    positions::Vector{Vec4f},
    colors::Vector{Vec4f},
    overlay::WGEGraphics.PointOverlay,
    camera::WGEGraphics.CameraPacket,
)
    center = overlay.position_xyz_m
    radius = overlay.radius_m
    color = _overlay_color(overlay)
    _append_segment!(
        positions,
        colors,
        _project_point(camera, (center[1] - radius, center[2], center[3])),
        _project_point(camera, (center[1] + radius, center[2], center[3])),
        color,
    )
    _append_segment!(
        positions,
        colors,
        _project_point(camera, (center[1], center[2], center[3] - radius)),
        _project_point(camera, (center[1], center[2], center[3] + radius)),
        color,
    )
    return nothing
end

function _append_overlay!(
    positions::Vector{Vec4f},
    colors::Vector{Vec4f},
    overlay::WGEGraphics.CircleOverlay,
    camera::WGEGraphics.CameraPacket,
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
        _append_segment!(positions, colors, _project_point(camera, first), _project_point(camera, last), color)
    end
    return nothing
end

function _append_overlay!(
    positions::Vector{Vec4f},
    colors::Vector{Vec4f},
    overlay::WGEGraphics.PolylineOverlay,
    camera::WGEGraphics.CameraPacket,
)
    color = _overlay_color(overlay)
    for index in 1:(length(overlay.points_xyz_m)-1)
        first = _project_point(camera, overlay.points_xyz_m[index])
        last = _project_point(camera, overlay.points_xyz_m[index+1])
        _append_segment!(positions, colors, first, last, color)
    end
    return nothing
end

function _instance_visible(
    packet::WGEGraphics.GraphicsScenePacket,
    instance::WGEGraphics.InstancePacket,
)
    center = _project_point(packet.camera, instance.transform.translation_xyz_m)
    span = _orthographic_span(packet.camera)
    aspect = Float32(packet.camera.width_px) / Float32(packet.camera.height_px)
    radius = max(instance.transform.scale_xyz...)
    margin_y = radius / (span * 0.5f0)
    margin_x = radius / (span * aspect * 0.5f0)
    return -1.0f0 - margin_x <= center[1] <= 1.0f0 + margin_x &&
        -1.0f0 - margin_y <= center[2] <= 1.0f0 + margin_y
end

function _mesh_visibility(packet::WGEGraphics.GraphicsScenePacket)::MeshVisibility
    instance_count = length(packet.instances)
    visible_instance_count = count(
        instance -> _instance_visible(packet, instance),
        packet.instances,
    )
    return MeshVisibility(
        instance_count,
        visible_instance_count,
        instance_count - visible_instance_count,
    )
end

function _mesh_resources!(
    state::LavaBackend,
    packet::WGEGraphics.GraphicsScenePacket,
    visibility::MeshVisibility=_mesh_visibility(packet),
)::Union{Nothing,MeshResources}
    current = state.mesh_resources
    current !== nothing && current.packet_sha256 == packet.packet_sha256 && return current
    groups = Dict{Tuple{String,String},Vector{WGEGraphics.InstancePacket}}()
    for instance in packet.instances
        _instance_visible(packet, instance) || continue
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
        for index in mesh_packet.indices
            vertex = Int(index) + 1
            push!(positions, Vec4f(mesh_packet.positions_m[vertex]..., 0.0f0))
            push!(normals, Vec4f(mesh_packet.normals[vertex]..., 0.0f0))
        end
        batch_instances = groups[(mesh_id, material_id)]
        translations = Vec4f[
            Vec4f(instance.transform.translation_xyz_m..., 0.0f0) for instance in batch_instances
        ]
        rotations = Vec4f[Vec4f(instance.transform.rotation_xyzw...) for instance in batch_instances]
        scales = Vec4f[Vec4f(instance.transform.scale_xyz..., 0.0f0) for instance in batch_instances]
        colors = Vec4f[Vec4f(material.base_color_rgba...) for _ in batch_instances]
        material_parameters = Vec4f[
            Vec4f(material.metallic, material.roughness, 0.0f0, 0.0f0) for _ in batch_instances
        ]
        gpu_positions = Lava.LavaArray{Vec4f,1}(positions; bq=state.queue)
        gpu_normals = Lava.LavaArray{Vec4f,1}(normals; bq=state.queue)
        gpu_translations = Lava.LavaArray{Vec4f,1}(translations; bq=state.queue)
        gpu_rotations = Lava.LavaArray{Vec4f,1}(rotations; bq=state.queue)
        gpu_scales = Lava.LavaArray{Vec4f,1}(scales; bq=state.queue)
        gpu_colors = Lava.LavaArray{Vec4f,1}(colors; bq=state.queue)
        gpu_material_parameters = Lava.LavaArray{Vec4f,1}(material_parameters; bq=state.queue)
        state.upload_bytes += UInt64(
            sizeof(Vec4f) *
                (length(positions) + length(normals) + length(translations) +
                length(rotations) + length(scales) + length(colors) +
                length(material_parameters)),
        )
        push!(
            batches,
            MeshBatchResources(
                gpu_positions,
                gpu_normals,
                gpu_translations,
                gpu_rotations,
                gpu_scales,
                gpu_colors,
                gpu_material_parameters,
                length(positions),
                length(batch_instances),
            ),
        )
        total_vertices += length(positions)
    end
    created = MeshResources(
        packet.packet_sha256,
        batches,
        visibility.instance_count,
        visibility.visible_instance_count,
        visibility.culled_instance_count,
        total_vertices,
    )
    state.mesh_resources = created
    return created
end

function _overlay_resources!(state::LavaBackend, packet::WGEGraphics.GraphicsScenePacket)
    current = state.overlay_resources
    current !== nothing && current.packet_sha256 == packet.packet_sha256 && return current
    positions = Vec4f[]
    colors = Vec4f[]
    for overlay in packet.overlays
        _append_overlay!(positions, colors, overlay, packet.camera)
    end
    isempty(positions) && return nothing
    gpu_positions = Lava.LavaArray{Vec4f,1}(positions; bq=state.queue)
    gpu_colors = Lava.LavaArray{Vec4f,1}(colors; bq=state.queue)
    state.upload_bytes += UInt64(sizeof(Vec4f) * (length(positions) + length(colors)))
    created = OverlayResources(packet.packet_sha256, gpu_positions, gpu_colors, length(positions))
    state.overlay_resources = created
    return created
end

function _pixel_luminance(pixel::NTuple{4,<:Real})
    return 0.2126 * Float64(pixel[1]) + 0.7152 * Float64(pixel[2]) + 0.0722 * Float64(pixel[3])
end

function _distinct_colors(pixels)
    return length(Set((rgba[1], rgba[2], rgba[3]) for rgba in (_rgba8(pixel) for pixel in pixels)))
end

function _role_pixels(pixels, overlays, role::Symbol)
    colors = Set{NTuple{4,UInt8}}()
    for overlay in overlays
        overlay.role == role && push!(colors, _rgba8(overlay.color_rgba))
    end
    isempty(colors) && return 0
    return count(pixel -> _rgba8(pixel) in colors, pixels)
end

function _scene_measurements(
    pixels::AbstractMatrix{<:NTuple{4,<:Real}},
    packet::WGEGraphics.GraphicsScenePacket,
)
    luminance = Float64[_pixel_luminance(pixel) for pixel in pixels]
    return (
        terrain_luminance_stddev=std(luminance; corrected=false),
        distinct_terrain_colors=_distinct_colors(pixels),
        route_visible_pixels=_role_pixels(pixels, packet.overlays, :route),
        player_spawn_visible_pixels=_role_pixels(pixels, packet.overlays, :player_spawn),
        opponent_spawn_visible_pixels=_role_pixels(pixels, packet.overlays, :opponent_spawn),
        encounter_visible_pixels=_role_pixels(pixels, packet.overlays, :encounter),
        objective_visible_pixels=_role_pixels(pixels, packet.overlays, :objective),
    )
end

function render_scene(
    packet::WGEGraphics.GraphicsScenePacket,
    state::LavaBackend=backend(),
)
    started_ns = time_ns()
    width, height = _validate_dimensions(packet.width_px, packet.height_px)
    material = _terrain_material(packet)
    for material_intent in packet.materials
        _validate_material(material_intent)
    end
    ambient_color = _ambient_color(packet.environment)
    texture_enabled = _material_texture_enabled(material)
    camera_position = Vec4f(packet.camera.position_xyz_m..., 0.0f0)
    lighting = _lighting(packet)
    span = _orthographic_span(packet.camera)
    aspect = Float32(width) / Float32(height)
    texture_resources = _material_texture_resources!(state, packet)
    resources = _terrain_resources!(state, packet)
    visibility = _mesh_visibility(packet)
    mesh_resources = _mesh_resources!(state, packet, visibility)
    overlay_resources = _overlay_resources!(state, packet)
    framebuffer = _framebuffer!(state, width, height, true)
    target = OffscreenTarget(framebuffer)
    terrain_vertices = 6 * (packet.terrain.resolution - 1)^2
    draw!(
        state.queue,
        state.sky_pipeline,
        target,
        3;
        args=(
            Vec4f(packet.environment.sky_top_rgb..., 1.0f0),
            Vec4f(packet.environment.sky_horizon_rgb..., 1.0f0),
            Vec4f(packet.environment.fog_color_rgb..., 1.0f0),
            packet.environment.exposure,
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
        target,
        terrain_vertices;
        args=(
            resources.heights,
            resources.slopes,
            resources.regions,
            resources.resolution,
            Float32(packet.terrain.width_m),
            Float32(packet.terrain.length_m),
            span,
            aspect,
            Vec4f(material.base_color_rgba...),
            material.metallic,
            material.roughness,
            lighting.direction,
            lighting.color,
            Float32(lighting.intensity),
            ambient_color,
            Vec4f(packet.environment.fog_color_rgb..., 1.0f0),
            packet.environment.fog_density,
            camera_position,
            packet.environment.exposure,
            texture_enabled,
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
            draw!(
                state.queue,
                state.mesh_pipeline,
                target,
                batch.vertex_count;
                args=(
                    batch.positions,
                    batch.normals,
                    batch.translations,
                    batch.rotations,
                    batch.scales,
                    batch.colors,
                    batch.material_parameters,
                    span,
                    aspect,
                    lighting.direction,
                    lighting.color,
                    Float32(lighting.intensity),
                    ambient_color,
                    Vec4f(packet.environment.fog_color_rgb..., 1.0f0),
                    packet.environment.fog_density,
                    camera_position,
                    packet.environment.exposure,
                    texture_enabled,
                ),
                instances=batch.instance_count,
                descriptor_set_layout=texture_resources.bindings.layout,
                descriptor_set=texture_resources.bindings.set,
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
    if overlay_resources !== nothing
        draw!(
            state.queue,
            state.overlay_pipeline,
            target,
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
    Lava.vk_flush!(state.context)
    pixels = readback_framebuffer(framebuffer)
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
        measurements=_scene_measurements(pixels, packet),
        telemetry=(
            upload_bytes=Int(state.upload_bytes),
            readback_bytes=Int(state.readback_bytes),
            draw_calls=Int(state.draw_calls),
            dispatch_calls=0,
            pipeline_compilations=Int(state.pipeline_compilations),
            instance_count=visibility.instance_count,
            visible_instance_count=visibility.visible_instance_count,
            culled_instance_count=visibility.culled_instance_count,
            terrain_vertex_count=terrain_vertices,
            mesh_vertex_count=mesh_resources === nothing ? 0 : mesh_resources.vertex_count,
            frame_time_us=Int(cld(time_ns() - started_ns, UInt64(1_000))),
        ),
    )
end

end
