module WGEGraphics

using JSON3
using SHA

export GraphicsScenePacket, ProtocolError, validate_scene_packet, packet_summary

const SCENE_PACKET_SCHEMA = "wge.graphics-scene-packet/v1"
const MAX_PACKET_ELEMENTS = 16 * 1024 * 1024
const MAX_CAPTURE_DIMENSION = 8192
const MAX_CAPTURE_BYTES = 32 * 1024 * 1024

struct ProtocolError <: Exception
    code::String
    detail::String
end

Base.showerror(io::IO, error::ProtocolError) = print(io, error.code, ": ", error.detail)

struct TerrainPacket
    terrain_id::String
    width_m::Float32
    length_m::Float32
    resolution::Int
    material_id::String
    heights_m::Vector{Float32}
    slope_grade::Vector{Float32}
    region_codes::Vector{UInt8}
end

abstract type CameraProjection end

struct OrthographicProjection <: CameraProjection
    span_m::Float32
end

struct PerspectiveProjection <: CameraProjection
    fov_y_degrees::Float32
end

const CameraProjectionValue = Union{OrthographicProjection, PerspectiveProjection}

struct CameraPacket
    camera_id::String
    projection::CameraProjectionValue
    position_xyz_m::NTuple{3, Float32}
    forward_xyz::NTuple{3, Float32}
    up_xyz::NTuple{3, Float32}
    near_plane_m::Float32
    far_plane_m::Float32
    width_px::UInt32
    height_px::UInt32
end

struct MaterialPacket
    material_id::String
    base_color_rgba::NTuple{4, Float32}
    metallic::Float32
    roughness::Float32
    alpha_mode::Symbol
    texture_ids::Vector{String}
end

struct TexturePacket
    texture_id::String
    source_artifact_id::String
    sha256::String
    width_px::UInt32
    height_px::UInt32
    mip_levels::UInt32
    color_space::Symbol
end

struct MeshPacket
    mesh_id::String
    positions_m::Vector{NTuple{3, Float32}}
    normals::Vector{NTuple{3, Float32}}
    indices::Vector{UInt32}
    material_id::String
end

struct TransformPacket
    translation_xyz_m::NTuple{3, Float32}
    rotation_xyzw::NTuple{4, Float32}
    scale_xyz::NTuple{3, Float32}
end

struct InstancePacket
    instance_id::String
    mesh_id::String
    material_id::String
    transform::TransformPacket
end

struct DirectionalLightPacket
    direction_xyz::NTuple{3, Float32}
end

struct PointLightPacket
    position_xyz_m::NTuple{3, Float32}
    range_m::Float32
end

const LightKindPacket = Union{DirectionalLightPacket, PointLightPacket}

struct LightPacket
    light_id::String
    kind::LightKindPacket
    color_rgb::NTuple{3, Float32}
    intensity::Float32
end

abstract type OverlayPacket end

struct PointOverlay <: OverlayPacket
    marker_id::String
    role::Symbol
    position_xyz_m::NTuple{3, Float32}
    radius_m::Float32
    color_rgba::NTuple{4, Float32}
end

struct CircleOverlay <: OverlayPacket
    marker_id::String
    role::Symbol
    center_xyz_m::NTuple{3, Float32}
    radius_m::Float32
    color_rgba::NTuple{4, Float32}
end

struct PolylineOverlay <: OverlayPacket
    marker_id::String
    role::Symbol
    points_xyz_m::Vector{NTuple{3, Float32}}
    thickness_m::Float32
    color_rgba::NTuple{4, Float32}
end

const OverlayValue = Union{PointOverlay, CircleOverlay, PolylineOverlay}

