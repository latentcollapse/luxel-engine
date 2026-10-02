using Base64
using GeometryBasics: Vec4f
using SHA
using Test
using WGEGraphics

include(joinpath(@__DIR__, "..", "src", "LavaAdapter.jl"))
using .LavaAdapter

@testset "RGBA payload layout" begin
    bytes = UInt8[
        1, 2, 3, 4,
        5, 6, 7, 8,
        9, 10, 11, 12,
        13, 14, 15, 16,
        17, 18, 19, 20,
        21, 22, 23, 24,
    ]
    matrix = LavaAdapter._texture_matrix(bytes, UInt32(3), UInt32(2))
    @test size(matrix) == (2, 3)
    @test matrix[1, 1] == (1.0f0 / 255.0f0, 2.0f0 / 255.0f0, 3.0f0 / 255.0f0, 4.0f0 / 255.0f0)
    @test matrix[2, 3] == (21.0f0 / 255.0f0, 22.0f0 / 255.0f0, 23.0f0 / 255.0f0, 24.0f0 / 255.0f0)
end

@testset "linear and sRGB transfer" begin
    @test LavaAdapter._linear_to_srgb(0.5f0) ≈ 0.7353569f0 atol = 1.0f-5
    @test LavaAdapter._srgb_to_linear(0.5f0) ≈ 0.21404114f0 atol = 1.0f-5
    @test LavaAdapter._srgb_to_linear(LavaAdapter._linear_to_srgb(0.5f0)) ≈ 0.5f0 atol = 1.0f-5

    srgb = LavaAdapter._texture_matrix(UInt8[128, 128, 128, 255], UInt32(1), UInt32(1), :srgb)
    @test srgb[1, 1][1] ≈ 0.2158605f0 atol = 1.0f-5
    @test srgb[1, 1][4] == 1.0f0

    normal = LavaAdapter._texture_matrix(UInt8[128, 64, 255, 255], UInt32(1), UInt32(1), :normal_map)
    @test normal[1, 1] == (128.0f0 / 255.0f0, 64.0f0 / 255.0f0, 1.0f0, 1.0f0)

    data = LavaAdapter._texture_matrix(UInt8[64, 128, 192, 255], UInt32(1), UInt32(1), :data)
    @test data[1, 1] == (64.0f0 / 255.0f0, 128.0f0 / 255.0f0, 192.0f0 / 255.0f0, 1.0f0)
end

@testset "analytic environment lighting" begin
    environment = WGEGraphics.EnvironmentPacket(
        (0.08f0, 0.16f0, 0.30f0),
        (0.48f0, 0.56f0, 0.62f0),
        (0.16f0, 0.20f0, 0.16f0),
        (0.46f0, 0.53f0, 0.58f0),
        0.006f0,
        1.0f0,
    )
    lighting = LavaAdapter._environment_lighting(environment)
    top = LavaAdapter._environment_color(
        Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0),
        lighting.sky_top,
        lighting.sky_horizon,
        lighting.ground,
    )
    horizon = LavaAdapter._environment_color(
        Vec4f(1.0f0, 0.0f0, 0.0f0, 0.0f0),
        lighting.sky_top,
        lighting.sky_horizon,
        lighting.ground,
    )
    ground = LavaAdapter._environment_color(
        Vec4f(0.0f0, -1.0f0, 0.0f0, 0.0f0),
        lighting.sky_top,
        lighting.sky_horizon,
        lighting.ground,
    )
    @test top[1:3] == lighting.sky_top[1:3]
    @test horizon[1:3] == lighting.sky_horizon[1:3]
    @test ground[1:3] == lighting.ground[1:3]
end

