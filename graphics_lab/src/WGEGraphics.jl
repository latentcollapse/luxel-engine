module WGEGraphics

using JSON3
using SHA

export GraphicsScenePacket, ProtocolError, validate_scene_packet, packet_summary

const SCENE_PACKET_SCHEMA = "wge.graphics-scene-packet/v1"
const MAX_PACKET_ELEMENTS = 16 * 1024 * 1024
const MAX_CAPTURE_DIMENSION = 8192

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

struct GraphicsScenePacket
    packet_sha256::String
    packet_id::String
    world_artifact_id::String
    world_artifact_sha256::String
    spatial_fields_sha256::String
    frame_seed::UInt64
    terrain::TerrainPacket
    material_count::Int
    texture_count::Int
    mesh_count::Int
    instance_count::Int
    light_count::Int
    overlay_count::Int
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
    packet.overlay_count,
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

    _validate_coordinate_system(_object(body["coordinate_system"], "coordinate_system"))
    camera = _object(body["camera"], "camera")
    _validate_camera(camera)
    terrain = _parse_terrain(_object(body["terrain"], "terrain"), body["materials"])
    materials = _parse_materials(body["materials"])
    textures = _parse_textures(body["textures"])
    _validate_material_texture_links(materials, textures)
    meshes = _parse_meshes(body["meshes"], materials.ids)
    instances = _parse_instances(body["instances"], meshes, materials.ids)
    _parse_lights(body["lights"])
    overlays = _parse_overlays(body["overlays"])
    capture = _object(body["capture"], "capture")
    _validate_capture(capture, camera)

    return GraphicsScenePacket(
        packet_sha256,
        _string(body["packet_id"], "packet_id"),
        world_id,
        _string(body["world_artifact_sha256"], "world_artifact_sha256"),
        spatial_sha,
        frame_seed,
        terrain,
        length(materials.ids),
        length(textures),
        length(meshes),
        length(instances),
        _array_length(body["lights"], "lights"),
        overlays,
        _string(capture["capture_id"], "capture_id"),
        UInt32(_integer(capture["width_px"], "capture.width_px")),
        UInt32(_integer(capture["height_px"], "capture.height_px")),
        _bool(capture["include_depth"], "capture.include_depth"),
    )
end

function _parse_terrain(value::JSON3.Object, materials_value)::TerrainPacket
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
    materials_value isa JSON3.Array || throw(ProtocolError("malformed_packet", "materials must be an array"))
    any(_string(material["material_id"], "material_id") == material_id for material in materials_value) ||
        throw(ProtocolError("provenance", "terrain references an unknown material"))
    return TerrainPacket(terrain_id, width, length, resolution, material_id, heights, slope, regions)
end

function _parse_buffer(value, expected_count::Int, label::String, expected_encoding::Symbol)
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
    digest == _payload_sha256(values, encoding) || throw(ProtocolError("provenance", "$label digest mismatch"))
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

function _payload_sha256(values::Vector{Float32}, ::Symbol)
    bytes = UInt8[]
    sizehint!(bytes, 4 * length(values))
    for value in values
        bits = reinterpret(UInt32, [value])[1]
        append!(bytes, UInt8[(bits >> 0) & 0xff, (bits >> 8) & 0xff, (bits >> 16) & 0xff, (bits >> 24) & 0xff])
    end
    return _sha256(bytes)
end

function _payload_sha256(values::Vector{UInt8}, ::Symbol)
    return _sha256(values)
end

function _payload_sha256(values::Vector{UInt32}, ::Symbol)
    bytes = UInt8[]
    sizehint!(bytes, 4 * length(values))
    for value in values
        append!(bytes, UInt8[(value >> 0) & 0xff, (value >> 8) & 0xff, (value >> 16) & 0xff, (value >> 24) & 0xff])
    end
    return _sha256(bytes)
end