struct GraphicsScenePacket
    packet_sha256::String
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
    overlays::Vector{OverlayValue}
    capture_id::String
    width_px::UInt32
    height_px::UInt32
    include_depth::Bool
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
    _exact_keys(value, Set(("body", "packet_sha256")), "packet")
    body = _object(value["body"], "body")
    _exact_keys(
        body,
        Set((
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
            "overlays",
            "capture",
        )),
        "packet.body",
    )
    _string(body["schema_version"], "packet.body.schema_version") == SCENE_PACKET_SCHEMA ||
        throw(ProtocolError("unsupported_schema", "scene packet schema is unsupported"))
    packet_sha256 = _sha(body_value=value, path="packet_sha256")
    _valid_sha(packet_sha256, "packet_sha256")
    _valid_id(_string(body["packet_id"], "packet.body.packet_id"), "packet_id")
    world_id = _string(body["world_artifact_id"], "packet.body.world_artifact_id")
    spatial_sha = _string(body["spatial_fields_sha256"], "packet.body.spatial_fields_sha256")
    _valid_id(world_id, "world_artifact_id")
    _valid_sha(_string(body["world_artifact_sha256"], "world_artifact_sha256"), "world_artifact_sha256")
    _valid_sha(spatial_sha, "spatial_fields_sha256")
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
    overlays = _parse_overlays(body["overlays"])
    capture = _object(body["capture"], "capture")
    _validate_capture(capture, body["camera"])

    return GraphicsScenePacket(
        packet_sha256,
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
        Vector{OverlayValue}(overlays),
        _string(capture["capture_id"], "capture_id"),
        UInt32(_integer(capture["width_px"], "capture.width_px")),
        UInt32(_integer(capture["height_px"], "capture.height_px")),
        _bool(capture["include_depth"], "capture.include_depth"),
    )
end

function _parse_terrain(value::JSON3.Object, materials::Vector{MaterialPacket})::TerrainPacket
    _exact_keys(
        value,
        Set((
            "terrain_id",
            "width_m",
            "length_m",
            "resolution",
            "material_id",
            "heights_m",
            "slope_grade",
            "region_codes",
        )),
        "terrain",
    )
    resolution = _integer(value["resolution"], "terrain.resolution")
    resolution >= 3 || throw(ProtocolError("malformed_packet", "terrain resolution is too small"))
    resolution * resolution <= MAX_PACKET_ELEMENTS ||
        throw(ProtocolError("malformed_packet", "terrain resolution exceeds packet bound"))
    width = _finite_float32(value["width_m"], "terrain.width_m")
    length = _finite_float32(value["length_m"], "terrain.length_m")
    width > 0.0f0 && length > 0.0f0 ||
        throw(ProtocolError("malformed_packet", "terrain dimensions must be positive"))
    terrain_id = _string(value["terrain_id"], "terrain.terrain_id")
    material_id = _string(value["material_id"], "terrain.material_id")
    _valid_id(terrain_id, "terrain_id")
    _valid_id(material_id, "terrain.material_id")
    heights = _parse_buffer(value["heights_m"], resolution * resolution, "terrain.heights_m", :f32)
    slope = _parse_buffer(value["slope_grade"], resolution * resolution, "terrain.slope_grade", :f32)
    regions = _parse_buffer(value["region_codes"], resolution * resolution, "terrain.region_codes", :u8)
    any(material.material_id == material_id for material in materials) ||
        throw(ProtocolError("provenance", "terrain references an unknown material"))
    return TerrainPacket(terrain_id, width, length, resolution, material_id, heights, slope, regions)
end

function _parse_buffer(
    value::JSON3.Object,
    expected_count::Int,
    label::String,
    expected_encoding::Symbol,
)
    object = _object(value, label)
    actual_keys = Set(String(key) for key in keys(object))
    required_keys = Set(("buffer_id", "byte_length", "count", "stride_bytes", "sha256", "payload"))
    allowed_keys = union(required_keys, Set(("source_artifact_id",)))
    (actual_keys == required_keys || actual_keys == allowed_keys) ||
        throw(ProtocolError("malformed_packet", "$label has an unexpected or missing field"))
    buffer_id = _string(object["buffer_id"], "$label.buffer_id")
    _valid_id(buffer_id, "$label.buffer_id")
    count = _integer(object["count"], "$label.count")
    count == expected_count || throw(ProtocolError("malformed_packet", "$label count does not match terrain"))
    payload = _object(object["payload"], "$label.payload")
    _exact_keys(payload, Set(("encoding", "values")), "$label.payload")
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
    return Float32[_finite_float32(item, "$label.values[$index]") for (index, item) in enumerate(array)]
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
                bits & 0xff,
                (bits >> 8) & 0xff,
                (bits >> 16) & 0xff,
                (bits >> 24) & 0xff,
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
                value & 0xff,
                (value >> 8) & 0xff,
                (value >> 16) & 0xff,
                (value >> 24) & 0xff,
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
        _exact_keys(
            object,
            Set(("material_id", "base_color_rgba", "metallic", "roughness", "alpha_mode", "texture_ids")),
            "material",
        )
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
        alpha_mode = Symbol(_string(object["alpha_mode"], "material.alpha_mode"))
        alpha_mode in (:opaque, :mask, :blend) ||
            throw(ProtocolError("unsupported", "material alpha mode is unsupported"))
        texture_ids = String[]
        for item in _array(object["texture_ids"], "material.texture_ids")
            texture_id = _string(item, "material.texture_id")
            _valid_id(texture_id, "material.texture_id")
            push!(texture_ids, texture_id)
        end
        push!(materials, MaterialPacket(id, color, metallic, roughness, alpha_mode, texture_ids))
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
        _exact_keys(
            object,
            Set(("texture_id", "source_artifact_id", "sha256", "width_px", "height_px", "mip_levels", "color_space")),
            "texture",
        )
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
        color_space = Symbol(_string(object["color_space"], "texture.color_space"))
        color_space in (:srgb, :linear, :normal_map, :data) ||
            throw(ProtocolError("unsupported", "texture color space is unsupported"))
        push!(
            textures,
            TexturePacket(id, source_artifact_id, sha256, width, height, mip_levels, color_space),
        )
    end
    return textures