@testset "clearcoat material lobe" begin
    common = (
        base_color=Vec4f(0.62f0, 0.48f0, 0.28f0, 1.0f0),
        normal=Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0),
        light_direction=Vec4f(0.0f0, -1.0f0, 0.0f0, 0.0f0),
        light_color=Vec4f(1.0f0, 0.96f0, 0.88f0, 1.0f0),
        light_intensity=1.5f0,
        environment_top=Vec4f(0.06f0, 0.12f0, 0.24f0, 1.0f0),
        environment_horizon=Vec4f(0.42f0, 0.48f0, 0.54f0, 1.0f0),
        environment_ground=Vec4f(0.10f0, 0.12f0, 0.10f0, 1.0f0),
        metallic=0.18f0,
        roughness=0.38f0,
        shadow_visibility=1.0f0,
        view_direction=Vec4f(0.0f0, 1.0f0, 2.0f0, 0.0f0),
        roughness_sample=0.72f0,
        occlusion_sample=1.0f0,
        occlusion_strength=0.6f0,
        emissive_factor=Vec4f(0.0f0, 0.0f0, 0.0f0, 1.0f0),
        emissive_sample=Vec4f(0.0f0, 0.0f0, 0.0f0, 1.0f0),
    )
    response(clearcoat, clearcoat_roughness) = LavaAdapter._material_response(
        common.base_color,
        common.normal,
        common.light_direction,
        common.light_color,
        common.light_intensity,
        common.environment_top,
        common.environment_horizon,
        common.environment_ground,
        common.metallic,
        common.roughness,
        clearcoat,
        clearcoat_roughness,
        common.shadow_visibility,
        common.view_direction,
        common.roughness_sample,
        common.occlusion_sample,
        common.occlusion_strength,
        common.emissive_factor,
        common.emissive_sample,
    )
    without_coat = response(0.0f0, 0.25f0)
    with_coat = response(1.0f0, 0.12f0)
    @test with_coat[1] > without_coat[1]
    @test with_coat[2] > without_coat[2]
    @test all(isfinite, with_coat)
end

@testset "semantic importance dispatch" begin
    background = WGEGraphics.BackgroundImportance()
    landmark = WGEGraphics.LandmarkImportance()
    critical = WGEGraphics.GameplayCriticalImportance()
    @test LavaAdapter._importance_margin(background) == 0.0f0
    @test LavaAdapter._importance_margin(background) < LavaAdapter._importance_margin(landmark)
    @test LavaAdapter._importance_margin(landmark) < LavaAdapter._importance_margin(critical)
end

@testset "rotated instance bounds use the instance quaternion" begin
    mesh = WGEGraphics.MeshPacket(
        "rotated-bounds-mesh",
        [(1.0f0, 0.0f0, 0.0f0)],
        [(0.0f0, 1.0f0, 0.0f0)],
        [(0.0f0, 0.0f0)],
        UInt32[0, 0, 0],
        "bounds-material",
    )
    packet = WGEGraphics.GraphicsScenePacket(
        "packet-sha",
        "content-sha",
        "bounds-packet",
        "world",
        "world-sha",
        "spatial-sha",
        UInt64(1),
        WGEGraphics.CameraPacket(
            "camera",
            WGEGraphics.OrthographicProjection(16.0f0),
            (0.0f0, 8.0f0, 0.0f0),
            (0.0f0, -1.0f0, 0.0f0),
            (0.0f0, 0.0f0, -1.0f0),
            0.1f0,
            100.0f0,
            UInt32(64),
            UInt32(64),
        ),
        WGEGraphics.TerrainPacket(
            "terrain",
            16.0f0,
            16.0f0,
            2,
            "bounds-material",
            Float32[0.0f0, 0.0f0, 0.0f0, 0.0f0],
            Float32[0.0f0, 0.0f0, 0.0f0, 0.0f0],
            UInt8[0, 0, 0, 0],
        ),
        [WGEGraphics.MaterialPacket(
            "bounds-material",
            (0.5f0, 0.5f0, 0.5f0, 1.0f0),
            0.0f0,
            1.0f0,
            0.0f0,
            0.5f0,
            :opaque,
            String[],
            nothing,
            nothing,
            nothing,
            nothing,
            1.0f0,
            1.0f0,
            (0.0f0, 0.0f0, 0.0f0),
        )],
        WGEGraphics.TexturePacket[],
        [mesh],
        WGEGraphics.InstancePacket[],
        [WGEGraphics.LightPacket(
            "sun",
            WGEGraphics.DirectionalLightPacket((0.0f0, -1.0f0, 0.0f0)),
            (1.0f0, 1.0f0, 1.0f0),
            1.0f0,
        )],
        WGEGraphics.EnvironmentPacket(
            (0.1f0, 0.1f0, 0.1f0),
            (0.2f0, 0.2f0, 0.2f0),
            (0.05f0, 0.05f0, 0.05f0),
            (0.1f0, 0.1f0, 0.1f0),
            0.0f0,
            1.0f0,
        ),
        WGEGraphics.OverlayValue[],
        "capture",
        UInt32(64),
        UInt32(64),
        false,
    )
    angle = Float32(pi / 2)
    importance = WGEGraphics.BackgroundImportance()
    transform = WGEGraphics.TransformPacket(
        (10.0f0, 0.0f0, 0.0f0),
        (0.0f0, sin(angle / 2.0f0), 0.0f0, cos(angle / 2.0f0)),
        (1.0f0, 1.0f0, 1.0f0),
    )
    instance = WGEGraphics.InstancePacket(
        "rotated-instance",
        "rotated-bounds-mesh",
        "bounds-material",
        importance,
        transform,
    )
    center, radius = LavaAdapter._instance_world_bound(packet, instance)
    @test center[1] ≈ 10.0f0 atol = 1.0f-5
    @test center[3] ≈ -1.0f0 atol = 1.0f-5
    @test radius ≈ 0.0f0 atol = 1.0f-5
