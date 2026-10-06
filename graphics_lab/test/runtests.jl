using Base64
using JSON3
using Test
using LuxelGraphics

include(joinpath(@__DIR__, "..", "bin", "luxel_graphics_worker.jl"))

function minimal_scene_packet_json()
    heights = fill(0.0f0, 9)
    slopes = fill(0.0f0, 9)
    regions = fill(UInt8(0), 9)
    buffer(id, encoding, values, stride) = (
        buffer_id=id,
        source_artifact_id=nothing,
        byte_length=length(values) * stride,
        count=length(values),
        stride_bytes=stride,
        sha256=LuxelGraphics._payload_sha256(values),
        payload=(encoding=encoding, values=values),
    )
    body = (
        schema_version="luxel.graphics-scene-packet/v6",
        packet_id="packet-1",
        world_artifact_id="world-1",
        world_artifact_sha256="sha256:" * repeat("0", 64),
        spatial_fields_sha256="sha256:" * repeat("1", 64),
        frame_seed=UInt64(7),
        coordinate_system=(up_axis="y", handedness="right", units_per_meter=1.0),
        camera=(
            camera_id="camera-1",
            projection=(kind="orthographic", span_m=20.0),
            position_xyz_m=(0.0, 10.0, 0.0),
            forward_xyz=(0.0, -1.0, 0.0),
            up_xyz=(0.0, 0.0, -1.0),
            near_plane_m=0.1,
            far_plane_m=100.0,
            width_px=32,
            height_px=32,
        ),
        terrain=(
            terrain_id="terrain-1",
            width_m=20.0,
            length_m=20.0,
            resolution=3,
            material_id="ground",
            heights_m=buffer("height-buffer", "f32", heights, 4),
            slope_grade=buffer("slope-buffer", "f32", slopes, 4),
            region_codes=buffer("region-buffer", "u8", regions, 1),
        ),
        materials=[(
            material_id="ground",
            base_color_rgba=(0.3, 0.4, 0.2, 1.0),
            metallic=0.0,
            roughness=0.8,
            clearcoat=0.0,
            clearcoat_roughness=0.5,
            alpha_mode="opaque",
            texture_ids=String[],
            normal_texture_id=nothing,
            normal_scale=1.0,
            occlusion_strength=1.0,
            emissive_factor_rgb=(0.0, 0.0, 0.0),
        )],
        textures=Any[],
        meshes=Any[],
        instances=Any[],
        lights=[(
            light_id="sun",
            kind=(kind="directional", direction_xyz=(0.2, -1.0, 0.1)),
            color_rgb=(1.0, 0.98, 0.9),
            intensity=1.0,
        )],
        environment=(
            sky_top_rgb=(0.1, 0.2, 0.4),
            sky_horizon_rgb=(0.5, 0.6, 0.7),
            ground_rgb=(0.2, 0.2, 0.2),
            fog_color_rgb=(0.5, 0.6, 0.7),
            fog_density=0.0,
            exposure=1.0,
        ),
        overlays=Any[],
        capture=(
            capture_id="capture-1",
            camera_id="camera-1",
            width_px=32,
            height_px=32,
            format="rgba8_srgb",
            include_depth=false,
            deterministic=true,
        ),
    )
    packet = JSON3.read(JSON3.write((body=body, packet_sha256="sha256:" * repeat("0", 64))))
    digest = LuxelGraphics._packet_content_sha256(packet)
    return JSON3.write((body=body, packet_sha256=digest))
end

@testset "graphics worker protocol boundary" begin
    ready = JSON3.read(
        JSON3.write((
            schema="luxel.graphics-worker/v1",
            kind="ready",
            profile="protocol_only",
            lava_revision=LAVA_REVISION,
        )),
    )
    @test ready["kind"] == "ready"
    @test LAVA_REVISION == "11c7e31bdf62408d22bf379e9e59510f69d2103e"
    failed = JSON3.read(handle("{\"op\":\"wrong\",\"packet\":{}}"))
    @test failed["kind"] == "failed"
    @test failed["code"] == "unsupported_operation"
    malformed = JSON3.read(handle("not-json"))
    @test malformed["code"] == "malformed_request"
    malformed_op = JSON3.read(handle(JSON3.write((op=42,))))
    @test malformed_op["code"] == "malformed_request"
    malformed_dimension = JSON3.read(handle(JSON3.write((op="probe_backend", width_px="wide"))))
    @test malformed_dimension["code"] == "malformed_request"
