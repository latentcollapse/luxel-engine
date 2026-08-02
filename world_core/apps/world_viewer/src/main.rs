use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    env, fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result, bail};
use bevy::shader::ShaderRef;
use bevy::{
    app::AppExit,
    asset::{AssetPlugin, RenderAssetUsages},
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    image::{
        ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler,
        ImageSamplerDescriptor,
    },
    light::{CascadeShadowConfigBuilder, Skybox},
    mesh::{Indices, PrimitiveTopology},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat, TextureViewDescriptor,
        TextureViewDimension,
    },
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    window::{PrimaryWindow, WindowPlugin},
};
use codeweald_worldspec::{validate_render_plan_value, validate_value};
use serde_json::{Value, json};

const TERRAIN_STRIDE: usize = 4;
const POLL_SECONDS: f32 = 0.5;
const TERRAIN_SHADER: &str = "assets/shaders/codeweald_terrain.wgsl";

type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

#[derive(Clone, Copy, Debug, Default, Reflect, ShaderType)]
struct TerrainUniform {
    repeat_m: Vec4,
    world_size_m: Vec2,
    _padding: Vec2,
    /// Mean linear luminance of each layer's accepted albedo, splat order.
    ///
    /// The shader divides its material sample by this to get light-and-dark
    /// break-up it can modulate the compiler's palette with, instead of
    /// replacing that palette outright. Measured by the compiler from the same
    /// hash-verified pixels that are sampled here, so the two cannot drift.
    material_mean_luma: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
struct TerrainExtension {
    #[texture(100)]
    #[sampler(101)]
    splat: Handle<Image>,
    #[texture(102)]
    #[sampler(103)]
    grass: Handle<Image>,
    #[texture(104)]
    #[sampler(105)]
    road: Handle<Image>,
    #[texture(106)]
    #[sampler(107)]
    rock: Handle<Image>,
    #[texture(108)]
    #[sampler(109)]
    snow: Handle<Image>,
    #[uniform(110)]
    settings: TerrainUniform,
    #[texture(111)]
    #[sampler(112)]
    rock_normal: Handle<Image>,
}

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        TERRAIN_SHADER.into()
    }
}

#[derive(Resource, Clone)]
struct ViewerConfig {
    batch: PathBuf,
    renderer_root: PathBuf,
    capture_path: Option<PathBuf>,
    capture_view: String,
}

impl ViewerConfig {
    fn from_args() -> Result<Self> {
        let mut arguments = env::args().skip(1);
        let mut batch = None;
        let mut capture_path = None;
        let mut capture_view = "overview".to_owned();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--batch" => batch = arguments.next().map(PathBuf::from),
                "--capture" => capture_path = arguments.next().map(PathBuf::from),
                "--view" => capture_view = arguments.next().context("--view requires a value")?,
                "-h" | "--help" => {
                    println!(
                        "usage: codeweald-world-viewer --batch <concept-batch-directory> \
                         [--capture <png-path>] \
                         [--view overview|west-wall|east-wall|player] \
                         | --provenance"
                    );
                    std::process::exit(0);
                }
                unknown => bail!("unknown argument {unknown:?}"),
            }
        }
        if !matches!(
            capture_view.as_str(),
            "overview" | "west-wall" | "east-wall" | "player" | "border"
        ) {
            bail!(
                "unknown view {capture_view:?}; expected overview, west-wall, east-wall, player, or border"
            );
        }
        let batch = batch
            .context("--batch is required")?
            .canonicalize()
            .context("cannot resolve concept batch")?;
        let concept_batches = batch.parent().context("batch has no parent")?;
        let renderer_root = concept_batches
            .parent()
            .context("batch must be inside a concept_batches directory")?
            .to_path_buf();
        if !renderer_root.join("project.godot").is_file() {
            bail!(
                "{} is not a Codeweald renderer root",
                renderer_root.display()
            );
        }
        Ok(Self {
            batch,
            renderer_root,
            capture_path,
            capture_view,
        })
    }

    fn artifact(&self, relative: &str) -> PathBuf {
        self.batch.join(relative)
    }

    fn asset_relative(&self, path: &Path) -> Result<String> {
        Ok(path
            .strip_prefix(&self.renderer_root)
            .context("asset lies outside renderer root")?
            .to_string_lossy()
            .replace('\\', "/"))
    }
}

#[derive(Resource)]
struct ReloadState {
    timer: Timer,
    signature: u64,
    candidate_signature: u64,
    stable_polls: u8,
    force: bool,
}

impl Default for ReloadState {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(POLL_SECONDS, TimerMode::Repeating),
            signature: 0,
            candidate_signature: 0,
            stable_polls: 0,
            force: true,
        }
    }
}

#[derive(Resource, Default)]
struct ViewerStatus {
    text: String,
    error: Option<String>,
}

#[derive(Resource, Default)]
struct CameraFrame {
    transform: Option<Transform>,
    pending: bool,
}

#[derive(Resource, Default)]
struct CaptureState {
    settled_frames: u16,
    requested: bool,
}

#[derive(Resource, Default)]
struct SemanticOverlay {
    corridors: Vec<Vec<Vec3>>,
    hydrology: Vec<Vec<Vec3>>,
    show_corridors: bool,
    show_hydrology: bool,
}

/// Ground truth for the marker raycast. Kept as its own resource (rather than
/// read back off the terrain mesh) because the mesh is decimated by
/// TERRAIN_STRIDE for rendering -- marker placement should hit the same
/// heightfield the compiler certified, not a coarsened approximation of it.
#[derive(Resource, Default)]
struct TerrainHeightfield {
    heights: Vec<f32>,
    resolution: usize,
    width: f32,
    length: f32,
}

impl TerrainHeightfield {
    fn sample(&self, x: f32, z: f32) -> Option<f32> {
        if self.heights.is_empty() {
            return None;
        }
        let u = (x / self.width + 0.5).clamp(0.0, 1.0);
        let v = (0.5 - z / self.length).clamp(0.0, 1.0);
        let column = (u * (self.resolution - 1) as f32).round() as usize;
        let row = (v * (self.resolution - 1) as f32).round() as usize;
        Some(self.heights[row * self.resolution + column])
    }