end

@testset "terrain albedo follows typed material intent" begin
    material_color = Vec4f(0.42f0, 0.31f0, 0.19f0, 1.0f0)
    for slope in (0.0f0, 0.25f0, 1.0f0), region in (UInt8(0), UInt8(1), UInt8(17), typemax(UInt8))
        @test LavaAdapter._terrain_base_color(material_color, slope, region) == material_color
    end
end

@testset "tangent basis lowering" begin
    flat_tangent = LavaAdapter._stable_tangent(Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0))
    @test Tuple(flat_tangent[1:3]) == (1.0f0, 0.0f0, 0.0f0)
    transformed = LavaAdapter._transform_tangent(
        Vec4f(0.0f0, 0.0f0, 0.0f0, 1.0f0),
        Vec4f(1.0f0, 1.0f0, 0.0f0, 1.0f0),
        Vec4f(2.0f0, 1.0f0, 1.0f0, 0.0f0),
    )
    @test transformed[1] ≈ 0.8944272f0 atol = 1.0f-5
    @test transformed[2] ≈ 0.4472136f0 atol = 1.0f-5
    @test transformed[4] == 1.0f0
end

@testset "capture-domain measurements" begin
    @test LavaAdapter._pixel_luminance((UInt8(255), UInt8(255), UInt8(255), UInt8(255))) ≈ 1.0 atol = 1.0f-6
    @test LavaAdapter._pixel_luminance((1.0f0, 1.0f0, 1.0f0, 1.0f0)) ≈ 1.0 atol = 1.0f-6
    @test LavaAdapter._rgba8((0.0f0, 0.0f0, 0.0f0, 1.0f0 / 510.0f0))[4] == UInt8(1)
    bytes = UInt8[0, 127, 255, 255, 255, 0, 0, 255]
    @test LavaAdapter._capture_pixels(bytes) == [
        (UInt8(0), UInt8(127), UInt8(255), UInt8(255)),
        (UInt8(255), UInt8(0), UInt8(0), UInt8(255)),
    ]
end