end

function _validate_material_texture_links(materials::Vector{MaterialPacket}, textures::Vector{TexturePacket})
    isempty(materials) && throw(ProtocolError("malformed_packet", "packet needs a material"))
    texture_ids = Set(texture.texture_id for texture in textures)
    for material in materials, texture_id in material.texture_ids
        texture_id in texture_ids ||
            throw(ProtocolError("provenance", "material references unknown texture $texture_id"))
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
            Set(("mesh_id", "positions_m", "normals", "indices", "material_id")),
            "mesh",
        )
        id = _string(object["mesh_id"], "mesh.mesh_id")
        _valid_id(id, "mesh_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate mesh $id"))
        push!(ids, id)
        positions = NTuple{3, Float32}[
            _tuple(position, Val(3), "mesh.positions_m") for
            position in _array(object["positions_m"], "mesh.positions_m")
        ]
        normals = NTuple{3, Float32}[
            _tuple(normal, Val(3), "mesh.normals") for
            normal in _array(object["normals"], "mesh.normals")
        ]
        !isempty(positions) && length(positions) == length(normals) ||
            throw(ProtocolError("malformed_packet", "mesh position and normal counts differ"))
        indices = UInt32[
            _uint32(index, "mesh.indices") for index in _array(object["indices"], "mesh.indices")
        ]
        isempty(positions) && throw(ProtocolError("malformed_packet", "mesh needs positions"))
        !isempty(indices) && length(indices) % 3 == 0 ||
            throw(ProtocolError("malformed_packet", "mesh indices must form triangles"))
        all(index -> Int(index) < length(positions), indices) ||
            throw(ProtocolError("malformed_packet", "mesh index is out of range"))
        material_id = _string(object["material_id"], "mesh.material_id")
        _valid_id(material_id, "mesh.material_id")
        material_id in material_ids ||
            throw(ProtocolError("provenance", "mesh references unknown material $material_id"))
        push!(meshes, MeshPacket(id, positions, normals, indices, material_id))
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
        _exact_keys(object, Set(("instance_id", "mesh_id", "material_id", "transform")), "instance")
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
        transform = _object(object["transform"], "instance.transform")
        _exact_keys(transform, Set(("translation_xyz_m", "rotation_xyzw", "scale_xyz")), "instance.transform")
        translation = _tuple(transform["translation_xyz_m"], Val(3), "instance.transform.translation_xyz_m")
        rotation = _tuple(transform["rotation_xyzw"], Val(4), "instance.transform.rotation_xyzw")
        scale = _tuple(transform["scale_xyz"], Val(3), "instance.transform.scale_xyz")
        sum(value * value for value in rotation) > eps(Float32) ||
            throw(ProtocolError("malformed_packet", "instance rotation is degenerate"))
        all(value -> value > 0.0f0, scale) ||
            throw(ProtocolError("malformed_packet", "instance scale must be positive"))
        push!(instances, InstancePacket(id, mesh_id, material_id, TransformPacket(translation, rotation, scale)))
    end
    return instances
end