    /// March the cursor ray to its first crossing of the heightfield, then
    /// bisect for a marker-placement-grade fix. Not used for anything the
    /// compiler certifies -- only for where a mark gets dropped.
    fn raycast(&self, origin: Vec3, direction: Vec3) -> Option<Vec3> {
        if self.heights.is_empty() || direction.y >= -0.0001 {
            return None;
        }
        let max_distance = self.width.max(self.length) * 2.0;
        let step = 1.0;
        let mut previous = origin;
        let mut t = 0.0;
        while t < max_distance {
            let point = origin + direction * t;
            let ground = self.sample(point.x, point.z)?;
            if point.y <= ground {
                let mut low = previous;
                let mut high = point;
                for _ in 0..12 {
                    let mid = low.lerp(high, 0.5);
                    let mid_ground = self.sample(mid.x, mid.z)?;
                    if mid.y <= mid_ground {
                        high = mid;
                    } else {
                        low = mid;
                    }
                }
                return Some(high);
            }
            previous = point;
            t += step;
        }
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkerCategory {
    Issue,
    Note,
    Good,
}

impl MarkerCategory {
    fn color(self) -> Color {
        match self {
            MarkerCategory::Issue => Color::srgb(0.95, 0.18, 0.15),
            MarkerCategory::Note => Color::srgb(0.95, 0.82, 0.15),
            MarkerCategory::Good => Color::srgb(0.25, 0.9, 0.35),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            MarkerCategory::Issue => "issue",
            MarkerCategory::Note => "note",
            MarkerCategory::Good => "good",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "note" => MarkerCategory::Note,
            "good" => MarkerCategory::Good,
            _ => MarkerCategory::Issue,
        }
    }
}

#[derive(Clone)]
struct MarkerEntry {
    position: Vec3,
    category: MarkerCategory,
}

/// Inspector marks: dropped in the 3D view, persisted to
/// `<batch>/inspector_marks.json` so Matt can flag something (like the
/// disconnected roofs) by walking up to it instead of screenshotting and
/// describing coordinates in chat.
#[derive(Resource)]
struct Markers {
    entries: Vec<MarkerEntry>,
    path: PathBuf,
}

impl Markers {
    fn load(path: PathBuf) -> Self {
        let entries = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|document| document.get("marks").cloned())
            .and_then(|marks| marks.as_array().cloned())
            .map(|marks| {
                marks
                    .iter()
                    .filter_map(|mark| {
                        let position = mark.get("position_m")?.as_array()?;
                        let [x, y, z] = [
                            position.first()?.as_f64()? as f32,
                            position.get(1)?.as_f64()? as f32,
                            position.get(2)?.as_f64()? as f32,
                        ];
                        let category = mark
                            .get("category")
                            .and_then(Value::as_str)
                            .map(MarkerCategory::from_str)
                            .unwrap_or(MarkerCategory::Issue);
                        Some(MarkerEntry {
                            position: Vec3::new(x, y, z),
                            category,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { entries, path }
    }

    fn save(&self) {
        let marks: Vec<Value> = self
            .entries
            .iter()
            .map(|entry| {
                json!({
                    "position_m": [entry.position.x, entry.position.y, entry.position.z],
                    "category": entry.category.as_str(),
                })
            })
            .collect();
        let document = json!({
            "schema_version": "codeweald.inspector-marks/v1",
            "marks": marks,
        });
        if let Ok(text) = serde_json::to_string_pretty(&document) {
            let _ = fs::write(&self.path, text);
        }
    }
}

#[derive(Resource)]
struct MarkerMode {
    active: bool,
    category: MarkerCategory,
}

impl Default for MarkerMode {
    fn default() -> Self {
        Self {
            active: false,
            category: MarkerCategory::Issue,
        }
    }
}

#[derive(Component)]
struct GeneratedWorld;

#[derive(Component)]
struct StatusText;

struct Placement {
    asset_path: String,
    transform: Transform,
}

/// The flat corridor ribbon: where the compiler *thinks* a road runs.
///
/// Hidden by default. The road a player sees is painted into the splatmap's
/// road channel and rendered by the terrain shader with a real material
/// (`caledonia_packed_dirt`); this untextured ribbon sat on top of it and hid
/// that art entirely. It is debug evidence, not world geometry, so it lives
/// behind the same key as the corridor centreline gizmo.
#[derive(Component)]
struct CorridorRibbon;

struct CompiledCorridor {
    id: String,
    mesh: Mesh,
}

struct CompiledWorld {
    zone_id: String,
    world_sha256: String,
    mesh: Mesh,
    style_texture: String,
    splat_texture: String,
    material_textures: [String; 4],
    rock_normal_texture: String,
    material_repeat_m: Vec4,
    material_mean_luma: Vec4,
    terrain_size_m: Vec2,
    placements: Vec<Placement>,
    corridors: Vec<CompiledCorridor>,
    overlay: SemanticOverlay,
    source_signature: u64,
    terrain_vertices: usize,
    camera_transform: Transform,
    heightfield: TerrainHeightfield,
    apron: Option<Apron>,
}

/// Land beyond the certified world, so the horizon does not end in void.
///
/// Built from `boundary_plan.json`'s declared rule rather than invented here:
/// the same numbers drive every backend's apron, so there is one apron and not
/// one per consumer. It carries no collider and is deliberately tinted apart
/// from the certified terrain -- an apron mistakable for real world would make
/// every outward capture evidence of ground that does not exist.
struct Apron {
    mesh: Mesh,
    colour: Color,
}

/// Material textures still waiting for a mip chain.
///
/// PNG carries no mip levels, and a tiled material sampled without them aliases
/// into moire the moment the camera pulls back -- which is why the terrain
/// shader previously admitted material detail only on steep faces, at a third
/// strength. Generating the chain here rather than shipping KTX2 from the
/// compiler keeps PNG as the portable artifact: Godot, Unity, and Unreal all
/// build mips on import, so emitting a pre-packed container would give every
/// other backend a harder file to read in order to fix a gap only this renderer
/// has.
#[derive(Resource, Default)]
struct PendingMipmaps(Vec<Handle<Image>>);

fn load_repeating_image(
    asset_server: &AssetServer,
    pending: &mut PendingMipmaps,
    path: String,
    is_srgb: bool,
) -> Handle<Image> {
    let handle = asset_server
        .load_builder()
        .with_settings(move |settings: &mut ImageLoaderSettings| {
            settings.is_srgb = is_srgb;
            settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::Repeat,
                address_mode_v: ImageAddressMode::Repeat,
                anisotropy_clamp: 8,
                mipmap_filter: ImageFilterMode::Linear,
                ..ImageSamplerDescriptor::linear()
            });
        })
        .load(path);
    pending.0.push(handle.clone());
    handle
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

/// Build a complete mip chain in place. Returns false if the image is not a
/// form this can handle, so the caller stops retrying it.
///
/// sRGB textures are averaged in linear space. Box-filtering encoded sRGB
/// directly biases every level brighter, and the error compounds down the chain
/// until distant terrain reads as a different material than near terrain.
fn generate_mipmaps(image: &mut Image) -> bool {
    let format = image.texture_descriptor.format;
    let encoded_srgb = match format {
        TextureFormat::Rgba8UnormSrgb => true,
        TextureFormat::Rgba8Unorm => false,
        _ => return false,
    };
    let mut width = image.texture_descriptor.size.width;
    let mut height = image.texture_descriptor.size.height;
    let Some(base) = image.data.as_ref() else {
        return false;
    };
    if base.len() < (width * height * 4) as usize {
        return false;
    }

    let mut chain = base[..(width * height * 4) as usize].to_vec();
    let mut current = chain.clone();
    let mut level_count = 1u32;
    while width > 1 || height > 1 {
        let next_width = (width / 2).max(1);
        let next_height = (height / 2).max(1);
        let mut next = vec![0u8; (next_width * next_height * 4) as usize];
        for y in 0..next_height {
            for x in 0..next_width {
                for channel in 0..4usize {
                    let mut total = 0.0f32;
                    for offset_y in 0..2u32 {
                        let source_y = (y * 2 + offset_y).min(height - 1);
                        for offset_x in 0..2u32 {
                            let source_x = (x * 2 + offset_x).min(width - 1);
                            let index =
                                ((source_y * width + source_x) as usize) * 4 + channel;
                            let value = f32::from(current[index]) / 255.0;
                            // Alpha is a coverage weight, never gamma encoded.
                            total += if encoded_srgb && channel < 3 {
                                srgb_to_linear(value)
                            } else {
                                value
                            };
                        }
                    }
                    let mean = total * 0.25;
                    let encoded = if encoded_srgb && channel < 3 {
                        linear_to_srgb(mean)
                    } else {
                        mean
                    };
                    next[((y * next_width + x) as usize) * 4 + channel] =
                        (encoded * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                }
            }
        }
        chain.extend_from_slice(&next);
        current = next;
        width = next_width;
        height = next_height;
        level_count += 1;
    }

    image.texture_descriptor.mip_level_count = level_count;
    image.data = Some(chain);
    true
}

/// Give every freshly loaded material texture its mip chain.
///
/// Loading is async, so a handle can sit here for several frames. Only images
/// that have arrived are touched: calling `get_mut` on every handle each frame
/// would mark them all modified and re-upload the set continuously.
fn build_pending_mipmaps(mut pending: ResMut<PendingMipmaps>, mut images: ResMut<Assets<Image>>) {
    if pending.0.is_empty() {
        return;
    }
    pending.0.retain(|handle| {
        let Some(image) = images.get(handle) else {
            return true;
        };
        if image.texture_descriptor.mip_level_count == 1
            && let Some(mut image) = images.get_mut(handle)
        {
            generate_mipmaps(&mut image);
        }
        // Loaded is done, whether or not a chain could be built. Retrying a
        // format this cannot handle would rebuild it every frame forever.
        false
    });
}

fn main() -> Result<()> {
    // Handled before ViewerConfig::from_args() (which requires --batch) so
    // provenance can be queried without a compiled batch on hand.
    if env::args().any(|argument| argument == "--provenance") {
        println!("{}", env!("CODEWEALD_SOURCE_DIGEST"));
        return Ok(());
    }
    let config = ViewerConfig::from_args()?;
    let asset_root = config.renderer_root.to_string_lossy().into_owned();
    let markers = Markers::load(config.artifact("inspector_marks.json"));
    App::new()
        .insert_resource(config)
        .insert_resource(ReloadState::default())
        .insert_resource(ViewerStatus::default())
        .insert_resource(CameraFrame::default())
        .insert_resource(CaptureState::default())
        .insert_resource(SemanticOverlay {
            show_corridors: false,
            show_hydrology: true,
            ..default()
        })
        .insert_resource(TerrainHeightfield::default())
        .insert_resource(markers)
        .insert_resource(MarkerMode::default())
        .insert_resource(ClearColor(Color::srgb(0.035, 0.045, 0.038)))
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: asset_root,
                    watch_for_changes_override: Some(true),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Codeweald World Viewer".into(),
                        resolution: (1440, 900).into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(FreeCameraPlugin)
        .add_plugins(MaterialPlugin::<TerrainMaterial>::default())
        .init_resource::<PendingMipmaps>()
        .add_systems(Startup, setup_viewer)
        .add_systems(
            Update,
            (
                monitor_compiled_world,
                build_pending_mipmaps.after(monitor_compiled_world),
                apply_camera_frame.after(monitor_compiled_world),
                draw_semantic_overlays,
                sync_corridor_ribbons,
                overlay_controls,
                update_status_text,
                capture_certified_world.after(apply_camera_frame),
            ),
        )
        .add_systems(
            Update,
            (
                toggle_marker_mode,
                place_marker.after(toggle_marker_mode),
                draw_markers,
            ),
        )
        .run();
    Ok(())
}

fn capture_certified_world(
    mut commands: Commands,
    config: Res<ViewerConfig>,
    mut capture: ResMut<CaptureState>,
    generated: Query<(), With<GeneratedWorld>>,
) {
    let Some(path) = config.capture_path.as_ref() else {
        return;
    };
    if capture.requested || generated.is_empty() {
        return;
    }
    // Imported glTF scenes and textures resolve asynchronously. Waiting after
    // the certified world root exists makes captures deterministic and avoids
    // evaluating a frame full of placeholders.
    capture.settled_frames = capture.settled_frames.saturating_add(1);
    if capture.settled_frames < 120 {
        return;
    }
    capture.requested = true;
    let path = path.clone();
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>, mut app_exit: MessageWriter<AppExit>| {
            save_to_disk(&path)(captured);
            app_exit.write(AppExit::Success);
        },
    );
}

fn setup_viewer(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let sky = images.add(build_inspection_skybox(48));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.42, 0.48, 0.52),
        brightness: 240.0,
        ..default()
    });
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-150.0, 125.0, 150.0).looking_at(Vec3::ZERO, Vec3::Y),
        Skybox {
            image: Some(sky),
            brightness: 520.0,
            ..default()
        },
        FreeCamera {
            sensitivity: 0.16,
            friction: 18.0,
            walk_speed: 28.0,
            run_speed: 110.0,
            scroll_factor: 0.0953102,
            key_up: KeyCode::Space,
            key_down: KeyCode::KeyZ,
            ..default()
        },
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.85, -0.55, 0.0)),
        CascadeShadowConfigBuilder {
            maximum_distance: 420.0,
            ..default()
        }
        .build(),
    ));
    // Inspection lighting must expose geometry on both sides of a massif.
    // A low-intensity shadowless fill avoids the former pitch-black western
    // wall without flattening the primary sun direction.
    commands.spawn((
        DirectionalLight {
            illuminance: 2_800.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.55, 2.35, 0.0)),
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            padding: UiRect::all(px(9)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.015, 0.02, 0.018, 0.84)),
        children![(
            StatusText,
            Text::new("Loading certified world…"),
            TextFont {
                font_size: FontSize::Px(16.0),
                ..default()
            },
            TextColor(Color::srgb(0.82, 0.94, 0.86)),
        )],
    ));
}