end

@testset "protocol packet parser rejects authority-shaped fakes" begin
    fake = JSON3.read(
        handle(
            JSON3.write((
                op="validate_packet",
                packet=(body=(schema_version="luxel.graphics-scene-packet/v6",), packet_sha256="pass"),
            )),
        ),
    )
    @test fake["kind"] == "failed"
    @test fake["code"] in ("malformed_packet", "unsupported_schema")
end

@testset "typed packet parser rejects duplicate fields and honors nullable Rust options" begin
    payload = minimal_scene_packet_json()
    packet = validate_scene_packet(payload)
    @test packet.world_artifact_id == "world-1"
    @test packet.terrain.heights_m == fill(0.0f0, 9)

    bound_scene = replace(
        payload,
        "\"packet_id\":\"packet-1\"," =>
            "\"packet_id\":\"packet-1\",\"scene_artifact_id\":\"scene-v1\",\"scene_artifact_sha256\":\"sha256:" * repeat("3", 64) * "\",",
    )
    @test validate_scene_packet(bound_scene).packet_id == "packet-1"
    incomplete_scene = replace(
        payload,
        "\"packet_id\":\"packet-1\"," =>
            "\"packet_id\":\"packet-1\",\"scene_artifact_id\":\"scene-v1\",",
    )
    @test_throws ProtocolError validate_scene_packet(incomplete_scene)

    duplicate_capture = replace(
        payload,
        "\"capture_id\":\"capture-1\"" => "\"capture_id\":\"capture-1\",\"capture_id\":\"capture-1\"",
    )
    @test_throws ProtocolError validate_scene_packet(duplicate_capture)

    duplicate_material = replace(
        payload,
        "\"materials\":[{\"material_id\":\"ground\"" =>
            "\"materials\":[{\"material_id\":\"ground\",\"material_id\":\"ground\"",
    )
    @test_throws ProtocolError validate_scene_packet(duplicate_material)

    duplicate_buffer = replace(
        payload,
        "\"buffer_id\":\"height-buffer\"" =>
            "\"buffer_id\":\"height-buffer\",\"buffer_id\":\"height-buffer\"",
    )
    @test_throws ProtocolError validate_scene_packet(duplicate_buffer)

    texture = JSON3.read(JSON3.write((
        texture_id="texture-1",
        source_artifact_id="source-1",
        sha256="sha256:" * repeat("2", 64),
        width_px=1,
        height_px=1,
        mip_levels=1,
        color_space="srgb",
        payload=nothing,
    )))
    parsed_texture = only(LuxelGraphics._parse_textures(JSON3.read(JSON3.write([texture]))))
    @test parsed_texture.payload === nothing
    @test isempty(parsed_texture.mip_chain)

    base_level = UInt8[255, 0, 0, 255, 0, 255, 0, 255]
    final_level = UInt8[128, 128, 0, 255]
    mip_texture = JSON3.read(JSON3.write((
        texture_id="mip-texture",
        source_artifact_id="source-1",
        sha256=LuxelGraphics._sha256(vcat(base_level, final_level)),
        width_px=2,
        height_px=1,
        mip_levels=2,
        color_space="srgb",
        payload=(
            encoding="rgba8_mip_chain",
            base64=(levels=[
                (width_px=2, height_px=1, base64=Base64.base64encode(base_level)),
                (width_px=1, height_px=1, base64=Base64.base64encode(final_level)),
            ],),
        ),
    )))
    parsed_mip_texture = only(LuxelGraphics._parse_textures(JSON3.read(JSON3.write([mip_texture]))))
    @test parsed_mip_texture.payload == base_level
    @test length(parsed_mip_texture.mip_chain) == 1
    @test parsed_mip_texture.mip_chain[1].bytes == final_level

    malformed_mip_texture = JSON3.read(JSON3.write((
        texture_id="mip-texture",
        source_artifact_id="source-1",
        sha256=LuxelGraphics._sha256(vcat(base_level, final_level)),
        width_px=2,
        height_px=1,
        mip_levels=2,
        color_space="srgb",
        payload=(
            encoding="rgba8_mip_chain",
            base64=(levels=[
                (width_px=2, height_px=1, base64=Base64.base64encode(base_level)),
                (width_px=2, height_px=1, base64=Base64.base64encode(final_level)),
            ],),
        ),
    )))
    @test_throws ProtocolError LuxelGraphics._parse_textures(JSON3.read(JSON3.write([malformed_mip_texture])))