function _parse_lights(value::JSON3.Array)::Vector{LightPacket}
    array = _array(value, "lights")
    isempty(array) && throw(ProtocolError("malformed_packet", "packet needs a light"))
    ids = Set{String}()
    lights = LightPacket[]
    sizehint!(lights, length(array))
    for light in array
        object = _object(light, "light")
        _exact_keys(object, Set(("light_id", "kind", "color_rgb", "intensity")), "light")
        id = _string(object["light_id"], "light.light_id")
        _valid_id(id, "light_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate light $id"))
        push!(ids, id)
        color = _tuple(object["color_rgb"], Val(3), "light.color_rgb")
        all(channel -> channel >= 0.0f0, color) ||
            throw(ProtocolError("malformed_packet", "light color is negative"))
        intensity = _finite_float32(object["intensity"], "light.intensity")
        intensity >= 0.0f0 || throw(ProtocolError("malformed_packet", "light intensity is negative"))
        push!(lights, LightPacket(id, _parse_light_kind(_object(object["kind"], "light.kind")), color, intensity))
    end
    return lights
end

function _parse_light_kind(value::JSON3.Object)::LightKindPacket
    kind = Symbol(_string(value["kind"], "light.kind.kind"))
    return _parse_light_kind(Val(kind), value)
end

function _parse_light_kind(::Val{:directional}, value::JSON3.Object)::DirectionalLightPacket
    _exact_keys(value, Set(("kind", "direction_xyz")), "light.kind")
    return DirectionalLightPacket(_tuple(value["direction_xyz"], Val(3), "light.kind.direction_xyz"))
end