function _parse_materials(value)
    array = _array(value, "materials")
    ids = Set{String}()
    texture_ids = Set{String}()
    for material in array
        object = _object(material, "material")
        _exact_keys(object, Set(("material_id", "base_color_rgba", "metallic", "roughness", "alpha_mode", "texture_ids")), "material")
        id = _string(object["material_id"], "material.material_id")
        _valid_id(id, "material_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate material $id"))
        push!(ids, id)
        color = _array(object["base_color_rgba"], "material.base_color_rgba")
        length(color) == 4 || throw(ProtocolError("malformed_packet", "material color needs four channels"))
        all(0.0f0 .<= Float32[_finite_float32(v, "material color") for v in color] .<= 1.0f0) ||
            throw(ProtocolError("malformed_packet", "material color is outside [0, 1]"))
        metallic = _finite_float32(object["metallic"], "material.metallic")
        roughness = _finite_float32(object["roughness"], "material.roughness")
        0.0f0 <= metallic <= 1.0f0 || throw(ProtocolError("malformed_packet", "material metallic is outside [0, 1]"))
        0.0f0 <= roughness <= 1.0f0 || throw(ProtocolError("malformed_packet", "material roughness is outside [0, 1]"))
        _string(object["alpha_mode"], "material.alpha_mode") in ("opaque", "mask", "blend") ||
            throw(ProtocolError("unsupported", "material alpha mode is unsupported"))
        for texture_id in _array(object["texture_ids"], "material.texture_ids")
            texture_id = _string(texture_id, "material.texture_id")
            _valid_id(texture_id, "material.texture_id")
            push!(texture_ids, texture_id)
        end
    end
    return (ids=ids, texture_ids=texture_ids)
end

function _parse_textures(value)
    array = _array(value, "textures")
    ids = Set{String}()
    for texture in array
        object = _object(texture, "texture")
        _exact_keys(object, Set(("texture_id", "source_artifact_id", "sha256", "width_px", "height_px", "mip_levels", "color_space")), "texture")
        id = _string(object["texture_id"], "texture.texture_id")
        _valid_id(id, "texture_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate texture $id"))
        push!(ids, id)
        _valid_id(_string(object["source_artifact_id"], "texture.source_artifact_id"), "texture.source_artifact_id")
        _valid_sha(_string(object["sha256"], "texture.sha256"), "texture.sha256")
        _integer(object["width_px"], "texture.width_px") > 0 || throw(ProtocolError("malformed_packet", "texture width is invalid"))
        _integer(object["height_px"], "texture.height_px") > 0 || throw(ProtocolError("malformed_packet", "texture height is invalid"))
        _integer(object["mip_levels"], "texture.mip_levels") > 0 || throw(ProtocolError("malformed_packet", "texture mip count is invalid"))
        _string(object["color_space"], "texture.color_space") in ("srgb", "linear", "normal_map", "data") ||
            throw(ProtocolError("unsupported", "texture color space is unsupported"))
    end
    return ids
end

function _validate_material_texture_links(materials, texture_ids::Set{String})
    isempty(materials.ids) && throw(ProtocolError("malformed_packet", "packet needs a material"))
    for texture_id in materials.texture_ids
        texture_id in texture_ids ||
            throw(ProtocolError("provenance", "material references unknown texture $texture_id"))
    end
end

function _parse_meshes(value, material_ids::Set{String})
    array = _array(value, "meshes")
    ids = Set{String}()
    for mesh in array
        object = _object(mesh, "mesh")
        _exact_keys(object, Set(("mesh_id", "positions_m", "normals", "indices", "material_id")), "mesh")
        id = _string(object["mesh_id"], "mesh.mesh_id")
        _valid_id(id, "mesh_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate mesh $id"))
        push!(ids, id)
        _array(object["positions_m"], "mesh.positions_m")
        _array(object["normals"], "mesh.normals")
        _array(object["indices"], "mesh.indices")
        material_id = _string(object["material_id"], "mesh.material_id")
        _valid_id(material_id, "mesh.material_id")
        material_id in material_ids ||
            throw(ProtocolError("provenance", "mesh references unknown material $material_id"))
    end
    return ids
end

function _parse_instances(value, mesh_ids::Set{String}, material_ids::Set{String})
    array = _array(value, "instances")
    ids = Set{String}()
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
        for (key, expected) in (("translation_xyz_m", 3), ("rotation_xyzw", 4), ("scale_xyz", 3))
            values = _array(transform[key], "instance.transform.$key")
            length(values) == expected || throw(ProtocolError("malformed_packet", "instance transform has wrong arity"))
            [_finite_float32(value, "instance transform") for value in values]
        end
    end
    isempty(array) || isempty(mesh_ids) || throw(ProtocolError("provenance", "instance references absent meshes"))
    return ids
end