end

@testset "typed numeric and identifier bounds match the Rust contract" begin
    @test LuxelGraphics._integer(3, "integer") == 3
    @test_throws ProtocolError LuxelGraphics._integer(3.0, "integer")
    @test_throws ProtocolError LuxelGraphics._integer(true, "integer")
    @test LuxelGraphics._uint64(7, "seed") == 7
    @test_throws ProtocolError LuxelGraphics._uint64(7.0, "seed")
    @test_throws ProtocolError LuxelGraphics._uint64(true, "seed")
    @test LuxelGraphics._finite_float32(1, "float") == 1.0f0
    @test_throws ProtocolError LuxelGraphics._finite_float32(true, "float")

    @test LuxelGraphics._valid_id(repeat("é", 128), "id") === nothing
    @test_throws ProtocolError LuxelGraphics._valid_id(repeat("é", 129), "id")

    boolean_projection = JSON3.read("""{"kind":"orthographic","span_m":true}""")
    @test_throws ProtocolError LuxelGraphics._parse_projection(boolean_projection)
end

@testset "protocol parser rejects degenerate graphics bases" begin
    camera = JSON3.read(
        """
        {
          "camera_id": "bad-camera",
          "projection": {"kind": "orthographic", "span_m": 20.0},
          "position_xyz_m": [0.0, 10.0, 0.0],
          "forward_xyz": [0.0, 0.0, 0.0],
          "up_xyz": [0.0, 1.0, 0.0],
          "near_plane_m": 0.1,
          "far_plane_m": 100.0,
          "width_px": 320,
          "height_px": 240
        }
        """,
    )
    @test_throws ProtocolError LuxelGraphics._parse_camera(camera)

    huge_projection = JSON3.read("{\"kind\":\"orthographic\",\"span_m\":1000001.0}")
    @test_throws ProtocolError LuxelGraphics._parse_projection(huge_projection)

    huge_texture = JSON3.read(
        """
        [{
          "texture_id": "huge-texture",
          "source_artifact_id": "source",
          "sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
          "width_px": 9000,
          "height_px": 1,
          "mip_levels": 1,
          "color_space": "srgb"
        }]
        """,
    )
    @test_throws ProtocolError LuxelGraphics._parse_textures(huge_texture)

    light = JSON3.read("{\"kind\":\"directional\",\"direction_xyz\":[0.0,0.0,0.0]}")
    @test_throws ProtocolError LuxelGraphics._parse_light_kind(light)

    mesh = JSON3.read(
        """
        [{
          "mesh_id": "bad-mesh",
          "positions_m": [[0.0,0.0,0.0],[1.0,0.0,0.0],[0.0,0.0,1.0]],
          "normals": [[0.0,1.0,0.0],[0.0,0.0,0.0],[0.0,1.0,0.0]],
          "uv0": [[0.0,0.0],[1.0,0.0],[0.0,1.0]],
          "indices": [0,1,2],
          "material_id": "terrain"
        }]
        """,
    )
    @test_throws ProtocolError LuxelGraphics._parse_meshes(mesh, Set(["terrain"]))
end

@testset "small non-degenerate vectors match the Rust packet contract" begin
    small_camera = JSON3.read(
        """
        {
          "camera_id": "small-camera",
          "projection": {"kind": "orthographic", "span_m": 20.0},
          "position_xyz_m": [0.0, 10.0, 0.0],
          "forward_xyz": [0.0002, 0.0, 0.0],
          "up_xyz": [0.0, 0.0, 0.0002],
          "near_plane_m": 0.1,
          "far_plane_m": 100.0,
          "width_px": 32,
          "height_px": 32
        }
        """,
    )
    @test LuxelGraphics._parse_camera(small_camera) isa LuxelGraphics.CameraPacket

    small_light = JSON3.read("""{"kind":"directional","direction_xyz":[0.0002,0.0,0.0]}""")
    @test LuxelGraphics._parse_light_kind(small_light) isa LuxelGraphics.DirectionalLightPacket

    small_normal_mesh = JSON3.read(
        """
        [{
          "mesh_id": "small-normal-mesh",
          "positions_m": [[0.0,0.0,0.0],[1.0,0.0,0.0],[0.0,0.0,1.0]],
          "normals": [[0.0002,0.0,0.0],[0.0002,0.0,0.0],[0.0002,0.0,0.0]],
          "uv0": [[0.0,0.0],[1.0,0.0],[0.0,1.0]],
          "indices": [0,1,2],
          "material_id": "terrain"
        }]
        """,
    )
    @test length(LuxelGraphics._parse_meshes(small_normal_mesh, Set(["terrain"]))) == 1