fn build_inspection_skybox(size: u32) -> Image {
    let mut pixels = Vec::with_capacity((size * size * 6 * 4) as usize);
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let u = (2.0 * (x as f32 + 0.5) / size as f32) - 1.0;
                let v = (2.0 * (y as f32 + 0.5) / size as f32) - 1.0;
                let direction = match face {
                    0 => Vec3::new(1.0, -v, -u),
                    1 => Vec3::new(-1.0, -v, u),
                    2 => Vec3::new(u, 1.0, v),
                    3 => Vec3::new(u, -1.0, -v),
                    4 => Vec3::new(u, -v, 1.0),
                    _ => Vec3::new(-u, -v, -1.0),
                }
                .normalize();
                let horizon = Vec3::new(0.43, 0.55, 0.63);
                let color = if direction.y >= 0.0 {
                    horizon.lerp(Vec3::new(0.10, 0.23, 0.43), direction.y.powf(0.55))
                } else {
                    horizon.lerp(Vec3::new(0.15, 0.18, 0.17), (-direction.y).powf(0.4))
                };
                pixels.extend(
                    color
                        .to_array()
                        .map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8),
                );
                pixels.push(255);
            }
        }
    }
    Image {
        texture_view_descriptor: Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::Cube),
            ..default()
        }),
        ..Image::new(
            Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 6,
            },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        )
    }
}

fn apply_camera_frame(
    keys: Res<ButtonInput<KeyCode>>,
    mut frame: ResMut<CameraFrame>,
    mut camera: Single<&mut Transform, With<Camera3d>>,
) {
    if !frame.pending && !keys.just_pressed(KeyCode::KeyF) {
        return;
    }
    frame.pending = false;
    if let Some(transform) = frame.transform.clone() {
        **camera = transform;
    }
}

fn source_files(config: &ViewerConfig) -> [PathBuf; 7] {
    [
        config.artifact("zone_spec.json"),
        config.artifact("asset_plan.json"),
        config.artifact("asset_visual_preflight/asset_visual_preflight_report.json"),
        config.artifact("placement_plan.json"),
        config.artifact("render_plan.json"),
        config.artifact("terrain/terrain_manifest.json"),
        config.artifact("terrain/heightfield_f32le.bin"),
    ]
}