function _parse_lights(value)
    array = _array(value, "lights")
    isempty(array) && throw(ProtocolError("malformed_packet", "packet needs a light"))
    ids = Set{String}()
    for light in array
        object = _object(light, "light")
        _exact_keys(object, Set(("light_id", "kind", "color_rgb", "intensity")), "light")
        id = _string(object["light_id"], "light.light_id")
        _valid_id(id, "light_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate light $id"))
        push!(ids, id)
        kind_object = _object(object["kind"], "light.kind")
        kind = _string(kind_object["kind"], "light.kind.kind")
        if kind == "directional"
            _exact_keys(kind_object, Set(("kind", "direction_xyz")), "light.kind")
            direction = _array(kind_object["direction_xyz"], "light.kind.direction_xyz")
            length(direction) == 3 || throw(ProtocolError("malformed_packet", "light direction needs three coordinates"))
            [_finite_float32(value, "light direction") for value in direction]
        elseif kind == "point"
            _exact_keys(kind_object, Set(("kind", "position_xyz_m", "range_m")), "light.kind")
            position = _array(kind_object["position_xyz_m"], "light.kind.position_xyz_m")
            length(position) == 3 || throw(ProtocolError("malformed_packet", "point light position needs three coordinates"))
            [_finite_float32(value, "point light position") for value in position]
            _finite_float32(kind_object["range_m"], "light.kind.range_m") > 0.0f0 || throw(ProtocolError("malformed_packet", "point light range is invalid"))
        else
            throw(ProtocolError("unsupported", "light kind is unsupported"))
        end
        color = _array(object["color_rgb"], "light.color_rgb")
        length(color) == 3 || throw(ProtocolError("malformed_packet", "light color needs three channels"))
        [_finite_float32(value, "light color") for value in color]
        _finite_float32(object["intensity"], "light.intensity") >= 0.0f0 || throw(ProtocolError("malformed_packet", "light intensity is negative"))
    end
    return length(array)
end

function _parse_overlays(value)
    array = _array(value, "overlays")
    ids = Set{String}()
    for overlay in array
        object = _object(overlay, "overlay")
        kind = _string(object["kind"], "overlay.kind")
        _exact_keys(object, kind == "point" ? Set(("kind", "marker_id", "role", "position_xyz_m", "radius_m", "color_rgba")) : kind == "circle" ? Set(("kind", "marker_id", "role", "center_xyz_m", "radius_m", "color_rgba")) : kind == "polyline" ? Set(("kind", "marker_id", "role", "points_xyz_m", "thickness_m", "color_rgba")) : Set(("kind",)), "overlay")
        kind in ("point", "circle", "polyline") || throw(ProtocolError("unsupported", "overlay kind is unsupported"))
        id = _string(object["marker_id"], "overlay.marker_id")
        _valid_id(id, "marker_id")
        id in ids && throw(ProtocolError("malformed_packet", "duplicate overlay $id"))
        push!(ids, id)
        _string(object["role"], "overlay.role") in ("route", "player_spawn", "opponent_spawn", "encounter", "objective") ||
            throw(ProtocolError("unsupported", "overlay role is unsupported"))
        points = kind == "point" ? _array(object["position_xyz_m"], "overlay.position_xyz_m") : kind == "circle" ? _array(object["center_xyz_m"], "overlay.center_xyz_m") : _array(object["points_xyz_m"], "overlay.points_xyz_m")
        kind == "polyline" && length(points) < 2 && throw(ProtocolError("malformed_packet", "polyline needs two points"))
        for point in points
            point_array = kind == "polyline" ? point : points
            if kind == "polyline"
                coordinates = _array(point_array, "overlay point")
                length(coordinates) == 3 || throw(ProtocolError("malformed_packet", "overlay point needs three coordinates"))
                [_finite_float32(value, "overlay point") for value in coordinates]
            else
                length(point_array) == 3 || throw(ProtocolError("malformed_packet", "overlay point needs three coordinates"))
                [_finite_float32(value, "overlay point") for value in point_array]
                break
            end
        end
        style_key = kind == "polyline" ? "thickness_m" : "radius_m"
        _finite_float32(object[style_key], "overlay style") > 0.0f0 || throw(ProtocolError("malformed_packet", "overlay style is invalid"))
        color = _array(object["color_rgba"], "overlay.color_rgba")
        length(color) == 4 || throw(ProtocolError("malformed_packet", "overlay color needs four channels"))
        [_finite_float32(value, "overlay color") for value in color]
    end
    return length(array)
end

function _validate_coordinate_system(value::JSON3.Object)
    _exact_keys(value, Set(("up_axis", "handedness", "units_per_meter")), "coordinate_system")
    _string(value["up_axis"], "coordinate_system.up_axis") == "y" || throw(ProtocolError("unsupported", "native graphics requires Y up"))
    _string(value["handedness"], "coordinate_system.handedness") == "right" || throw(ProtocolError("unsupported", "native graphics requires right-handed coordinates"))
    _finite_float32(value["units_per_meter"], "coordinate_system.units_per_meter") == 1.0f0 || throw(ProtocolError("unsupported", "native graphics requires one unit per meter"))
end