@testset "camera projection lowering" begin
    orthographic = WGEGraphics.CameraPacket(
        "orthographic",
        WGEGraphics.OrthographicProjection(20.0f0),
        (0.0f0, 10.0f0, 0.0f0),
        (0.0f0, -1.0f0, 0.0f0),
        (0.0f0, 0.0f0, -1.0f0),
        0.1f0,
        100.0f0,
        UInt32(320),
        UInt32(240),
    )
    orthographic_frame = LavaAdapter._camera_frame(orthographic)
    orthographic_center = LavaAdapter._project_point(orthographic_frame, (0.0f0, 0.0f0, 0.0f0))
    orthographic_near_clip = LavaAdapter._project_clip(
        Vec4f(0.0f0, 10.5f0, 0.0f0, 1.0f0),
        orthographic_frame.position,
        orthographic_frame.right,
        orthographic_frame.up,
        orthographic_frame.forward,
        orthographic_frame.projection,
        orthographic_frame.mode,
    )
    orthographic_far_clip = LavaAdapter._project_clip(
        Vec4f(0.0f0, -110.0f0, 0.0f0, 1.0f0),
        orthographic_frame.position,
        orthographic_frame.right,
        orthographic_frame.up,
        orthographic_frame.forward,
        orthographic_frame.projection,
        orthographic_frame.mode,
    )
    @test orthographic_frame.mode == 0.0f0
    @test orthographic_center[1] ≈ 0.0f0
    @test orthographic_center[2] ≈ 0.0f0
    @test 0.0f0 < orthographic_center[3] < 1.0f0
    # The semantic camera-up direction must land above the framebuffer
    # center after the Vulkan positive-height viewport lowering.
    semantic_up_point = LavaAdapter._project_point(orthographic_frame, (0.0f0, 0.0f0, -1.0f0))
    @test semantic_up_point[2] < orthographic_center[2]
    @test orthographic_near_clip[3] < 0.0f0
    @test orthographic_far_clip[3] > 1.0f0
    @test LavaAdapter._project_overlay_point(orthographic_frame, (0.0f0, 10.5f0, 0.0f0)) === nothing
    point_positions = Vec4f[]
    point_colors = Vec4f[]
    point = WGEGraphics.PointOverlay(
        "marker",
        :player_spawn,
        (-6.0f0, 0.0f0, 4.0f0),
        0.8f0,
        (0.28f0, 0.94f0, 0.42f0, 1.0f0),
    )
    LavaAdapter._append_overlay!(point_positions, point_colors, point, orthographic_frame)
    point_center = LavaAdapter._project_point(orthographic_frame, point.position_xyz_m)
    @test length(point_positions) == 36
    @test length(point_colors) == length(point_positions)
    @test any(
        position -> position[1] == point_center[1] && position[2] == point_center[2],
        point_positions,
    )

    perspective = WGEGraphics.CameraPacket(
        "perspective",
        WGEGraphics.PerspectiveProjection(60.0f0),
        (0.0f0, 10.0f0, 0.0f0),
        (0.0f0, -1.0f0, 0.0f0),
        (0.0f0, 0.0f0, -1.0f0),
        0.1f0,
        100.0f0,
        UInt32(320),
        UInt32(240),
    )
    perspective_frame = LavaAdapter._camera_frame(perspective)
    perspective_center = LavaAdapter._project_point(perspective_frame, (0.0f0, 0.0f0, 0.0f0))
    near_clip = LavaAdapter._project_clip(
        Vec4f(0.0f0, 8.0f0, 0.0f0, 1.0f0),
        perspective_frame.position,
        perspective_frame.right,
        perspective_frame.up,
        perspective_frame.forward,
        perspective_frame.projection,
        perspective_frame.mode,
    )
    far_clip = LavaAdapter._project_clip(
        Vec4f(0.0f0, -10.0f0, 0.0f0, 1.0f0),
        perspective_frame.position,
        perspective_frame.right,
        perspective_frame.up,
        perspective_frame.forward,
        perspective_frame.projection,
        perspective_frame.mode,
    )
    @test perspective_frame.mode == 1.0f0
    @test perspective_center[1] ≈ 0.0f0
    @test perspective_center[2] ≈ 0.0f0
    @test 0.0f0 < perspective_center[3] < 1.0f0
    @test near_clip[4] > 0.0f0
    @test far_clip[4] > near_clip[4]
    @test (near_clip[1] / near_clip[4]) ≈ 0.0f0
    @test (far_clip[1] / far_clip[4]) ≈ 0.0f0
    @test (near_clip[3] / near_clip[4]) ≈
          (100.0f0 * 2.0f0 - 0.1f0 * 100.0f0) / (99.9f0 * 2.0f0)
    @test (far_clip[3] / far_clip[4]) ≈
          (100.0f0 * 20.0f0 - 0.1f0 * 100.0f0) / (99.9f0 * 20.0f0)
    thin_perspective = WGEGraphics.CameraPacket(
        "thin-perspective",
        WGEGraphics.PerspectiveProjection(60.0f0),
        (0.0f0, 10.0f0, 0.0f0),
        (0.0f0, -1.0f0, 0.0f0),
        (0.0f0, 0.0f0, -1.0f0),
        1.0f0,
        1.00001f0,
        UInt32(320),
        UInt32(240),
    )
    thin_frame = LavaAdapter._camera_frame(thin_perspective)
    thin_clip = LavaAdapter._project_clip(
        Vec4f(0.0f0, 8.999995f0, 0.0f0, 1.0f0),
        thin_frame.position,
        thin_frame.right,
        thin_frame.up,
        thin_frame.forward,
        thin_frame.projection,
        thin_frame.mode,
    )
    @test 0.0f0 < thin_clip[3] / thin_clip[4] < 1.0f0
    large_frustum = WGEGraphics.CameraPacket(
        "large-frustum",
        WGEGraphics.PerspectiveProjection(60.0f0),
        (0.0f0, 1.0f6, 0.0f0),
        (0.0f0, -1.0f0, 0.0f0),
        (0.0f0, 0.0f0, -1.0f0),
        999999.875f0,
        1.0f6,
        UInt32(320),
        UInt32(240),
    )
    large_frame = LavaAdapter._camera_frame(large_frustum)
    large_far_clip = LavaAdapter._project_clip(
        Vec4f(0.0f0, 0.0f0, 0.0f0, 1.0f0),
        large_frame.position,
        large_frame.right,
        large_frame.up,
        large_frame.forward,
        large_frame.projection,
        large_frame.mode,
    )
    large_far_ndc = large_far_clip[3] / large_far_clip[4]
    @test large_far_ndc ≈ 1.0f0 atol = 1.0f-5
    @test LavaAdapter._project_point(large_frame, (0.0f0, 0.0f0, 0.0f0))[3] ≈ 1.0f0 atol = 1.0f-5
    tiny_near = 0.0001f0
    tiny_far = nextfloat(tiny_near)
    tiny_camera = WGEGraphics.CameraPacket(
        "tiny-overlay-frustum",
        WGEGraphics.PerspectiveProjection(60.0f0),
        (0.0f0, 0.0f0, 0.0f0),
        (0.0f0, 0.0f0, 1.0f0),
        (0.0f0, 1.0f0, 0.0f0),
        tiny_near,
        tiny_far,
        UInt32(320),
        UInt32(240),
    )
    tiny_frame = LavaAdapter._camera_frame(tiny_camera)
    @test LavaAdapter._clip_overlay_segment(
        tiny_frame,
        (0.0f0, 0.0f0, prevfloat(tiny_near)),
        (0.0f0, 0.0f0, tiny_far),
    ) !== nothing
    @test LavaAdapter._sphere_visible(
        perspective_frame,
        Vec4f(11.2f0, 0.0f0, 10.0f0, 0.0f0),
        4.0f0,
    )
    @test !LavaAdapter._sphere_visible(
        perspective_frame,
        Vec4f(20.0f0, 0.0f0, 10.0f0, 0.0f0),
        0.5f0,
    )
    clipped_positions = Vec4f[]
    clipped_colors = Vec4f[]
    LavaAdapter._append_overlay_segment!(
        clipped_positions,
        clipped_colors,
        perspective_frame,
        (0.0f0, 9.5f0, 0.0f0),
        (0.0f0, 5.0f0, 0.0f0),
        Vec4f(1.0f0, 0.0f0, 0.0f0, 1.0f0),
        0.1f0,
    )
    @test length(clipped_positions) == 6
    @test length(clipped_colors) == 6
    thin_positions = Vec4f[]
    thick_positions = Vec4f[]
    thin_colors = Vec4f[]
    thick_colors = Vec4f[]
    segment_first = (0.0f0, 9.0f0, 0.0f0)
    segment_last = (0.0f0, 5.0f0, 0.0f0)
    LavaAdapter._append_overlay_segment!(
        thin_positions,
        thin_colors,
        perspective_frame,
        segment_first,
        segment_last,
        Vec4f(1.0f0, 0.0f0, 0.0f0, 1.0f0),
        0.1f0,
    )
    LavaAdapter._append_overlay_segment!(
        thick_positions,
        thick_colors,
        perspective_frame,
        segment_first,
        segment_last,
        Vec4f(1.0f0, 0.0f0, 0.0f0, 1.0f0),
        1.0f0,
    )
    @test maximum(point[2] for point in thick_positions) - minimum(point[2] for point in thick_positions) >
          maximum(point[2] for point in thin_positions) - minimum(point[2] for point in thin_positions)

    @test LavaAdapter._distance_between(
        Vec4f(10.0f0, 2.0f0, -4.0f0, 0.0f0),
        Vec4f(2.0f0, 2.0f0, -4.0f0, 0.0f0),
    ) ≈ 8.0f0

    degenerate_camera = WGEGraphics.CameraPacket(
        "degenerate",
        WGEGraphics.OrthographicProjection(20.0f0),
        (0.0f0, 10.0f0, 0.0f0),
        (0.0f0, 0.0f0, 0.0f0),
        (0.0f0, 1.0f0, 0.0f0),
        0.1f0,
        100.0f0,
        UInt32(320),
        UInt32(240),
    )
    @test_throws LavaAdapter.AdapterError LavaAdapter._camera_frame(degenerate_camera)

    collinear_camera = WGEGraphics.CameraPacket(
        "collinear",
        WGEGraphics.OrthographicProjection(20.0f0),
        (0.0f0, 10.0f0, 0.0f0),
        (0.0f0, -1.0f0, 0.0f0),
        (0.0f0, 2.0f0, 0.0f0),
        0.1f0,
        100.0f0,
        UInt32(320),
        UInt32(240),
    )
    @test_throws LavaAdapter.AdapterError LavaAdapter._camera_frame(collinear_camera)

    @test LavaAdapter._shadow_up_hint(Vec4f(0.0f0, 0.0f0, 1.0f0, 0.0f0)) ==
        Vec4f(1.0f0, 0.0f0, 0.0f0, 0.0f0)
    @test LavaAdapter._shadow_up_hint(Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0)) ==
        Vec4f(0.0f0, 1.0f0, 0.0f0, 0.0f0)