end

@testset "W-1 wet zone and standing water decode, and refuse what they must" begin
    zone(; water=(albedo_rgb=[0.018, 0.022, 0.016], roughness=0.02, edge_wobble=0.2, wobble_cycles_per_m=0.45, surface_y_m=12.49), extra...) =
        JSON3.read(JSON3.write(merge((center_xz_m=[31.7, 27.2], radii_xz_m=[3.8, 2.7], falloff_m=0.6,
                                      albedo_scale=0.6, roughness_scale=0.25, normal_scale=0.4),
                                     water === nothing ? NamedTuple() : (standing_water=water,), NamedTuple(extra))))
    parsed = LuxelGraphics._parse_wet_zone(zone())
    @test parsed.radii_xz_m == (3.8f0, 2.7f0)
    @test parsed.standing_water.surface_y_m == 12.49f0
    @test LuxelGraphics._parse_wet_zone(zone(water=nothing)).standing_water === nothing
    refuses(z) = try
        LuxelGraphics._parse_wet_zone(z); false
    catch e
        e isa LuxelGraphics.ProtocolError
    end
    @test refuses(zone(stray=1))                                   # unknown field
    @test refuses(zone(albedo_scale=1.5))                          # brightens
    @test refuses(zone(radii_xz_m=[0.0, 2.7]))                     # degenerate ellipse
    @test refuses(zone(water=(albedo_rgb=[0.02, 0.02, 0.02], roughness=0.02, edge_wobble=0.8, wobble_cycles_per_m=0.45, surface_y_m=0.0)))
    @test refuses(zone(water=(albedo_rgb=[0.02, 0.02, 0.02], roughness=0.02, edge_wobble=0.2, wobble_cycles_per_m=0.45)))  # no surface height
end

@testset "N-7 instance variation decodes, defaults to no tint, and refuses out-of-range tints" begin
    inst(; extra...) = JSON3.read(JSON3.write(merge((instance_id="i", mesh_id="m", material_id="mat", importance="background",
        transform=(translation_xyz_m=[0, 0, 0], rotation_xyzw=[0, 0, 0, 1], scale_xyz=[1, 1, 1])), NamedTuple(extra))))
    plain = LuxelGraphics.InstancePacket("i", "m", "mat", LuxelGraphics.BackgroundImportance(),
        LuxelGraphics.TransformPacket((0f0, 0f0, 0f0), (0f0, 0f0, 0f0, 1f0), (1f0, 1f0, 1f0)))
    @test plain.tint_rgb == (1.0f0, 1.0f0, 1.0f0)
    mesh = LuxelGraphics.MeshPacket("m", [(0f0, 0f0, 0f0)], [(0f0, 1f0, 0f0)], [(0f0, 0f0)], UInt32[], "mat",
                                  NTuple{4,Float32}[])
    parse(i) = LuxelGraphics._parse_instances(JSON3.read(JSON3.write([i])), [mesh], Set(["mat"]))
    @test only(parse(inst())).tint_rgb == (1.0f0, 1.0f0, 1.0f0)
    @test only(parse(inst(variation=(tint_rgb=[1.05, 0.97, 0.99],)))).tint_rgb == (1.05f0, 0.97f0, 0.99f0)
    refuses(i) = try parse(i); false catch e; e isa LuxelGraphics.ProtocolError end
    @test refuses(inst(variation=(tint_rgb=[2.0, 1.0, 1.0],)))
    @test refuses(inst(variation=(tint_rgb=[1.0, 1.0, 1.0], stray=1)))
end