function _validate_camera(value::JSON3.Object)
    _exact_keys(value, Set(("camera_id", "projection", "position_xyz_m", "forward_xyz", "up_xyz", "near_plane_m", "far_plane_m", "width_px", "height_px")), "camera")
    _valid_id(_string(value["camera_id"], "camera.camera_id"), "camera_id")
    for key in ("position_xyz_m", "forward_xyz", "up_xyz")
        coordinates = _array(value[key], "camera.$key")
        length(coordinates) == 3 || throw(ProtocolError("malformed_packet", "camera vector has wrong arity"))
        [_finite_float32(item, "camera.$key") for item in coordinates]
    end
    near = _finite_float32(value["near_plane_m"], "camera.near_plane_m")
    far = _finite_float32(value["far_plane_m"], "camera.far_plane_m")
    near > 0.0f0 && far > near || throw(ProtocolError("malformed_packet", "camera planes are invalid"))
    projection = _object(value["projection"], "camera.projection")
    projection_kind = _string(projection["kind"], "camera.projection.kind")
    if projection_kind == "orthographic"
        _exact_keys(projection, Set(("kind", "span_m")), "camera.projection")
        _finite_float32(projection["span_m"], "camera.projection.span_m") > 0.0f0 ||
            throw(ProtocolError("malformed_packet", "orthographic camera span is invalid"))
    elseif projection_kind == "perspective"
        _exact_keys(projection, Set(("kind", "fov_y_degrees")), "camera.projection")
        fov = _finite_float32(projection["fov_y_degrees"], "camera.projection.fov_y_degrees")
        1.0f0 <= fov <= 179.0f0 ||
            throw(ProtocolError("malformed_packet", "perspective camera field of view is invalid"))
    else
        throw(ProtocolError("unsupported", "camera projection is unsupported"))
    end
    _dimensions(_integer(value["width_px"], "camera.width_px"), _integer(value["height_px"], "camera.height_px"), "camera")
end

function _validate_capture(value::JSON3.Object, camera::JSON3.Object)
    _exact_keys(value, Set(("capture_id", "camera_id", "width_px", "height_px", "format", "include_depth", "deterministic")), "capture")
    _valid_id(_string(value["capture_id"], "capture.capture_id"), "capture_id")
    _string(value["camera_id"], "capture.camera_id") == _string(camera["camera_id"], "camera.camera_id") || throw(ProtocolError("provenance", "capture camera is detached"))
    width = _integer(value["width_px"], "capture.width_px")
    height = _integer(value["height_px"], "capture.height_px")
    width == _integer(camera["width_px"], "camera.width_px") && height == _integer(camera["height_px"], "camera.height_px") || throw(ProtocolError("provenance", "capture dimensions are detached"))
    _dimensions(width, height, "capture")
    _string(value["format"], "capture.format") == "rgba8_srgb" || throw(ProtocolError("unsupported", "capture format is unsupported"))
    _bool(value["include_depth"], "capture.include_depth")
    _bool(value["deterministic"], "capture.deterministic") || throw(ProtocolError("unsupported", "capture must be deterministic"))
end

function _object(value, label::String)::JSON3.Object
    value isa JSON3.Object || throw(ProtocolError("malformed_packet", "$label must be an object"))
    return value
end

function _array(value, label::String)::JSON3.Array
    value isa JSON3.Array || throw(ProtocolError("malformed_packet", "$label must be an array"))
    return value
end

function _array_length(value, label::String)::Int
    return length(_array(value, label))
end

function _exact_keys(value::JSON3.Object, expected::Set{String}, label::String)
    actual = Set(String(key) for key in keys(value))
    actual == expected || throw(ProtocolError("malformed_packet", "$label has an unexpected or missing field"))
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
        return Int(value)
    elseif value isa AbstractFloat && isfinite(value) && isinteger(value)
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

function _valid_id(value::String, label::String)
    (!isempty(value) && length(value) <= 256) || throw(ProtocolError("malformed_packet", "$label is empty or too long"))
    any(isspace, value) && throw(ProtocolError("malformed_packet", "$label contains whitespace"))
end

function _valid_sha(value::String, label::String)
    length(value) == 71 && startswith(value, "sha256:") && all(isxdigit, value[8:end]) ||
        throw(ProtocolError("malformed_packet", "$label is not a sha256 digest"))
end

function _dimensions(width::Int, height::Int, label::String)
    1 <= width <= MAX_CAPTURE_DIMENSION && 1 <= height <= MAX_CAPTURE_DIMENSION ||
        throw(ProtocolError("malformed_packet", "$label dimensions are outside bounds"))
end

function _sha(; body_value, path::String)
    return _string(body_value[path], path)
end

function _sha256(bytes::Vector{UInt8})::String
    return "sha256:" * bytes2hex(sha256(bytes))
end

end