end

@testset "persistent Lava adapter" begin
    state = LavaAdapter.backend()
    @test state === LavaAdapter.backend()

    capabilities = LavaAdapter.backend_probe(state)
    @test capabilities.schema == "wge.lava-backend-probe/v1"
    @test capabilities.persistent_context
    @test !isempty(capabilities.device_uuid)
    @test capabilities.hardware_ray_tracing
    @test capabilities.gpu_timestamps

    first = LavaAdapter.render_probe(24, 12, state)
    @test first.matrix_size == (24, 12)
    @test first.center_rgba == Float32[0.0, 1.0, 0.0, 1.0]
    @test first.green_pixels > 0
    first_bytes = base64decode(first.capture_base64)
    @test length(first_bytes) == 24 * 12 * 4
    @test first.capture_sha256 == "sha256:" * bytes2hex(sha256(first_bytes))
    @test first.telemetry.draw_calls == 1
    @test first.telemetry.pipeline_compilations == 1

    second = LavaAdapter.render_probe(8, 6, state)
    @test second.matrix_size == (8, 6)
    @test second.center_rgba == Float32[0.0, 1.0, 0.0, 1.0]
    @test second.telemetry.draw_calls == 1
    @test second.telemetry.pipeline_compilations == 0
    @test length(base64decode(second.capture_base64)) == 8 * 6 * 4

    depth = LavaAdapter.render_depth_probe(state)
    @test depth.nearer_fragment_won
    @test depth.center_rgba[3] > 0.9f0

    texture = LavaAdapter.render_texture_probe(state)
    @test texture.texture_sampled
    @test texture.center_rgba ≈ texture.expected_rgba atol=0.05f0

    final_capabilities = LavaAdapter.backend_probe(state)
    @test final_capabilities.offscreen_raster
    @test final_capabilities.readback
    @test final_capabilities.depth_attachment
    @test final_capabilities.texture_sampling
end