fn source_signature(config: &ViewerConfig) -> Result<u64> {
    let mut hasher = DefaultHasher::new();
    for path in source_files(config) {
        path.hash(&mut hasher);
        let metadata =
            fs::metadata(&path).with_context(|| format!("cannot stat {}", path.display()))?;
        metadata.len().hash(&mut hasher);
        metadata
            .modified()
            .unwrap_or(UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .hash(&mut hasher);
    }
    Ok(hasher.finish())
}

#[allow(clippy::too_many_arguments)]
fn monitor_compiled_world(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    config: Res<ViewerConfig>,
    mut reload: ResMut<ReloadState>,
    mut status: ResMut<ViewerStatus>,
    mut camera_frame: ResMut<CameraFrame>,
    mut overlay: ResMut<SemanticOverlay>,
    generated: Query<Entity, With<GeneratedWorld>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    asset_server: Res<AssetServer>,
    mut pending_mipmaps: ResMut<PendingMipmaps>,
    mut heightfield: ResMut<TerrainHeightfield>,
) {
    reload.timer.tick(time.delta());
    reload.force |= keys.just_pressed(KeyCode::KeyR);
    if !reload.force && !reload.timer.just_finished() {
        return;
    }
    // The compiler replaces several hash-bound artifacts in sequence. Do not
    // interpret its intentionally mixed intermediate state as a corrupt world.
    if config.artifact(".build-in-progress").is_file() {
        return;
    }
    let signature = match source_signature(&config) {
        Ok(value) => value,
        Err(error) => {
            status.error = Some(format!("{error:#}"));
            return;
        }
    };
    let forced = reload.force;
    reload.force = false;
    if !forced && signature == reload.signature {
        return;
    }
    if !forced {
        if signature != reload.candidate_signature {
            reload.candidate_signature = signature;
            reload.stable_polls = 0;
            return;
        }
        reload.stable_polls = reload.stable_polls.saturating_add(1);
        if reload.stable_polls < 3 {
            return;
        }
    }
    match load_compiled_world(&config, signature) {
        Ok(world) => {
            for entity in &generated {
                commands.entity(entity).despawn();
            }
            *heightfield = world.heightfield;
            let terrain = meshes.add(world.mesh);
            let style_texture = asset_server.load(world.style_texture.clone());
            let terrain_material = terrain_materials.add(ExtendedMaterial {
                base: StandardMaterial {
                    base_color: Color::WHITE,
                    base_color_texture: Some(style_texture),
                    // The baked roughness map remains a portable artifact, but
                    // PNG carries no mip chain. Enabling it here aliases badly
                    // at grazing terrain angles; the triplanar material lane
                    // will consume packed mipmapped maps once emitted.
                    metallic_roughness_texture: None,
                    perceptual_roughness: 0.96,
                    metallic: 0.0,
                    ..default()
                },
                extension: TerrainExtension {
                    splat: asset_server.load(world.splat_texture.clone()),
                    grass: load_repeating_image(
                        &asset_server,
                        &mut pending_mipmaps,
                        world.material_textures[0].clone(),
                        true,
                    ),
                    road: load_repeating_image(
                        &asset_server,
                        &mut pending_mipmaps,
                        world.material_textures[1].clone(),
                        true,
                    ),
                    rock: load_repeating_image(
                        &asset_server,
                        &mut pending_mipmaps,
                        world.material_textures[2].clone(),
                        true,
                    ),
                    snow: load_repeating_image(
                        &asset_server,
                        &mut pending_mipmaps,
                        world.material_textures[3].clone(),
                        true,
                    ),
                    rock_normal: load_repeating_image(
                        &asset_server,
                        &mut pending_mipmaps,
                        world.rock_normal_texture.clone(),
                        false,
                    ),
                    settings: TerrainUniform {
                        repeat_m: world.material_repeat_m,
                        material_mean_luma: world.material_mean_luma,
                        world_size_m: world.terrain_size_m,
                        _padding: Vec2::ZERO,
                    },
                },
            });
            let corridor_material = materials.add(StandardMaterial {
                base_color: Color::srgb(0.34, 0.23, 0.105),
                perceptual_roughness: 0.99,
                metallic: 0.0,
                cull_mode: None,
                ..default()
            });
            let apron = world.apron.map(|apron| {
                (
                    meshes.add(apron.mesh),
                    materials.add(StandardMaterial {
                        base_color: apron.colour,
                        perceptual_roughness: 1.0,
                        metallic: 0.0,
                        ..default()
                    }),
                )
            });
            let corridor_count = world.corridors.len();
            let corridor_meshes = world
                .corridors
                .into_iter()
                .map(|corridor| (corridor.id, meshes.add(corridor.mesh)))
                .collect::<Vec<_>>();
            let root = commands
                .spawn((
                    GeneratedWorld,
                    Name::new(world.zone_id.clone()),
                    Transform::default(),
                    Visibility::default(),
                ))
                .id();
            let placement_count = world.placements.len();
            commands.entity(root).with_children(|parent| {
                parent.spawn((
                    Name::new("CertifiedTerrain"),
                    Mesh3d(terrain),
                    MeshMaterial3d(terrain_material),
                ));
                if let Some((mesh, material)) = apron {
                    // Named apart from CertifiedTerrain on purpose: anything
                    // walking this scene graph must be able to tell scenery
                    // from certified world.
                    parent.spawn((
                        Name::new("Apron"),
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                    ));
                }
                for (id, corridor) in corridor_meshes {
                    parent.spawn((
                        Name::new(format!("CompiledCorridor:{id}")),
                        CorridorRibbon,
                        Mesh3d(corridor),
                        MeshMaterial3d(corridor_material.clone()),
                        // Off unless asked for: it covers the terrain's own
                        // road material, which is the art we are trying to see.
                        Visibility::Hidden,
                    ));
                }
                for placement in world.placements {
                    let scene = asset_server
                        .load(GltfAssetLabel::Scene(0).from_asset(placement.asset_path));
                    parent.spawn((WorldAssetRoot(scene), placement.transform));
                }
            });
            *overlay = world.overlay;
            camera_frame.transform = Some(world.camera_transform);
            camera_frame.pending = true;
            status.text = format!(
                "{}\nworld {}\n{} terrain vertices · {} certified placements\n\
                 {} compiled roads · auto reload 500 ms · R reload · F frame · 1 road debug\n\
                 K mark mode · 3/4/5 issue/note/good · click to drop · Backspace undo",
                world.zone_id,
                &world.world_sha256[..12],
                world.terrain_vertices,
                placement_count,
                corridor_count,
            );
            status.error = None;
            reload.signature = world.source_signature;
            reload.candidate_signature = world.source_signature;
            reload.stable_polls = 0;
            info!("loaded {}", status.text.replace('\n', " | "));
        }
        Err(error) => {
            status.error = Some(format!("{error:#}"));
            // A stable but invalid transaction is reported once. Any later
            // artifact write changes the signature and automatically retries.
            reload.signature = signature;
            reload.stable_polls = 0;
            error!("compiled-world reload rejected: {error:#}");
        }
    }
}

fn load_compiled_world(config: &ViewerConfig, signature: u64) -> Result<CompiledWorld> {
    let zone_text = fs::read_to_string(config.artifact("zone_spec.json"))
        .context("cannot read zone_spec.json")?;
    let zone: Value = serde_json::from_str(&zone_text).context("cannot parse zone_spec.json")?;
    let identity = validate_value(&zone).map_err(|error| anyhow::anyhow!("{error}"))?;
    let zone_id = zone
        .pointer("/zone/id")
        .and_then(Value::as_str)
        .context("ZoneSpec has no zone id")?
        .to_owned();

    let manifest: Value = serde_json::from_slice(
        &fs::read(config.artifact("terrain/terrain_manifest.json"))
            .context("cannot read terrain manifest")?,
    )
    .context("cannot parse terrain manifest")?;
    if manifest.get("zone_spec_sha256").and_then(Value::as_str)
        != Some(identity.canonical_sha256.as_str())
    {
        bail!("terrain manifest is not bound to the current ZoneSpec");
    }
    let resolution = manifest
        .get("resolution")
        .and_then(Value::as_u64)
        .context("terrain manifest has no resolution")? as usize;
    let width = manifest
        .pointer("/world_bounds_m/width")
        .and_then(Value::as_f64)
        .context("terrain width is missing")? as f32;
    let length = manifest
        .pointer("/world_bounds_m/length")
        .and_then(Value::as_f64)
        .context("terrain length is missing")? as f32;
    let height_bytes = fs::read(config.artifact("terrain/heightfield_f32le.bin"))
        .context("cannot read float32 heightfield")?;
    let heights = decode_heightfield(&height_bytes, resolution)?;
    let (height_min, height_max) = height_range(&heights)?;
    let mesh = build_terrain_mesh(&heights, resolution, width, length)?;
    let terrain_vertices = mesh.count_vertices();

    let landform_plan: Value = serde_json::from_slice(
        &fs::read(config.artifact("placement_plan.json")).context("cannot read placement plan")?,
    )
    .context("cannot parse placement plan")?;
    let asset_plan: Value = serde_json::from_slice(
        &fs::read(config.artifact("asset_plan.json")).context("cannot read asset plan")?,
    )
    .context("cannot parse asset plan")?;
    let asset_preflight: Value = serde_json::from_slice(
        &fs::read(config.artifact("asset_visual_preflight/asset_visual_preflight_report.json"))
            .context("cannot read asset preflight")?,
    )
    .context("cannot parse asset preflight")?;
    let render_plan: Value = serde_json::from_slice(
        &fs::read(config.artifact("render_plan.json")).context("cannot read render plan")?,
    )
    .context("cannot parse render plan")?;
    validate_render_plan_value(
        &zone,
        &asset_plan,
        &asset_preflight,
        &manifest,
        &height_bytes,
        &landform_plan,
        &render_plan,
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    let mut scene_paths = BTreeMap::<String, String>::new();
    collect_asset_paths(&asset_plan, &mut scene_paths);
    let mut placements = Vec::new();
    for record in render_plan
        .get("instances")
        .and_then(Value::as_array)
        .context("render-plan instances are missing")?
    {
        let digest = record
            .get("asset_sha256")
            .and_then(Value::as_str)
            .context("placement asset digest is missing")?;
        let asset_path = scene_paths
            .get(digest)
            .with_context(|| format!("render plan references unmapped asset {digest}"))?
            .clone();
        let position = number_array(record.get("position_m"), 3, "placement position")?;
        let yaw = record
            .get("yaw_degrees")
            .and_then(Value::as_f64)
            .context("placement yaw is missing")? as f32;
        let scale = record
            .get("scale")
            .and_then(Value::as_f64)
            .context("placement scale is missing")? as f32;
        placements.push(Placement {
            asset_path,
            transform: Transform {
                translation: Vec3::new(position[0], position[1], position[2]),
                rotation: Quat::from_rotation_y(yaw.to_radians()),
                scale: Vec3::splat(scale),
            },
        });
    }
    let corridors = render_plan
        .get("corridors")
        .and_then(Value::as_array)
        .context("render-plan corridors are missing")?
        .iter()
        .map(|record| {
            let id = record
                .get("id")
                .and_then(Value::as_str)
                .context("corridor id is missing")?
                .to_owned();
            let width = record
                .get("width_m")
                .and_then(Value::as_f64)
                .context("corridor width is missing")? as f32;
            let points = record
                .get("centerline_m")
                .and_then(Value::as_array)
                .context("corridor centerline is missing")?
                .iter()
                .map(|point| {
                    let values = number_array(Some(point), 3, "corridor point")?;
                    Ok(Vec3::new(values[0], values[1], values[2]))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(CompiledCorridor {
                id,
                mesh: build_corridor_mesh(&points, width)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let sampler = TerrainSampler {
        heights: &heights,
        resolution,
        width,
        length,
    };
    let overlay = build_overlay(&zone, &sampler)?;
    // The semantic preview supplies a stable low-frequency ground layer. The
    // terrain extension consumes the authoritative splat and verified source
    // bundles directly for world-space detail on steep faces.
    let style_texture = config.asset_relative(&config.artifact("terrain/terrain_preview.png"))?;
    let splat_texture = config.asset_relative(&config.artifact("terrain/splatmap.png"))?;
    let mut material_textures = Vec::with_capacity(4);
    let mut material_repeats = Vec::with_capacity(4);
    let mut material_mean_luma = Vec::with_capacity(4);
    for layer in ["grass", "road", "rock", "snow"] {
        let albedo = manifest
            .pointer(&format!("/terrain_materials/{layer}/maps/albedo/path"))
            .and_then(Value::as_str)
            .with_context(|| format!("terrain material {layer} has no albedo"))?;
        material_textures.push(config.asset_relative(&config.renderer_root.join(albedo))?);
        material_repeats.push(
            manifest
                .pointer(&format!("/terrain_materials/{layer}/meters_per_repeat"))
                .and_then(Value::as_f64)
                .with_context(|| format!("terrain material {layer} has no repeat scale"))?
                as f32,
        );
        material_mean_luma.push(
            manifest
                .pointer(&format!(
                    "/terrain_materials/{layer}/albedo_mean_linear_luminance"
                ))
                .and_then(Value::as_f64)
                .with_context(|| {
                    format!("terrain material {layer} has no mean albedo luminance")
                })? as f32,
        );
    }
    let material_textures: [String; 4] = material_textures
        .try_into()
        .map_err(|_| anyhow::anyhow!("terrain material texture contract is incomplete"))?;
    let rock_normal = manifest
        .pointer("/terrain_materials/rock/maps/normal/path")
        .and_then(Value::as_str)
        .context("terrain rock material has no normal map")?;
    // The apron is built from the compiler's declared rule, never from numbers
    // chosen here -- otherwise the viewer's horizon and every backend's horizon
    // would be two different worlds that happen to look similar.
    let apron = match fs::read(config.artifact("boundary_plan.json")) {
        Ok(bytes) => {
            let plan: Value =
                serde_json::from_slice(&bytes).context("cannot parse boundary plan")?;
            let extent = plan
                .pointer("/apron/extent_m")
                .and_then(Value::as_f64)
                .context("boundary plan declares no apron extent")? as f32;
            let depth = plan
                .pointer("/apron/falloff_depth_m")
                .and_then(Value::as_f64)
                .context("boundary plan declares no apron falloff depth")?
                as f32;
            let rock = manifest
                .pointer("/style_palette_srgb/rock")
                .and_then(Value::as_array)
                .map(|channels| {
                    let component = |index: usize| {
                        channels
                            .get(index)
                            .and_then(Value::as_f64)
                            .unwrap_or(0.2) as f32
                    };
                    // Deliberately darker than the palette: the apron should
                    // read as distance, and should never be mistaken in a
                    // capture for ground a player could reach.
                    Color::srgb(
                        component(0) * 0.7,
                        component(1) * 0.7,
                        component(2) * 0.75,
                    )
                })
                .unwrap_or(Color::srgb(0.11, 0.12, 0.14));
            Some(Apron {
                mesh: build_apron_mesh(&heights, resolution, width, length, extent, depth)?,
                colour: rock,
            })
        }
        Err(_) => {
            // An older batch predating roadmap 1.2. Say so rather than
            // silently rendering the cliff that this exists to remove.
            eprintln!(
                "boundary_plan.json is absent: rendering without an apron, so the \
                 world edge will drop into void. Rebuild the batch to emit it."
            );
            None
        }
    };

    Ok(CompiledWorld {
        zone_id,
        world_sha256: identity.canonical_sha256,
        apron,
        mesh,
        style_texture,
        splat_texture,
        material_textures,
        rock_normal_texture: config.asset_relative(&config.renderer_root.join(rock_normal))?,
        material_repeat_m: Vec4::from_array(
            material_repeats
                .try_into()
                .map_err(|_| anyhow::anyhow!("terrain material repeat contract is incomplete"))?,
        ),
        material_mean_luma: Vec4::from_array(material_mean_luma.try_into().map_err(|_| {
            anyhow::anyhow!("terrain material luminance contract is incomplete")
        })?),
        terrain_size_m: Vec2::new(width, length),
        placements,
        corridors,
        overlay,
        source_signature: signature,
        terrain_vertices,
        camera_transform: inspection_camera(
            &config.capture_view,
            width,
            length,
            height_min,
            height_max,
        ),
        heightfield: TerrainHeightfield {
            heights,
            resolution,
            width,
            length,
        },
    })
}

fn build_corridor_mesh(points: &[Vec3], width: f32) -> Result<Mesh> {
    if points.len() < 2 || !width.is_finite() || width <= 0.0 {
        bail!("corridor needs at least two points and a positive width");
    }
    let mut positions = Vec::with_capacity(points.len() * 2);
    let mut normals = Vec::with_capacity(points.len() * 2);
    let mut uvs = Vec::with_capacity(points.len() * 2);
    let mut travelled = 0.0;
    for (index, point) in points.iter().enumerate() {
        if index > 0 {
            travelled += point.distance(points[index - 1]);
        }
        let previous = points[index.saturating_sub(1)];
        let next = points[(index + 1).min(points.len() - 1)];
        let tangent = Vec3::new(next.x - previous.x, 0.0, next.z - previous.z).normalize_or_zero();
        if tangent.length_squared() <= 1e-8 {
            bail!("corridor contains a zero-length tangent");
        }
        let side = Vec3::new(-tangent.z, 0.0, tangent.x) * (width * 0.5);
        let lift = Vec3::Y * 0.045;
        positions.push((*point - side + lift).to_array());
        positions.push((*point + side + lift).to_array());
        normals.extend_from_slice(&[Vec3::Y.to_array(), Vec3::Y.to_array()]);
        uvs.push([0.0, travelled / 4.0]);
        uvs.push([1.0, travelled / 4.0]);
    }
    let mut indices = Vec::with_capacity((points.len() - 1) * 6);
    for index in 0..points.len() - 1 {
        let left = (index * 2) as u32;
        let right = left + 1;
        let next_left = left + 2;
        let next_right = left + 3;
        indices.extend_from_slice(&[left, next_left, right, right, next_left, next_right]);
    }
    Ok(Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_indices(Indices::U32(indices))
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs))
}

fn collect_asset_paths(value: &Value, paths: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(object) => {
            if let (Some(digest), Some(path)) = (
                object.get("sha256").and_then(Value::as_str),
                object.get("source_path").and_then(Value::as_str),
            ) {
                paths.insert(
                    digest.to_owned(),
                    path.trim_start_matches("res://").to_owned(),
                );
            }
            for child in object.values() {
                collect_asset_paths(child, paths);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_asset_paths(child, paths);
            }
        }
        _ => {}
    }
}

fn height_range(heights: &[f32]) -> Result<(f32, f32)> {
    let minimum = heights
        .iter()
        .copied()
        .reduce(f32::min)
        .context("heightfield is empty")?;
    let maximum = heights
        .iter()
        .copied()
        .reduce(f32::max)
        .context("heightfield is empty")?;
    if !minimum.is_finite() || !maximum.is_finite() {
        bail!("heightfield contains non-finite values");
    }
    Ok((minimum, maximum))
}

fn overview_camera(width: f32, length: f32, height_min: f32, height_max: f32) -> Transform {
    let extent = width.max(length);
    let center = Vec3::new(0.0, (height_min + height_max) * 0.5, 0.0);
    Transform::from_translation(center + Vec3::new(extent * -0.18, extent * 1.02, extent * 1.02))
        .looking_at(center, Vec3::Y)
}

fn inspection_camera(
    view: &str,
    width: f32,
    length: f32,
    height_min: f32,
    height_max: f32,
) -> Transform {
    if view == "overview" {
        return overview_camera(width, length, height_min, height_max);
    }
    let relief = (height_max - height_min).max(1.0);
    let eye_y = height_min + (relief * 0.32).max(14.0);
    let target_y = height_min + relief * 0.48;
    match view {
        "west-wall" => Transform::from_xyz(width * 0.08, eye_y, 0.0)
            .looking_at(Vec3::new(-width * 0.43, target_y, 0.0), Vec3::Y),
        "east-wall" => Transform::from_xyz(-width * 0.08, eye_y, 0.0)
            .looking_at(Vec3::new(width * 0.43, target_y, 0.0), Vec3::Y),
        // The only view that looks *outward*. Every other inspection view
        // points into the map, which is why the border cliff was found by
        // flying the free camera and never by a capture: the automated
        // evidence had no view of the defect it was supposed to catch.
        "border" => Transform::from_xyz(0.0, eye_y, length * 0.20).looking_at(
            Vec3::new(0.0, height_min, length * 0.95),
            Vec3::Y,
        ),
        "player" => Transform::from_xyz(0.0, eye_y, length * 0.34).looking_at(
            Vec3::new(0.0, height_min + relief * 0.18, -length * 0.28),
            Vec3::Y,
        ),
        _ => overview_camera(width, length, height_min, height_max),
    }
}

fn number_array(value: Option<&Value>, expected: usize, label: &str) -> Result<Vec<f32>> {
    let values = value
        .and_then(Value::as_array)
        .with_context(|| format!("{label} is not an array"))?;
    if values.len() != expected {
        bail!("{label} must have {expected} values");
    }
    values
        .iter()
        .map(|value| {
            value
                .as_f64()
                .map(|number| number as f32)
                .with_context(|| format!("{label} contains a non-number"))
        })
        .collect()
}

fn decode_heightfield(bytes: &[u8], resolution: usize) -> Result<Vec<f32>> {
    let expected = resolution
        .checked_mul(resolution)
        .and_then(|count| count.checked_mul(4))
        .context("heightfield resolution overflow")?;
    if bytes.len() != expected {
        bail!("heightfield has {} bytes; expected {expected}", bytes.len());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

fn build_terrain_mesh(heights: &[f32], resolution: usize, width: f32, length: f32) -> Result<Mesh> {
    if resolution < 2 || (resolution - 1) % TERRAIN_STRIDE != 0 {
        bail!("resolution {resolution} is incompatible with stride {TERRAIN_STRIDE}");
    }
    let grid = (resolution - 1) / TERRAIN_STRIDE + 1;
    let mut positions = Vec::with_capacity(grid * grid);
    let mut normals = Vec::with_capacity(grid * grid);
    let mut uvs = Vec::with_capacity(grid * grid);
    let dx = width / (resolution - 1) as f32;
    let dz = length / (resolution - 1) as f32;
    for row in 0..grid {
        let source_row = row * TERRAIN_STRIDE;
        for column in 0..grid {
            let source_column = column * TERRAIN_STRIDE;
            let u = source_column as f32 / (resolution - 1) as f32;
            let v = source_row as f32 / (resolution - 1) as f32;
            let height = heights[source_row * resolution + source_column];
            positions.push([(u - 0.5) * width, height, (0.5 - v) * length]);
            let left =
                heights[source_row * resolution + source_column.saturating_sub(TERRAIN_STRIDE)];
            let right = heights
                [source_row * resolution + (source_column + TERRAIN_STRIDE).min(resolution - 1)];
            let up =
                heights[source_row.saturating_sub(TERRAIN_STRIDE) * resolution + source_column];
            let down = heights
                [(source_row + TERRAIN_STRIDE).min(resolution - 1) * resolution + source_column];
            let normal = Vec3::new(
                (left - right) / (2.0 * dx * TERRAIN_STRIDE as f32),
                1.0,
                (down - up) / (2.0 * dz * TERRAIN_STRIDE as f32),
            )
            .normalize_or_zero();
            normals.push(normal.to_array());
            uvs.push([u, v]);
        }
    }
    let mut indices = Vec::with_capacity((grid - 1) * (grid - 1) * 6);
    for row in 0..grid - 1 {
        for column in 0..grid - 1 {
            let a = (row * grid + column) as u32;
            let b = a + 1;
            let c = a + grid as u32;
            let d = c + 1;
            indices.extend_from_slice(&[a, b, c, b, d, c]);
        }
    }
    Ok(Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_indices(Indices::U32(indices))
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs))
}

/// Outward rings of apron geometry per side.
///
/// Spaced quadratically so they bunch against the seam, where the silhouette
/// actually reads, and stretch out toward the horizon where it does not.
const APRON_RINGS: usize = 8;

fn apron_axis(half: f32, extent: f32, inner: usize) -> Vec<f32> {
    let mut axis = Vec::with_capacity(inner + APRON_RINGS * 2);
    for ring in 0..APRON_RINGS {
        let amount = (APRON_RINGS - ring) as f32 / APRON_RINGS as f32;
        axis.push(-half - extent * amount * amount);
    }
    for index in 0..inner {
        axis.push(-half + 2.0 * half * index as f32 / (inner - 1) as f32);
    }
    for ring in 1..=APRON_RINGS {
        let amount = ring as f32 / APRON_RINGS as f32;
        axis.push(half + extent * amount * amount);
    }
    axis
}

/// The skirt of land beyond the certified world.
///
/// Vertices on the shared boundary are sampled from the heightfield at exactly
/// the positions the terrain mesh uses, so the seam closes rather than cracking
/// open a thin line of void -- which would be a more embarrassing version of
/// the defect this exists to fix. Beyond the boundary the edge height falls
/// away by `depth` over `extent`, so the world reads as land receding rather
/// than as a plateau on a table.
fn build_apron_mesh(
    heights: &[f32],
    resolution: usize,
    width: f32,
    length: f32,
    extent: f32,
    depth: f32,
) -> Result<Mesh> {
    if !extent.is_finite() || extent <= 0.0 {
        bail!("apron extent must be positive, got {extent}");
    }
    if (resolution - 1) % TERRAIN_STRIDE != 0 {
        bail!("resolution {resolution} is incompatible with stride {TERRAIN_STRIDE}");
    }
    let inner = (resolution - 1) / TERRAIN_STRIDE + 1;
    let (half_width, half_length) = (width * 0.5, length * 0.5);
    let xs = apron_axis(half_width, extent, inner);
    let zs = apron_axis(half_length, extent, inner);
    let sampler = TerrainSampler {
        heights,
        resolution,
        width,
        length,
    };

    let height_at = |x: f32, z: f32| -> f32 {
        let outside = (x.abs() - half_width)
            .max(z.abs() - half_length)
            .max(0.0);
        let amount = (outside / extent).clamp(0.0, 1.0);
        sampler.sample(x.clamp(-half_width, half_width), z.clamp(-half_length, half_length))
            - depth * amount
    };

    let mut positions = Vec::with_capacity(xs.len() * zs.len());
    for &z in &zs {
        for &x in &xs {
            positions.push([x, height_at(x, z), z]);
        }
    }
    let mut normals = Vec::with_capacity(positions.len());
    for (row, &z) in zs.iter().enumerate() {
        for (column, &x) in xs.iter().enumerate() {
            let west = xs[column.saturating_sub(1)];
            let east = xs[(column + 1).min(xs.len() - 1)];
            let north = zs[row.saturating_sub(1)];
            let south = zs[(row + 1).min(zs.len() - 1)];
            let dx = (east - west).max(1e-3);
            let dz = (south - north).max(1e-3);
            normals.push(
                Vec3::new(
                    (height_at(west, z) - height_at(east, z)) / dx,
                    1.0,
                    (height_at(x, north) - height_at(x, south)) / dz,
                )
                .normalize_or_zero()
                .to_array(),
            );
        }
    }

    // The axes place a grid line exactly on each world edge, so no quad ever
    // straddles the boundary: a quad is either wholly the certified interior
    // (skipped, the terrain mesh owns it) or wholly apron.
    let interior = APRON_RINGS..APRON_RINGS + inner - 1;
    let stride = xs.len();
    let mut indices = Vec::new();
    for row in 0..zs.len() - 1 {
        for column in 0..xs.len() - 1 {
            if interior.contains(&row) && interior.contains(&column) {
                continue;
            }
            let a = (row * stride + column) as u32;
            let (b, c) = (a + 1, a + stride as u32);
            // Wound opposite to `build_terrain_mesh`. The terrain grid walks z
            // *down* as its row index rises; this grid walks z up, which
            // mirrors every triangle. Copying the terrain's winding here made
            // the whole apron backface-culled -- built, spawned, and invisible,
            // which an A/B capture caught as zero differing pixels.
            indices.extend_from_slice(&[a, c, b, b, c, c + 1]);
        }
    }
    if indices.is_empty() {
        bail!("apron produced no geometry");
    }
    Ok(Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_indices(Indices::U32(indices))
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals))
}

struct TerrainSampler<'a> {
    heights: &'a [f32],
    resolution: usize,
    width: f32,
    length: f32,
}

impl TerrainSampler<'_> {
    fn sample(&self, x: f32, z: f32) -> f32 {
        let u = (x / self.width + 0.5).clamp(0.0, 1.0);
        let v = (0.5 - z / self.length).clamp(0.0, 1.0);
        let column = (u * (self.resolution - 1) as f32).round() as usize;
        let row = (v * (self.resolution - 1) as f32).round() as usize;
        self.heights[row * self.resolution + column]
    }
}

fn build_overlay(zone: &Value, sampler: &TerrainSampler<'_>) -> Result<SemanticOverlay> {
    let mut overlay = SemanticOverlay {
        show_corridors: false,
        show_hydrology: true,
        ..default()
    };
    for feature in zone
        .get("features")
        .and_then(Value::as_array)
        .context("ZoneSpec features are missing")?
    {
        let category = feature
            .get("category")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if category != "corridor" && category != "hydrology" {
            continue;
        }
        if category == "hydrology"
            && feature
                .pointer("/properties/channel_profile")
                .and_then(Value::as_str)
                == Some("wetland_rill")
        {
            // A wetland trace drives a broken pool/rill field in the terrain
            // compiler; drawing it as one river centerline is misleading.
            continue;
        }
        let Some(points) = feature
            .pointer("/geometry/points")
            .and_then(Value::as_array)
        else {
            continue;
        };
        let mut line = Vec::with_capacity(points.len());
        for point in points {
            let values = number_array(Some(point), 2, "feature point")?;
            line.push(Vec3::new(
                values[0],
                sampler.sample(values[0], values[1]) + 0.65,
                values[1],
            ));
        }
        if category == "corridor" {
            overlay.corridors.push(line);
        } else {
            overlay.hydrology.push(line);
        }
    }
    Ok(overlay)
}

/// The ribbon follows the corridor debug toggle, so one key answers "where does
/// the compiler think the roads are" with both the centreline and the footprint.
fn sync_corridor_ribbons(
    overlay: Res<SemanticOverlay>,
    mut ribbons: Query<&mut Visibility, With<CorridorRibbon>>,
) {
    if !overlay.is_changed() {
        return;
    }
    let wanted = if overlay.show_corridors {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut ribbons {
        *visibility = wanted;
    }
}

fn draw_semantic_overlays(mut gizmos: Gizmos, overlay: Res<SemanticOverlay>) {
    if overlay.show_corridors {
        for line in &overlay.corridors {
            for segment in line.windows(2) {
                gizmos.line(segment[0], segment[1], Color::srgb(0.95, 0.74, 0.22));
            }
        }
    }
    if overlay.show_hydrology {
        for line in &overlay.hydrology {
            for segment in line.windows(2) {
                gizmos.line(segment[0], segment[1], Color::srgb(0.12, 0.72, 0.96));
            }
        }
    }
}

fn overlay_controls(keys: Res<ButtonInput<KeyCode>>, mut overlay: ResMut<SemanticOverlay>) {
    if keys.just_pressed(KeyCode::Digit1) {
        overlay.show_corridors = !overlay.show_corridors;
    }
    let _ = &overlay;
    if keys.just_pressed(KeyCode::Digit2) {
        overlay.show_hydrology = !overlay.show_hydrology;
    }
}

fn toggle_marker_mode(keys: Res<ButtonInput<KeyCode>>, mut mode: ResMut<MarkerMode>) {
    if keys.just_pressed(KeyCode::KeyK) {
        mode.active = !mode.active;
    }
    if keys.just_pressed(KeyCode::Digit3) {
        mode.category = MarkerCategory::Issue;
    }
    if keys.just_pressed(KeyCode::Digit4) {
        mode.category = MarkerCategory::Note;
    }
    if keys.just_pressed(KeyCode::Digit5) {
        mode.category = MarkerCategory::Good;
    }
}

/// K toggles marker mode; while active, left click drops a mark of the
/// selected category (3=issue, 4=note, 5=good; issue is the default) at the
/// terrain point under the cursor, and Backspace undoes the last one. Marks
/// persist to inspector_marks.json immediately, so they survive a viewer
/// restart and are readable straight off disk without a screenshot.
fn place_marker(
    mode: Res<MarkerMode>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    heightfield: Res<TerrainHeightfield>,
    mut markers: ResMut<Markers>,
) {
    if !mode.active {
        return;
    }
    if keys.just_pressed(KeyCode::Backspace) {
        if markers.entries.pop().is_some() {
            markers.save();
        }
        return;
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (camera, camera_transform) = *camera;
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let Some(hit) = heightfield.raycast(ray.origin, *ray.direction) else {
        return;
    };
    markers.entries.push(MarkerEntry {
        position: hit,
        category: mode.category,
    });
    markers.save();
}

fn draw_markers(mut gizmos: Gizmos, markers: Res<Markers>) {
    const RADIUS: f32 = 2.2;
    const SEGMENTS: u32 = 24;
    const POST_HEIGHT: f32 = 5.0;
    for entry in &markers.entries {
        let color = entry.category.color();
        let base = entry.position + Vec3::Y * 0.05;
        let mut previous = base + Vec3::new(RADIUS, 0.0, 0.0);
        for index in 1..=SEGMENTS {
            let angle = std::f32::consts::TAU * index as f32 / SEGMENTS as f32;
            let next = base + Vec3::new(angle.cos() * RADIUS, 0.0, angle.sin() * RADIUS);
            gizmos.line(previous, next, color);
            previous = next;
        }
        let top = base + Vec3::Y * POST_HEIGHT;
        gizmos.line(base, top, color);
        gizmos.line(
            top + Vec3::new(-0.6, 0.0, -0.6),
            top + Vec3::new(0.6, 0.0, 0.6),
            color,
        );
        gizmos.line(
            top + Vec3::new(-0.6, 0.0, 0.6),
            top + Vec3::new(0.6, 0.0, -0.6),
            color,
        );
    }
}

fn update_status_text(status: Res<ViewerStatus>, mut text: Query<&mut Text, With<StatusText>>) {
    if !status.is_changed() {
        return;
    }
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    text.0 = match &status.error {
        Some(error) => format!("{}\nRELOAD REJECTED: {error}", status.text),
        None => status.text.clone(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::mesh::VertexAttributeValues;

    fn ramp(resolution: usize) -> Vec<f32> {
        (0..resolution * resolution)
            .map(|index| (index % resolution) as f32 * 0.5)
            .collect()
    }

    fn winding_sign(mesh: &Mesh) -> f32 {
        // Signed area of the first triangle projected onto XZ. The terrain and
        // the apron must agree, or one of them is entirely backface-culled --
        // built, spawned, and invisible.
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("mesh has no positions");
        };
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("mesh has no indices");
        };
        let (a, b, c) = (
            positions[indices[0] as usize],
            positions[indices[1] as usize],
            positions[indices[2] as usize],
        );
        ((b[0] - a[0]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[0] - a[0])).signum()
    }

    #[test]
    fn source_signature_names_the_missing_file_instead_of_hanging() {
        // Regression for D9: concept_batches/caledonia_v1 is a real batch
        // missing placement_plan.json, render_plan.json, and
        // terrain/heightfield_f32le.bin. Before this test existed, a missing
        // required artifact was never verified to surface as a named error --
        // the reported symptom was the viewer sitting on a blank window
        // forever. source_signature must return a named Err, not silently
        // succeed or hang.
        let batch = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../godot_renderer/concept_batches/caledonia_v1")
            .canonicalize()
            .expect("caledonia_v1 batch must exist for this regression test");
        let config = ViewerConfig {
            batch,
            renderer_root: PathBuf::new(),
            capture_path: None,
            capture_view: "overview".to_owned(),
        };
        let error =
            source_signature(&config).expect_err("an incomplete batch must not produce a stable signature");
        let message = format!("{error:#}");
        assert!(
            message.contains("placement_plan.json"),
            "error must name the missing file by name, got: {message}"
        );
    }

    #[test]
    fn apron_is_wound_the_same_way_as_the_terrain_it_joins() {
        // Regression: the apron grid walks +Z as its row index rises and the
        // terrain grid walks -Z, so copying the terrain's index order mirrors
        // every triangle. An A/B capture caught it as zero differing pixels --
        // the apron was rendering nothing at all.
        let resolution = 9;
        let heights = ramp(resolution);
        let terrain = build_terrain_mesh(&heights, resolution, 8.0, 8.0).unwrap();
        let apron = build_apron_mesh(&heights, resolution, 8.0, 8.0, 16.0, 4.0).unwrap();
        assert_eq!(winding_sign(&terrain), winding_sign(&apron));
    }

    #[test]
    fn apron_leaves_the_certified_interior_to_the_terrain_mesh() {
        let resolution = 9;
        let apron = build_apron_mesh(&ramp(resolution), resolution, 8.0, 8.0, 16.0, 4.0).unwrap();
        let inner = (resolution - 1) / TERRAIN_STRIDE + 1;
        let side = inner + APRON_RINGS * 2;
        let total = (side - 1) * (side - 1);
        let interior = (inner - 1) * (inner - 1);
        assert_eq!(
            (total - interior) * 6,
            apron.indices().map(|indices| indices.len()).unwrap()
        );
    }

    #[test]
    fn apron_meets_the_terrain_exactly_at_the_seam() {
        // A seam that does not close is a thin line of void along the world
        // edge -- a smaller, more embarrassing version of the defect the apron
        // exists to remove.
        let resolution = 9;
        let heights = ramp(resolution);
        let (width, length) = (8.0, 8.0);
        let apron = build_apron_mesh(&heights, resolution, width, length, 16.0, 4.0).unwrap();
        let Some(VertexAttributeValues::Float32x3(positions)) =
            apron.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("apron has no positions");
        };
        let sampler = TerrainSampler {
            heights: &heights,
            resolution,
            width,
            length,
        };
        let mut seam = 0;
        for position in positions {
            // The seam proper: on the edge line *and* within the world in the
            // other axis. A vertex on the x-edge line but far out in z is
            // apron, and is meant to have fallen away.
            if (position[0].abs() - width * 0.5).abs() > 1e-4
                || position[2].abs() > length * 0.5 + 1e-4
            {
                continue;
            }
            seam += 1;
            assert!(
                (position[1] - sampler.sample(position[0], position[2])).abs() < 1e-4,
                "apron vertex at {position:?} does not sit on the terrain"
            );
        }
        assert!(seam > 0, "no seam vertices found");
    }

    #[test]
    fn apron_falls_away_from_the_world_rather_than_rising() {
        let resolution = 9;
        let apron = build_apron_mesh(&ramp(resolution), resolution, 8.0, 8.0, 16.0, 4.0).unwrap();
        let Some(VertexAttributeValues::Float32x3(positions)) =
            apron.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("apron has no positions");
        };
        // The furthest ring must sit a full falloff below the edge it copies,
        // so the horizon reads as land receding and never as invented mountains
        // the certified world does not contain.
        let lowest = positions
            .iter()
            .map(|position| position[1])
            .fold(f32::INFINITY, f32::min);
        let edge_lowest = positions
            .iter()
            .filter(|position| position[0].abs() <= 4.0 + 1e-4 && position[2].abs() <= 4.0 + 1e-4)
            .map(|position| position[1])
            .fold(f32::INFINITY, f32::min);
        assert!((edge_lowest - lowest - 4.0).abs() < 1e-3, "{edge_lowest} {lowest}");
    }

    #[test]
    fn apron_refuses_a_nonsense_extent() {
        assert!(build_apron_mesh(&ramp(9), 9, 8.0, 8.0, 0.0, 4.0).is_err());
    }

    #[test]
    fn decodes_and_meshes_a_bounded_heightfield() {
        let resolution = 5;
        let values: Vec<f32> = (0..resolution * resolution)
            .map(|index| index as f32 * 0.1)
            .collect();
        let bytes: Vec<u8> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let decoded = decode_heightfield(&bytes, resolution).unwrap();
        let mesh = build_terrain_mesh(&decoded, resolution, 4.0, 4.0).unwrap();
        assert_eq!(4, mesh.count_vertices());
    }

    #[test]
    fn rejects_truncated_heightfields() {
        assert!(decode_heightfield(&[0; 12], 2).is_err());
    }
}