function _parse_light_kind(::Val{:point}, value::JSON3.Object)::PointLightPacket
    _exact_keys(value, Set(("kind", "position_xyz_m", "range_m")), "light.kind")
    range = _finite_float32(value["range_m"], "light.kind.range_m")
    range > 0.0f0 || throw(ProtocolError("malformed_packet", "point light range is invalid"))
    return PointLightPacket(
        _tuple(value["position_xyz_m"], Val(3), "light.kind.position_xyz_m"),
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

function _overlay_color(value::JSON3.Object)::NTuple{4, Float32}
    color = _tuple(value["color_rgba"], Val(4), "overlay.color_rgba")
    all(channel -> 0.0f0 <= channel <= 1.0f0, color) ||
        throw(ProtocolError("malformed_packet", "overlay color is outside [0, 1]"))
    return color
end

function _parse_overlay(::Val{:point}, value::JSON3.Object)::PointOverlay
    _exact_keys(
        value,
        Set(("kind", "marker_id", "role", "position_xyz_m", "radius_m", "color_rgba")),
        "overlay",
    )
    marker_id, role = _overlay_header(value)
    radius = _finite_float32(value["radius_m"], "overlay.radius_m")
    radius > 0.0f0 || throw(ProtocolError("malformed_packet", "overlay radius is invalid"))
    return PointOverlay(
        marker_id,
        role,
        _tuple(value["position_xyz_m"], Val(3), "overlay.position_xyz_m"),
        radius,
        _overlay_color(value),
    )
end

function _parse_overlay(::Val{:circle}, value::JSON3.Object)::CircleOverlay
    _exact_keys(
        value,
        Set(("kind", "marker_id", "role", "center_xyz_m", "radius_m", "color_rgba")),
        "overlay",
    )
    marker_id, role = _overlay_header(value)
    radius = _finite_float32(value["radius_m"], "overlay.radius_m")
    radius > 0.0f0 || throw(ProtocolError("malformed_packet", "overlay radius is invalid"))
    return CircleOverlay(
        marker_id,
        role,
        _tuple(value["center_xyz_m"], Val(3), "overlay.center_xyz_m"),
        radius,
        _overlay_color(value),
    )
end

function _parse_overlay(::Val{:polyline}, value::JSON3.Object)::PolylineOverlay
    _exact_keys(
        value,
        Set(("kind", "marker_id", "role", "points_xyz_m", "thickness_m", "color_rgba")),
        "overlay",
    )
    marker_id, role = _overlay_header(value)
    point_values = _array(value["points_xyz_m"], "overlay.points_xyz_m")
    length(point_values) >= 2 || throw(ProtocolError("malformed_packet", "polyline needs two points"))
    points = NTuple{3, Float32}[_tuple(point, Val(3), "overlay point") for point in point_values]
    thickness = _finite_float32(value["thickness_m"], "overlay.thickness_m")
    thickness > 0.0f0 || throw(ProtocolError("malformed_packet", "overlay thickness is invalid"))
    return PolylineOverlay(marker_id, role, points, thickness, _overlay_color(value))
end

function _parse_overlay(::Val{kind}, ::JSON3.Object) where {kind}
    throw(ProtocolError("unsupported", "overlay kind $(kind) is unsupported"))
end

function _validate_coordinate_system(value::JSON3.Object)::Nothing
    _exact_keys(value, Set(("up_axis", "handedness", "units_per_meter")), "coordinate_system")
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
        Set((
            "camera_id",
            "projection",
            "position_xyz_m",
            "forward_xyz",
            "up_xyz",
            "near_plane_m",
            "far_plane_m",
            "width_px",
            "height_px",
        )),
        "camera",
    )
    camera_id = _string(value["camera_id"], "camera.camera_id")
    _valid_id(camera_id, "camera_id")
    near = _finite_float32(value["near_plane_m"], "camera.near_plane_m")
    far = _finite_float32(value["far_plane_m"], "camera.far_plane_m")
    near > 0.0f0 && far > near || throw(ProtocolError("malformed_packet", "camera planes are invalid"))
    width = _integer(value["width_px"], "camera.width_px")
    height = _integer(value["height_px"], "camera.height_px")
    _dimensions(width, height, "camera")
    return CameraPacket(
        camera_id,
        _parse_projection(_object(value["projection"], "camera.projection")),
        _tuple(value["position_xyz_m"], Val(3), "camera.position_xyz_m"),
        _tuple(value["forward_xyz"], Val(3), "camera.forward_xyz"),
        _tuple(value["up_xyz"], Val(3), "camera.up_xyz"),
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
    _exact_keys(value, Set(("kind", "span_m")), "camera.projection")
    span = _finite_float32(value["span_m"], "camera.projection.span_m")
    span > 0.0f0 || throw(ProtocolError("malformed_packet", "orthographic camera span is invalid"))
    return OrthographicProjection(span)
end

function _parse_projection(::Val{:perspective}, value::JSON3.Object)::PerspectiveProjection
    _exact_keys(value, Set(("kind", "fov_y_degrees")), "camera.projection")
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
        Set((
            "capture_id",
            "camera_id",
            "width_px",
            "height_px",
            "format",
            "include_depth",
            "deterministic",
        )),
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
    _bool(value["include_depth"], "capture.include_depth")
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

function _tuple(value, ::Val{N}, label::String)::NTuple{N, Float32} where {N}
    array = _array(value, label)
    length(array) == N || throw(ProtocolError("malformed_packet", "$label has the wrong arity"))
    return ntuple(index -> _finite_float32(array[index], "$label[$index]"), N)
end

function _exact_keys(value::JSON3.Object, expected::Set{String}, label::String)::Nothing
    actual = Set(String(key) for key in keys(value))
    actual == expected || throw(ProtocolError("malformed_packet", "$label has an unexpected or missing field"))
    return nothing
end

function _string(value, label::String)::String
    value isa AbstractString || throw(ProtocolError("malformed_packet", "$label must be a string"))
    return String(value)
end

function _bool(value, label::String)::Bool
    value isa Bool || throw(ProtocolError("malformed_packet", "$label must be a boolean"))
    return value
end

function _integer(value, label::String)::Int
    if value isa Integer
        try
            return Int(value)
        catch
            throw(ProtocolError("malformed_packet", "$label is outside the host integer range"))
        end
    elseif value isa AbstractFloat && isfinite(value) && isinteger(value)
        typemin(Int) <= value <= typemax(Int) ||
            throw(ProtocolError("malformed_packet", "$label is outside the host integer range"))
        return Int(value)
    end
    throw(ProtocolError("malformed_packet", "$label must be an integer"))
end

function _uint64(value, label::String)::UInt64
    integer = _integer(value, label)
    integer >= 0 || throw(ProtocolError("malformed_packet", "$label must be non-negative"))
    return UInt64(integer)
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
    number = if value isa Integer
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
    (!isempty(value) && length(value) <= 256) || throw(ProtocolError("malformed_packet", "$label is empty or too long"))
    any(isspace, value) && throw(ProtocolError("malformed_packet", "$label contains whitespace"))
    return nothing
end

function _valid_sha(value::String, label::String)::Nothing
    length(value) == 71 && startswith(value, "sha256:") && all(isxdigit, value[8:end]) ||
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

function _sha256(bytes::Vector{UInt8})::String
    return "sha256:" * bytes2hex(sha256(bytes))
end

end
