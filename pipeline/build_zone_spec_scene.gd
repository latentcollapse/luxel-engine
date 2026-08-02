@tool
extends SceneTree

## Autonomous Godot scene compiler. It writes a candidate by default; the
## Python orchestrator promotes that candidate only after every gate passes.

const ZoneTerrainBuilderScript = preload("res://zone_terrain_builder.gd")
const AlbionKeepScript = preload("res://AlbionPortalKeep.gd")
const HiberniaKeepScript = preload("res://HiberniaPortalKeep.gd")
const MidgardKeepScript = preload("res://MidgardPortalKeep.gd")
const ZoneFoliageWindScript = preload("res://pipeline/zone_foliage_wind.gd")

const DEFAULT_OUTPUT_SCENE_PATH := "res://.codeweald_candidate_moba_3d.tscn"

var _multimesh_template_cache: Dictionary = {}
var _multimesh_resource_dir := ""
var _multimesh_resource_paths: Dictionary = {}
var _multimesh_resource_errors: Array[String] = []
var _settlement_grounded_component_count := 0
var _settlement_grounding_max_adjustment_m := 0.0
var _settlement_grounding_max_residual_m := 0.0

func _output_scene_path() -> String:
	var arguments := OS.get_cmdline_user_args()
	for index in range(arguments.size() - 1):
		if arguments[index] == "--output":
			var requested := str(arguments[index + 1]).replace("\\", "/")
			if not requested.begins_with("res://"):
				push_error("Invalid --output scene path")
				return ""
			var relative := requested.trim_prefix("res://")
			if relative.is_empty() or relative.begins_with("/") or ".." in relative.split("/") or not relative.ends_with(".tscn"):
				push_error("Invalid --output scene path")
				return ""
			return "res://" + relative
	return DEFAULT_OUTPUT_SCENE_PATH

func _batch_path() -> String:
	var arguments := OS.get_cmdline_user_args()
	for index in range(arguments.size() - 1):
		if arguments[index] == "--batch":
			var requested := str(arguments[index + 1]).replace("\\", "/")
			if requested.begins_with("res://"):
				requested = requested.trim_prefix("res://")
			if requested.is_empty() or requested.begins_with("/") or ".." in requested.split("/"):
				push_error("Invalid --batch path")
				return ""
			return "res://" + requested.trim_suffix("/")
	return "res://concept_batches/caledonia_v1"

func _read_json(path: String) -> Dictionary:
	var file := FileAccess.open(path, FileAccess.READ)
	if file == null:
		push_error("Cannot read ZoneSpec: " + path)
		return {}
	var parser := JSON.new()
	if parser.parse(file.get_as_text()) != OK:
		push_error("Cannot parse ZoneSpec: " + parser.get_error_message())
		return {}
	return parser.data as Dictionary

func _set_owner_recursive(node: Node, scene_root: Node) -> void:
	for child in node.get_children():
		child.owner = scene_root
		_set_owner_recursive(child, scene_root)

func _add_environment(
	scene_root: Node3D,
	style_reference: Dictionary,
	style_calibration: Dictionary
) -> void:
	var metrics: Dictionary = style_reference.get("metrics", {}) as Dictionary
	var adjustment: Dictionary = style_calibration.get("adjustment", {}) as Dictionary
	var mean_rgb: Array = metrics.get("mean_rgb", [0.20, 0.20, 0.16]) as Array
	if mean_rgb.size() != 3:
		mean_rgb = [0.20, 0.20, 0.16]
	var source_color := Color(
		float(mean_rgb[0]), float(mean_rgb[1]), float(mean_rgb[2])
	)
	var source_peak := maxf(maxf(source_color.r, source_color.g), source_color.b)
	var source_tint := source_color / maxf(source_peak, 0.001)
	source_tint.a = 1.0
	# Materials already carry their own hue.  Apply only half of the source
	# white-balance offset to illumination so the grade does not tint the same
	# palette twice (which previously drove blue far below the reference).
	var illumination_tint := Color(
		0.50 + source_tint.r * 0.50,
		0.50 + source_tint.g * 0.50,
		0.50 + source_tint.b * 0.50
	)
	var source_luminance := float(metrics.get("mean_luminance", 0.20))
	var source_contrast := float(metrics.get("luminance_stddev", 0.10))
	var source_saturation := float(metrics.get("mean_saturation", 0.28))
	var world_environment := WorldEnvironment.new()
	world_environment.name = "HighlandAtmosphere"
	var environment := Environment.new()
	environment.background_mode = Environment.BG_COLOR
	environment.background_color = Color(
		source_color.r * 0.60, source_color.g * 0.60, source_color.b * 0.60
	)
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	environment.ambient_light_color = Color(
		illumination_tint.r * 0.58,
		illumination_tint.g * 0.58,
		illumination_tint.b * 0.58
	)
	# This is diffuse Highland daylight, not a miniature under a work lamp.
	# Favor soft ambient fill over a hard key so horizontal terrain does not
	# bleach out in player-height cameras while vertical forms remain legible.
	environment.ambient_light_energy = 1.02
	environment.tonemap_mode = Environment.TONE_MAPPER_FILMIC
	# The concept is also the color-script authority.  These bounded values
	# provide a deterministic first-pass grade while leaving local materials
	# and lighting responsible for structural detail.
	environment.adjustment_enabled = true
	environment.adjustment_brightness = clampf(
		(0.96 + source_luminance * 1.15)
			* float(adjustment.get("brightness_multiplier", 1.0)),
		0.86,
		1.90
	)
	environment.adjustment_contrast = clampf(
		(1.0 + source_contrast * 0.80)
			* float(adjustment.get("contrast_multiplier", 1.0)),
		0.86,
		1.48
	)
	environment.adjustment_saturation = clampf(
		(0.90 + source_saturation * 0.60)
			* float(adjustment.get("saturation_multiplier", 1.0)),
		0.35,
		1.32
	)
	environment.glow_enabled = true
	environment.glow_intensity = 0.55
	environment.fog_enabled = true
	environment.fog_light_color = Color(
		illumination_tint.r * 0.50,
		illumination_tint.g * 0.50,
		illumination_tint.b * 0.50
	)
	environment.fog_light_energy = 0.28
	# This is a 2.4 km overview.  A density suitable for a player-height scene
	# erased the roads and valley after a few hundred metres in the evidence
	# camera, so keep only distant Highland atmospheric perspective here.
	environment.fog_density = 0.00010
	environment.fog_sky_affect = 0.7
	world_environment.environment = environment
	scene_root.add_child(world_environment)
	var sun := DirectionalLight3D.new()
	sun.name = "OvercastSun"
	sun.rotation_degrees = Vector3(-48.0, -32.0, 0.0)
	sun.light_color = Color(
		0.82 + illumination_tint.r * 0.16,
		0.82 + illumination_tint.g * 0.16,
		0.82 + illumination_tint.b * 0.16
	)
	sun.light_energy = 0.68
	sun.shadow_enabled = true
	scene_root.add_child(sun)

func _add_collision(
	scene_root: Node3D,
	terrain_sampler: Dictionary,
	collision_resource_path: String
) -> Dictionary:
	var image := terrain_sampler.get("image") as Image
	var bounds := terrain_sampler.get("bounds", {}) as Dictionary
	var range_data := terrain_sampler.get("range", {}) as Dictionary
	if image == null or image.is_empty():
		return {"sample_count": 0, "external": false}
	var map_width := image.get_width()
	var map_depth := image.get_height()
	var world_width := float(bounds.get("width", 0.0))
	var world_length := float(bounds.get("length", 0.0))
	var min_height := float(range_data.get("min", 0.0))
	var max_height := float(range_data.get("max", 0.0))
	if (
		map_width < 3
		or map_depth < 3
		or world_width <= 0.0
		or world_length <= 0.0
		or max_height <= min_height
	):
		return {"sample_count": 0, "external": false}
	var height_data := PackedFloat32Array()
	height_data.resize(map_width * map_depth)
	for z_index in range(map_depth):
		# Terrain UV +V maps toward world -Z, while HeightMapShape3D rows map
		# toward local +Z. Reverse the image rows so collision and render
		# geometry describe the same world-space surface.
		var image_z := map_depth - 1 - z_index
		for x_index in range(map_width):
			height_data[z_index * map_width + x_index] = lerpf(
				min_height,
				max_height,
				image.get_pixel(x_index, image_z).r
			)
	var shape := HeightMapShape3D.new()
	shape.map_width = map_width
	shape.map_depth = map_depth
	shape.map_data = height_data
	var save_result := ResourceSaver.save(
		shape,
		collision_resource_path,
		ResourceSaver.FLAG_COMPRESS
	)
	if save_result != OK:
		return {"sample_count": 0, "external": false}
	var external_shape := ResourceLoader.load(
		collision_resource_path,
		"",
		ResourceLoader.CACHE_MODE_REPLACE
	) as Shape3D
	if external_shape == null:
		return {"sample_count": 0, "external": false}
	var body := StaticBody3D.new()
	body.name = "TerrainCollision"
	body.scale = Vector3(
		world_width / float(map_width - 1),
		1.0,
		world_length / float(map_depth - 1)
	)
	var collision := CollisionShape3D.new()
	collision.shape = external_shape
	body.add_child(collision)
	scene_root.add_child(body)
	return {
		"sample_count": map_width * map_depth,
		"external": external_shape.resource_path == collision_resource_path,
		"resource_path": collision_resource_path,
		"resource_bytes": FileAccess.get_file_as_bytes(
			collision_resource_path
		).size(),
	}

func _add_navigation_region(
	scene_root: Node3D,
	terrain_sampler: Dictionary,
	zone_spec: Dictionary
) -> Dictionary:
	var bounds := zone_spec.get("zone", {}).get("world_bounds", {}) as Dictionary
	var width := float(bounds.get("width", 0.0))
	var length := float(bounds.get("length", 0.0))
	if width <= 0.0 or length <= 0.0:
		return {"vertex_count": 0, "polygon_count": 0}
	var policy := zone_spec.get("traversal_policy", {}) as Dictionary
	var target_cell_size := 40.0
	var columns := clampi(ceili(width / target_cell_size) + 1, 17, 129)
	var rows := clampi(ceili(length / target_cell_size) + 1, 17, 129)
	var spacing_x := width / float(columns - 1)
	var spacing_z := length / float(rows - 1)
	var vertices := PackedVector3Array()
	for row in range(rows):
		var z := length * 0.5 - float(row) * spacing_z
		for column in range(columns):
			var x := -width * 0.5 + float(column) * spacing_x
			vertices.append(Vector3(
				x,
				ZoneTerrainBuilderScript.sample_height(terrain_sampler, x, z) + 0.12,
				z
			))
	var navigation_mesh := NavigationMesh.new()
	navigation_mesh.agent_radius = float(policy.get("agent_radius_m", 2.5))
	navigation_mesh.agent_height = float(policy.get("agent_height_m", 8.0))
	navigation_mesh.agent_max_climb = float(policy.get("agent_max_climb_m", 4.0))
	navigation_mesh.agent_max_slope = float(
		policy.get("agent_max_slope_degrees", 45.0)
	)
	navigation_mesh.cell_size = minf(spacing_x, spacing_z)
	navigation_mesh.cell_height = maxf(
		0.25, float(policy.get("agent_max_climb_m", 4.0)) * 0.25
	)
	navigation_mesh.vertices = vertices
	var maximum_grade := tan(deg_to_rad(navigation_mesh.agent_max_slope))
	var polygon_count := 0
	for row in range(rows - 1):
		for column in range(columns - 1):
			var top_left := row * columns + column
			var top_right := top_left + 1
			var bottom_left := (row + 1) * columns + column
			var bottom_right := bottom_left + 1
			var grade_x_top := absf(
				vertices[top_right].y - vertices[top_left].y
			) / spacing_x
			var grade_x_bottom := absf(
				vertices[bottom_right].y - vertices[bottom_left].y
			) / spacing_x
			var grade_z_left := absf(
				vertices[bottom_left].y - vertices[top_left].y
			) / spacing_z
			var grade_z_right := absf(
				vertices[bottom_right].y - vertices[top_right].y
			) / spacing_z
			if maxf(
				maxf(grade_x_top, grade_x_bottom),
				maxf(grade_z_left, grade_z_right)
			) > maximum_grade:
				continue
			navigation_mesh.add_polygon(PackedInt32Array([
				top_left,
				top_right,
				bottom_right,
				bottom_left,
			]))
			polygon_count += 1
	var region := NavigationRegion3D.new()
	region.name = "ZoneNavigationRegion"
	region.navigation_mesh = navigation_mesh
	region.use_edge_connections = true
	region.set_meta("codeweald_navigation_contract", true)
	region.set_meta("codeweald_navigation_vertices", vertices.size())
	region.set_meta("codeweald_navigation_polygons", polygon_count)
	scene_root.add_child(region)
	return {
		"vertex_count": vertices.size(),
		"polygon_count": polygon_count,
		"columns": columns,
		"rows": rows,
		"cell_size_m": minf(spacing_x, spacing_z),
	}

func _banner_material(effect: Dictionary) -> ShaderMaterial:
	var parameters: Dictionary = effect.get("parameters", {}) as Dictionary
	var shader := Shader.new()
	shader.code = """
		shader_type spatial;
		render_mode cull_disabled, diffuse_burley;
		uniform vec3 banner_color = vec3(0.2, 0.3, 0.8);
		uniform float wave_period = 3.6;
		uniform float wave_amplitude = 0.72;
		void vertex() {
			float phase = TIME * 6.2831853 / max(wave_period, 0.1) + VERTEX.y * 0.72;
			VERTEX.z += sin(phase) * wave_amplitude * (VERTEX.x * 0.055 + 0.50);
		}
		void fragment() {
			ALBEDO = banner_color;
			ROUGHNESS = 0.72;
		}
	"""
	var material := ShaderMaterial.new()
	material.shader = shader
	var color_data: Array = parameters.get("color_srgb", [0.45, 0.45, 0.45]) as Array
	if color_data.size() == 3:
		material.set_shader_parameter("banner_color", Vector3(float(color_data[0]), float(color_data[1]), float(color_data[2])))
	material.set_shader_parameter("wave_period", float(parameters.get("period_s", 3.6)))
	material.set_shader_parameter("wave_amplitude", float(parameters.get("amplitude_m", 0.72)))
	return material

func _add_keep_banner(keep: Node3D, effect: Dictionary) -> void:
	if str(effect.get("kind", "")) != "banner_wave":
		return
	var parameters := effect.get("parameters", {}) as Dictionary
	var height := float(parameters.get("banner_height_m", 110.0))
	var size_data: Array = parameters.get("banner_size_m", [14.0, 9.0]) as Array
	var banner_size := Vector2(14.0, 9.0)
	if size_data.size() == 2:
		banner_size = Vector2(float(size_data[0]), float(size_data[1]))
	for side in [-1.0, 1.0]:
		var banner := MeshInstance3D.new()
		banner.name = "RealmBanner" + str(side)
		var mesh := QuadMesh.new()
		mesh.size = banner_size
		banner.mesh = mesh
		banner.position = Vector3(side * banner_size.x * 0.61, height, 0.0)
		banner.material_override = _banner_material(effect)
		keep.add_child(banner)

func _keep_material(color: Color, roughness: float, metallic: float = 0.0) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = roughness
	material.metallic = metallic
	return material

func _apply_keep_palette_recursive(node: Node) -> void:
	if node is MeshInstance3D:
		var lowered := str(node.name).to_lower()
		var material: StandardMaterial3D
		if "roof" in lowered or "slate" in lowered or "spire" in lowered:
			material = _keep_material(Color(0.035, 0.055, 0.070), 0.82)
		elif "timber" in lowered or "drawbridge" in lowered:
			material = _keep_material(Color(0.105, 0.052, 0.022), 0.90)
		elif "iron" in lowered or "gate_shadow" in lowered:
			material = _keep_material(Color(0.025, 0.032, 0.034), 0.52, 0.42)
		elif "shadow" in lowered or "plinth" in lowered or "merlon" in lowered:
			material = _keep_material(Color(0.080, 0.095, 0.090), 0.97)
		else:
			material = _keep_material(Color(0.145, 0.160, 0.145), 0.94)
		(node as MeshInstance3D).material_override = material
	for child in node.get_children():
		_apply_keep_palette_recursive(child)

func _add_keep(scene_root: Node3D, feature: Dictionary, effect: Dictionary, assignment: Dictionary, terrain_sampler: Dictionary, missing_assets: Array) -> bool:
	var properties: Dictionary = feature.get("properties", {})
	var points: Array = feature.get("geometry", {}).get("points", [])
	if points.is_empty():
		return false
	var point: Array = points[0]
	var realm: String = str(properties.get("realm", "")).to_lower()
	var keep: Node3D
	var assets: Array = assignment.get("assets", []) as Array
	var scales: Array = assignment.get("scale_m", [1.0, 1.0]) as Array
	var facing: Array = properties.get("facing_direction_xz", []) as Array
	if not assets.is_empty() and scales.size() == 2:
		# Fortifications travel through the same selected-asset contract as every
		# other landmark. The prior CSG-only realm keeps remain a bounded fallback
		# for old batches that do not request a verified fortification kit.
		keep = Node3D.new()
		keep.name = str(feature.get("id", "faction_keep"))
		keep.set_meta("codeweald_profile_id", assignment.get("profile_id", ""))
		keep.position = Vector3(float(point[0]), ZoneTerrainBuilderScript.sample_height(terrain_sampler, float(point[0]), float(point[1])) + 1.0, float(point[1]))
		if facing.size() == 2:
			keep.rotation.y = atan2(float(facing[0]), float(facing[1]))
		scene_root.add_child(keep)
		var asset_index := 0
		if realm == "hibernia":
			asset_index = mini(1, assets.size() - 1)
		elif realm == "midgard":
			asset_index = mini(2, assets.size() - 1)
		if not _instantiate_scene(str(assets[asset_index]), keep, Vector3.ZERO, float(scales[0]), 0.0, missing_assets):
			scene_root.remove_child(keep)
			keep.queue_free()
			return false
		_apply_keep_palette_recursive(keep)
	else:
		match realm:
			"albion": keep = AlbionKeepScript.new()
			"hibernia": keep = HiberniaKeepScript.new()
			"midgard": keep = MidgardKeepScript.new()
			_:
				push_warning("Unsupported faction realm for keep " + str(feature.get("id", "unknown")) + ": " + realm)
				return false
		keep.name = str(feature.get("id", "faction_keep"))
		keep.position = Vector3(float(point[0]), ZoneTerrainBuilderScript.sample_height(terrain_sampler, float(point[0]), float(point[1])) + 1.0, float(point[1]))
		if facing.size() == 2:
			keep.rotation.y = atan2(float(facing[0]), float(facing[1]))
		scene_root.add_child(keep)
	keep.set_meta("codeweald_feature", feature.get("id", ""))
	keep.set_meta("codeweald_realm", realm)
	_add_keep_banner(keep, effect)
	return true

func _lane_surface_material(terrain_manifest_path: String) -> ShaderMaterial:
	var manifest := _read_json(terrain_manifest_path)
	var palette: Dictionary = manifest.get("style_palette_srgb", {}) as Dictionary
	var road_channels: Array = palette.get("road", [0.47, 0.34, 0.19]) as Array
	var grass_channels: Array = palette.get("grass", [0.33, 0.43, 0.20]) as Array
	var road_color_srgb := Vector3(float(road_channels[0]), float(road_channels[1]), float(road_channels[2]))
	var grass_color_srgb := Vector3(float(grass_channels[0]), float(grass_channels[1]), float(grass_channels[2]))
	var road_luminance := road_color_srgb.dot(Vector3(0.2126, 0.7152, 0.0722))
	var grass_luminance := grass_color_srgb.dot(Vector3(0.2126, 0.7152, 0.0722))
	# The source palette is authoritative, but packed-dirt still needs a minimum
	# value separation from its surroundings at overview distance. Preserve hue
	# and raise value only when concept quantization compressed the two tokens.
	var required_luminance := grass_luminance + 0.075
	if road_luminance < required_luminance and road_luminance > 0.001:
		road_color_srgb *= minf(required_luminance / road_luminance, 1.65)
	# The compatibility renderer and the existing style-calibration loop treat
	# these art-direction uniforms in display space. Full sRGB conversion made
	# the paths nearly black; attenuating the reviewed colour keeps the dirt
	# readable without returning to the old overexposed cream ribbon. The PBR
	# luminance detail below already supplies bounded local variation.
	var road_color := road_color_srgb
	var shader := Shader.new()
	shader.code = """
		shader_type spatial;
		render_mode cull_disabled, diffuse_burley, fog_disabled;
		uniform vec3 road_color = vec3(0.47, 0.34, 0.19);
		uniform sampler2D road_albedo : source_color, repeat_enable, filter_linear_mipmap;
		uniform sampler2D road_normal : hint_normal, repeat_enable, filter_linear_mipmap;
		uniform sampler2D road_roughness : repeat_enable, filter_linear_mipmap;
		uniform bool has_road_pbr = false;
		varying vec3 codeweald_world_position;
		void vertex() {
			codeweald_world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
		}
		void fragment() {
			float broad = sin(codeweald_world_position.x * 0.071) * sin(codeweald_world_position.z * 0.063);
			float grit = sin(codeweald_world_position.x * 0.83 + codeweald_world_position.z * 0.47);
			vec2 material_uv = codeweald_world_position.xz * 0.085;
			float albedo_detail = has_road_pbr ? dot(texture(road_albedo, material_uv).rgb, vec3(0.2126, 0.7152, 0.0722)) : 0.52;
			ALBEDO = road_color * (0.92 + albedo_detail * 0.42 + broad * 0.055 + grit * 0.025);
			if (has_road_pbr) {
				NORMAL_MAP = texture(road_normal, material_uv).rgb;
				NORMAL_MAP_DEPTH = 0.32;
				ROUGHNESS = clamp(texture(road_roughness, material_uv).r, 0.72, 0.98);
			} else {
				ROUGHNESS = 0.94;
			}
			// Roads are world geometry, not luminous UI ribbons. Ambient and
			// directional lighting provide enough separation from the grass.
			EMISSION = vec3(0.0);
		}
	"""
	var material := ShaderMaterial.new()
	material.shader = shader
	material.set_shader_parameter("road_color", road_color)
	var road_maps: Dictionary = ((manifest.get("terrain_materials", {}) as Dictionary).get("road", {}) as Dictionary).get("maps", {}) as Dictionary
	var road_albedo_path := str((road_maps.get("albedo", {}) as Dictionary).get("path", ""))
	var road_normal_path := str((road_maps.get("normal", {}) as Dictionary).get("path", ""))
	var road_roughness_path := str((road_maps.get("roughness", {}) as Dictionary).get("path", ""))
	var road_albedo := load("res://" + road_albedo_path) as Texture2D if not road_albedo_path.is_empty() else null
	var road_normal := load("res://" + road_normal_path) as Texture2D if not road_normal_path.is_empty() else null
	var road_roughness := load("res://" + road_roughness_path) as Texture2D if not road_roughness_path.is_empty() else null
	if road_albedo != null and road_normal != null and road_roughness != null:
		material.set_shader_parameter("road_albedo", road_albedo)
		material.set_shader_parameter("road_normal", road_normal)
		material.set_shader_parameter("road_roughness", road_roughness)
		material.set_shader_parameter("has_road_pbr", true)
	return material

func _lane_centerline(
	curve: Curve3D,
	terrain_sampler: Dictionary,
	spacing_m: float = 4.0,
	height_offset_m: float = 0.72
) -> Array[Vector3]:
	var samples: Array[Vector3] = []
	var length := curve.get_baked_length()
	var steps := maxi(1, ceili(length / spacing_m))
	for step in range(steps + 1):
		var sampled := curve.sample_baked(length * float(step) / float(steps), true)
		samples.append(Vector3(
			sampled.x,
			ZoneTerrainBuilderScript.sample_height(terrain_sampler, sampled.x, sampled.z) + height_offset_m,
			sampled.z
		))
	return samples

func _add_lane_surface(path: Path3D, centerline: Array[Vector3], half_width: float, terrain_sampler: Dictionary, material: Material) -> bool:
	if centerline.size() < 2:
		return false
	var left: Array[Vector3] = []
	var right: Array[Vector3] = []
	for index in range(centerline.size()):
		var before := centerline[maxi(0, index - 1)]
		var after := centerline[mini(centerline.size() - 1, index + 1)]
		var tangent := Vector2(after.x - before.x, after.z - before.z).normalized()
		var side := Vector2(-tangent.y, tangent.x)
		var left_2d := Vector2(centerline[index].x, centerline[index].z) + side * half_width
		var right_2d := Vector2(centerline[index].x, centerline[index].z) - side * half_width
		left.append(Vector3(left_2d.x, ZoneTerrainBuilderScript.sample_height(terrain_sampler, left_2d.x, left_2d.y) + 0.76, left_2d.y))
		right.append(Vector3(right_2d.x, ZoneTerrainBuilderScript.sample_height(terrain_sampler, right_2d.x, right_2d.y) + 0.76, right_2d.y))
	var st := SurfaceTool.new()
	st.begin(Mesh.PRIMITIVE_TRIANGLES)
	var travelled := 0.0
	for index in range(centerline.size() - 1):
		if index > 0:
			travelled += centerline[index - 1].distance_to(centerline[index])
		var next_travelled := travelled + centerline[index].distance_to(centerline[index + 1])
		var v0 := travelled / 8.0
		var v1 := next_travelled / 8.0
		st.set_uv(Vector2(0.0, v0)); st.add_vertex(left[index])
		st.set_uv(Vector2(1.0, v0)); st.add_vertex(right[index])
		st.set_uv(Vector2(0.0, v1)); st.add_vertex(left[index + 1])
		st.set_uv(Vector2(1.0, v0)); st.add_vertex(right[index])
		st.set_uv(Vector2(1.0, v1)); st.add_vertex(right[index + 1])
		st.set_uv(Vector2(0.0, v1)); st.add_vertex(left[index + 1])
	st.generate_normals()
	var surface := MeshInstance3D.new()
	surface.name = "LaneSurface"
	surface.mesh = st.commit()
	surface.material_override = material
	surface.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	surface.set_meta("codeweald_lane_surface", true)
	path.add_child(surface)
	return surface.mesh != null

func _add_lane_path(scene_root: Node3D, feature: Dictionary, terrain_sampler: Dictionary, lane_material: Material) -> bool:
	var points: Array = feature.get("geometry", {}).get("points", [])
	if points.size() < 2:
		return false
	var lane_id := str(feature.get("properties", {}).get("lane_id", feature.get("id", "lane")))
	var path := Path3D.new()
	path.name = "LaneNavigation_" + lane_id
	var curve := Curve3D.new()
	for point_var in points:
		var point: Array = point_var as Array
		var x := float(point[0])
		var z := float(point[1])
		curve.add_point(Vector3(x, ZoneTerrainBuilderScript.sample_height(terrain_sampler, x, z) + 0.65, z))
	# Curved handles make this an actual B-spline-style movement route rather
	# than a render-only polyline. Consumers can sample it for minions, cameras,
	# patrols, and later native navigation adapters.
	for index in range(curve.point_count):
		var before := curve.get_point_position(maxi(0, index - 1))
		var after := curve.get_point_position(mini(curve.point_count - 1, index + 1))
		var tangent := (after - before) * 0.22
		curve.set_point_in(index, -tangent)
		curve.set_point_out(index, tangent)
	path.curve = curve
	path.set_meta("codeweald_feature", feature.get("id", ""))
	path.set_meta("codeweald_lane_id", lane_id)
	scene_root.add_child(path)
	var properties: Dictionary = feature.get("properties", {}) as Dictionary
	var half_width := float(
		properties.get(
			"visual_width_m",
			properties.get("minimum_width_m", 20.0)
		)
	) * 0.5
	return _add_lane_surface(path, _lane_centerline(curve, terrain_sampler), half_width, terrain_sampler, lane_material)

func _effect_by_feature(runtime_effects: Dictionary, feature_id: String) -> Dictionary:
	for effect_var in runtime_effects.get("effects", []):
		var effect := effect_var as Dictionary
		if str(effect.get("feature_id", "")) == feature_id:
			return effect
	return {}

func _water_material(effect: Dictionary) -> ShaderMaterial:
	var parameters: Dictionary = effect.get("parameters", {}) as Dictionary
	var shader := Shader.new()
	shader.code = """
		shader_type spatial;
		render_mode blend_mix, cull_disabled, diffuse_burley;
		uniform float flow_speed = 0.65;
		uniform float wave_amplitude = 0.10;
		uniform float wave_frequency = 0.8;
		void fragment() {
			float wave = sin((UV.y + TIME * flow_speed) * 24.0 * wave_frequency) * wave_amplitude;
			// Highland water is a dark teal compositional seam, not a luminous
			// cyan road. Retain enough value separation to read the downhill
			// network while matching the concept's wetland palette.
			ALBEDO = vec3(0.040, 0.130, 0.155) + vec3(0.012, 0.030, 0.038) * wave;
			EMISSION = vec3(0.002, 0.010, 0.014) * (0.65 + wave);
			METALLIC = 0.04;
			ROUGHNESS = 0.34;
			ALPHA = 0.92;
		}
	"""
	var material := ShaderMaterial.new()
	material.shader = shader
	material.set_shader_parameter("flow_speed", float(parameters.get("flow_speed_mps", 0.65)))
	material.set_shader_parameter("wave_amplitude", float(parameters.get("wave_amplitude_m", 0.10)))
	material.set_shader_parameter("wave_frequency", float(parameters.get("wave_frequency_hz", 0.8)))
	return material

func _add_stream(scene_root: Node3D, feature: Dictionary, effect: Dictionary, terrain_sampler: Dictionary) -> bool:
	var points: Array = feature.get("geometry", {}).get("points", [])
	if points.size() < 2:
		return false
	var width := float(feature.get("properties", {}).get("width_m", 12.0))
	var curve := Curve3D.new()
	for point_var in points:
		var point: Array = point_var as Array
		var x := float(point[0])
		var z := float(point[1])
		curve.add_point(Vector3(
			x,
			ZoneTerrainBuilderScript.sample_height(terrain_sampler, x, z) + 0.55,
			z
		))
	for index in range(curve.point_count):
		var before := curve.get_point_position(maxi(0, index - 1))
		var after := curve.get_point_position(mini(curve.point_count - 1, index + 1))
		var tangent := (after - before) * 0.18
		curve.set_point_in(index, -tangent)
		curve.set_point_out(index, tangent)
	var centerline := _lane_centerline(curve, terrain_sampler, 2.0, 0.55)
	var left: Array[Vector3] = []
	var right: Array[Vector3] = []
	for index in range(centerline.size()):
		var before := centerline[maxi(0, index - 1)]
		var after := centerline[mini(centerline.size() - 1, index + 1)]
		var direction := Vector2(after.x - before.x, after.z - before.z).normalized()
		var side := Vector2(-direction.y, direction.x) * width * 0.5
		left.append(Vector3(
			centerline[index].x + side.x,
			centerline[index].y,
			centerline[index].z + side.y
		))
		right.append(Vector3(
			centerline[index].x - side.x,
			centerline[index].y,
			centerline[index].z - side.y
		))
	var st := SurfaceTool.new()
	st.begin(Mesh.PRIMITIVE_TRIANGLES)
	var travelled := 0.0
	for index in range(centerline.size() - 1):
		if index > 0:
			travelled += centerline[index - 1].distance_to(centerline[index])
		var next_travelled := travelled + centerline[index].distance_to(centerline[index + 1])
		var v0 := travelled / 6.0
		var v1 := next_travelled / 6.0
		st.set_uv(Vector2(0.0, v0)); st.add_vertex(left[index])
		st.set_uv(Vector2(1.0, v0)); st.add_vertex(right[index])
		st.set_uv(Vector2(0.0, v1)); st.add_vertex(left[index + 1])
		st.set_uv(Vector2(1.0, v0)); st.add_vertex(right[index])
		st.set_uv(Vector2(1.0, v1)); st.add_vertex(right[index + 1])
		st.set_uv(Vector2(0.0, v1)); st.add_vertex(left[index + 1])
	st.generate_normals()
	var water := MeshInstance3D.new()
	water.name = "Waterway_" + str(feature.get("id", "stream"))
	water.mesh = st.commit()
	water.material_override = _water_material(effect)
	water.set_meta("codeweald_feature", feature.get("id", ""))
	scene_root.add_child(water)
	return true

func _add_arcane_ruin(scene_root: Node3D, feature: Dictionary, assignment: Dictionary, effect: Dictionary, terrain_sampler: Dictionary, missing_assets: Array) -> bool:
	var points: Array = feature.get("geometry", {}).get("points", [])
	if points.is_empty():
		return false
	var point: Array = points[0] as Array
	var x := float(point[0])
	var z := float(point[1])
	var assets: Array = assignment.get("assets", []) as Array
	var scales: Array = assignment.get("scale_m", [1.0, 1.0]) as Array
	if assets.is_empty() or scales.size() != 2:
		push_warning("No resolved Godot ruin asset for " + str(feature.get("id", "center")))
		return false
	var ruin := Node3D.new()
	ruin.name = "ArcaneRuin_" + str(feature.get("id", "center"))
	ruin.set_meta("codeweald_feature", feature.get("id", ""))
	ruin.set_meta("codeweald_profile_id", assignment.get("profile_id", ""))
	ruin.set_meta("codeweald_anchor_world", Vector3(x, ZoneTerrainBuilderScript.sample_height(terrain_sampler, x, z), z))
	scene_root.add_child(ruin)
	var asset_index := absi(str(feature.get("id", "center")).hash()) % assets.size()
	var placed := _instantiate_scene(str(assets[asset_index]), ruin, Vector3(x, ZoneTerrainBuilderScript.sample_height(terrain_sampler, x, z) + 0.05, z), float(scales[0]), 0.0, missing_assets)
	if placed:
		_apply_ruin_palette_recursive(ruin)
		var parameters: Dictionary = effect.get("parameters", {}) as Dictionary
		var glow := OmniLight3D.new()
		glow.name = "ArcaneRuinGlow"
		glow.position = Vector3(x, ZoneTerrainBuilderScript.sample_height(terrain_sampler, x, z) + 6.0, z)
		glow.light_color = Color(0.34, 0.11, 0.62)
		glow.light_energy = float(parameters.get("light_energy_min", 0.45))
		glow.omni_range = 34.0
		ruin.add_child(glow)
		var player := AnimationPlayer.new()
		player.name = "ObjectivePulse"
		var animation := Animation.new()
		animation.length = float(parameters.get("period_s", 2.4))
		animation.loop_mode = Animation.LOOP_LINEAR
		var track := animation.add_track(Animation.TYPE_VALUE)
		animation.track_set_path(track, NodePath("ArcaneRuinGlow:light_energy"))
		animation.track_insert_key(track, 0.0, float(parameters.get("light_energy_min", 0.45)))
		animation.track_insert_key(track, animation.length * 0.5, float(parameters.get("light_energy_max", 1.6)))
		animation.track_insert_key(track, animation.length, float(parameters.get("light_energy_min", 0.45)))
		var library := AnimationLibrary.new()
		library.add_animation("pulse", animation)
		player.add_animation_library("", library)
		# Autoplay must be assigned before this child enters the saved scene;
		# calling play() in the compiler process does not make a runtime scene
		# animate after it is reopened.
		player.autoplay = "pulse"
		ruin.add_child(player)
	return placed

func _apply_ruin_palette_recursive(node: Node) -> void:
	if node is MeshInstance3D:
		var lowered := str(node.name).to_lower()
		var material := StandardMaterial3D.new()
		if "crystal" in lowered or "portal" in lowered:
			material.albedo_color = Color(0.095, 0.012, 0.20)
			material.emission_enabled = true
			material.emission = Color(0.22, 0.025, 0.42)
			material.emission_energy_multiplier = 1.35
			material.roughness = 0.34
		elif "foundation" in lowered or "moss" in lowered:
			material.albedo_color = Color(0.042, 0.068, 0.034)
			material.roughness = 0.97
		else:
			material.albedo_color = Color(0.070, 0.082, 0.090)
			material.roughness = 0.92
		(node as MeshInstance3D).material_override = material
	for child in node.get_children():
		_apply_ruin_palette_recursive(child)

func _apply_material_recursive(node: Node, material: Material) -> void:
	if node is MeshInstance3D:
		(node as MeshInstance3D).material_override = material
	for child in node.get_children():
		_apply_material_recursive(child, material)

func _add_bridge(
	scene_root: Node3D,
	feature: Dictionary,
	assignment: Dictionary,
	terrain_sampler: Dictionary,
	missing_assets: Array
) -> bool:
	var points: Array = feature.get("geometry", {}).get("points", [])
	var assets: Array = assignment.get("assets", []) as Array
	var scales: Array = assignment.get("scale_m", [1.0, 1.0]) as Array
	if points.is_empty() or assets.is_empty() or scales.size() != 2:
		push_warning("No resolved Godot bridge asset for " + str(feature.get("id", "bridge")))
		return false
	var point: Array = points[0] as Array
	var x := float(point[0])
	var z := float(point[1])
	var properties: Dictionary = feature.get("properties", {}) as Dictionary
	var bridge := Node3D.new()
	bridge.name = "Bridge_" + str(feature.get("id", "crossing"))
	bridge.position = Vector3(
		x,
		ZoneTerrainBuilderScript.sample_height(terrain_sampler, x, z) + 0.05,
		z
	)
	bridge.rotation.y = deg_to_rad(float(properties.get("rotation_degrees", 0.0)))
	bridge.set_meta("codeweald_feature", feature.get("id", ""))
	bridge.set_meta("codeweald_derived_from", properties.get("derived_from", []))
	bridge.set_meta("codeweald_crossing_structure", true)
	bridge.set_meta("codeweald_profile_id", assignment.get("profile_id", ""))
	scene_root.add_child(bridge)
	var asset_index := absi(str(feature.get("id", "bridge")).hash()) % assets.size()
	if not _instantiate_scene(
		str(assets[asset_index]),
		bridge,
		Vector3.ZERO,
		float(scales[0]),
		0.0,
		missing_assets
	):
		scene_root.remove_child(bridge)
		bridge.queue_free()
		return false
	var body := StaticBody3D.new()
	body.name = "BridgeCollision"
	var shape_node := CollisionShape3D.new()
	var shape := BoxShape3D.new()
	shape.size = Vector3(
		float(properties.get("lane_width_m", 32.0)),
		1.5,
		maxf(28.0, float(properties.get("stream_width_m", 12.0)) + 10.0)
	)
	shape_node.shape = shape
	shape_node.position.y = 0.75
	body.add_child(shape_node)
	bridge.add_child(body)
	var link := NavigationLink3D.new()
	link.name = "BridgeNavigationLink"
	var span := maxf(
		28.0,
		float(properties.get("stream_width_m", 12.0)) + 10.0
	)
	link.start_position = Vector3(0.0, 1.7, -span * 0.5)
	link.end_position = Vector3(0.0, 1.7, span * 0.5)
	link.bidirectional = true
	link.enter_cost = 0.25
	link.travel_cost = 1.0
	link.set_meta("codeweald_off_mesh_link", true)
	link.set_meta("codeweald_feature", feature.get("id", ""))
	bridge.add_child(link)
	return true

func _add_settlement(
	scene_root: Node3D,
	feature: Dictionary,
	assignment: Dictionary,
	terrain_sampler: Dictionary,
	missing_assets: Array
) -> bool:
	var points: Array = feature.get("geometry", {}).get("points", [])
	var assets: Array = assignment.get("assets", []) as Array
	var scales: Array = assignment.get("scale_m", [1.0, 1.0]) as Array
	if points.is_empty() or assets.is_empty() or scales.size() != 2:
		push_warning("No resolved Godot settlement asset for " + str(feature.get("id", "settlement")))
		return false
	var point: Array = points[0] as Array
	var x := float(point[0])
	var z := float(point[1])
	var feature_hash := absi(str(feature.get("id", "settlement")).hash())
	var asset_path := str(assets[feature_hash % assets.size()])
	var scale_value := lerpf(
		float(scales[0]),
		float(scales[1]),
		float(feature_hash % 997) / 996.0
	)
	var settlement := Node3D.new()
	settlement.name = "Settlement_" + str(feature.get("id", "settlement"))
	var anchor := Vector3(
		x,
		ZoneTerrainBuilderScript.sample_height(terrain_sampler, x, z),
		z
	)
	settlement.position = anchor
	settlement.set_meta("codeweald_feature", feature.get("id", ""))
	settlement.set_meta("codeweald_anchor_world", anchor)
	settlement.set_meta("codeweald_profile_id", assignment.get("profile_id", ""))
	scene_root.add_child(settlement)
	var placed := _instantiate_scene(
		asset_path,
		settlement,
		Vector3(0.0, 0.05, 0.0),
		scale_value,
		float(feature_hash % 6283) / 1000.0,
		missing_assets
	)
	if placed:
		_apply_settlement_palette_recursive(settlement)
		_ground_settlement_components(settlement, terrain_sampler)
	return placed

func _settlement_component_key(node_name: String) -> String:
	if node_name.begins_with("House_"):
		var parts := node_name.split("_")
		if parts.size() >= 2:
			return "%s_%s" % [parts[0], parts[1]]
	if node_name.begins_with("Watchtower_"):
		return "Watchtower"
	if node_name == "Village_well":
		return node_name
	return ""

func _collect_settlement_components(node: Node, groups: Dictionary) -> void:
	if node is MeshInstance3D:
		var key := _settlement_component_key(str(node.name))
		if not key.is_empty():
			if not groups.has(key):
				groups[key] = []
			(groups[key] as Array).append(node)
	for child in node.get_children():
		_collect_settlement_components(child, groups)

func _settlement_world_transform(
	node: Node3D, settlement: Node3D
) -> Transform3D:
	return settlement.transform * _relative_transform(node, settlement)

func _mesh_world_min_y(
	node: MeshInstance3D, settlement: Node3D
) -> float:
	var bounds := node.mesh.get_aabb()
	var minimum_y := INF
	var world_transform := _settlement_world_transform(node, settlement)
	for x_side in range(2):
		for y_side in range(2):
			for z_side in range(2):
				var corner := bounds.position + Vector3(
					bounds.size.x * x_side,
					bounds.size.y * y_side,
					bounds.size.z * z_side
				)
				minimum_y = minf(
					minimum_y,
					(world_transform * corner).y
				)
	return minimum_y

func _ground_settlement_components(
	settlement: Node3D,
	terrain_sampler: Dictionary
) -> void:
	# Settlement GLBs are modular kits exported as sibling meshes. Ground each
	# house/well/tower as one rigid component instead of forcing a circular
	# terrain terrace beneath the whole village. This preserves nearby streams
	# and lets the same cluster conform to different terrain.
	var groups := {}
	_collect_settlement_components(settlement, groups)
	for key_var in groups:
		var key := str(key_var)
		var meshes: Array = groups[key] as Array
		var reference: MeshInstance3D = null
		for mesh_var in meshes:
			var mesh := mesh_var as MeshInstance3D
			var lowered := str(mesh.name).to_lower()
			if (
				("house_" in lowered and "_foundation" in lowered)
				or lowered == "watchtower_stone"
				or lowered == "village_well"
			):
				reference = mesh
				break
		if reference == null:
			continue
		var sample_position := _settlement_world_transform(
			reference, settlement
		).origin
		var terrain_y := ZoneTerrainBuilderScript.sample_height(
			terrain_sampler, sample_position.x, sample_position.z
		)
		var adjustment_m := (
			terrain_y - _mesh_world_min_y(reference, settlement) + 0.03
		)
		for mesh_var in meshes:
			var mesh := mesh_var as MeshInstance3D
			var parent_3d := mesh.get_parent() as Node3D
			var parent_y_scale := (
				_relative_transform(parent_3d, settlement).basis.y.length()
				if parent_3d != null
				else 1.0
			)
			mesh.position.y += adjustment_m / maxf(parent_y_scale, 0.0001)
		var residual_m := absf(
			_mesh_world_min_y(reference, settlement)
			- ZoneTerrainBuilderScript.sample_height(
				terrain_sampler,
				_settlement_world_transform(reference, settlement).origin.x,
				_settlement_world_transform(reference, settlement).origin.z
			)
			- 0.03
		)
		_settlement_grounded_component_count += 1
		_settlement_grounding_max_adjustment_m = maxf(
			_settlement_grounding_max_adjustment_m, absf(adjustment_m)
		)
		_settlement_grounding_max_residual_m = maxf(
			_settlement_grounding_max_residual_m, residual_m
		)

func _apply_settlement_palette_recursive(node: Node) -> void:
	if node is MeshInstance3D:
		var lowered := str(node.name).to_lower()
		if "village_terrace" in lowered or lowered == "cube":
			# Terrain compilation now creates a physically flat, eased village
			# pad. The prefab's old circular emergency plinth advertised every
			# settlement as a toy diorama; Cube is a stale Blender default left
			# in the source kit. Neither belongs in the runtime composition.
			(node as MeshInstance3D).visible = false
			return
		var material: StandardMaterial3D
		if "window" in lowered:
			material = _keep_material(Color(0.22, 0.075, 0.012), 0.58)
			material.emission_enabled = true
			material.emission = Color(0.16, 0.045, 0.006)
			material.emission_energy_multiplier = 0.55
		elif "roof" in lowered or "slate" in lowered:
			material = _keep_material(Color(0.022, 0.032, 0.036), 0.84)
		elif (
			"door" in lowered
			or "timber" in lowered
			or "foundation" in lowered
			or "chimney" in lowered
		):
			material = _keep_material(Color(0.042, 0.028, 0.016), 0.94)
		elif "terrace" in lowered or "earth" in lowered:
			material = _keep_material(Color(0.070, 0.055, 0.030), 0.98)
		elif "plaster" in lowered:
			material = _keep_material(Color(0.155, 0.115, 0.055), 0.93)
		else:
			material = _keep_material(Color(0.085, 0.095, 0.078), 0.96)
		(node as MeshInstance3D).material_override = material
	for child in node.get_children():
		_apply_settlement_palette_recursive(child)

func _instantiate_scene(path: String, parent: Node3D, position: Vector3, scale_value: float, rotation_y: float, missing_assets: Array, wind_effect: Dictionary = {}, wind_phase: float = 0.0, material_override: Material = null) -> bool:
	if not ResourceLoader.exists(path, "PackedScene"):
		push_warning("Selected asset is not import-ready: " + path)
		if not missing_assets.has(path):
			missing_assets.append(path)
		return false
	var packed := load(path) as PackedScene
	if packed == null:
		push_warning("Missing selected asset: " + path)
		if not missing_assets.has(path):
			missing_assets.append(path)
		return false
	var instance := packed.instantiate() as Node3D
	if instance == null:
		push_warning("Selected asset is not a Node3D: " + path)
		if not missing_assets.has(path):
			missing_assets.append(path)
		return false
	instance.position = position
	instance.set_meta("codeweald_source_asset", path)
	instance.rotation.y = rotation_y
	instance.scale = Vector3.ONE * scale_value
	if material_override != null:
		_apply_material_recursive(instance, material_override)
	if not wind_effect.is_empty():
		var parameters: Dictionary = wind_effect.get("parameters", {}) as Dictionary
		instance.set_script(ZoneFoliageWindScript)
		instance.set("sway_degrees", float(parameters.get("sway_degrees", 3.0)))
		instance.set("gust_period_s", float(parameters.get("gust_period_s", 7.0)))
		instance.set("wind_phase", wind_phase)
	parent.add_child(instance)
	return true

func _first_mesh_instance(node: Node) -> MeshInstance3D:
	if node is MeshInstance3D and (node as MeshInstance3D).mesh != null:
		return node as MeshInstance3D
	for child in node.get_children():
		var found := _first_mesh_instance(child)
		if found != null:
			return found
	return null

func _relative_transform(node: Node3D, root: Node3D) -> Transform3D:
	if node == root:
		return Transform3D.IDENTITY
	var result := node.transform
	var current := node.get_parent() as Node3D
	while current != null and current != root:
		result = current.transform * result
		current = current.get_parent() as Node3D
	return result

func _wind_multimesh_material(source: Material, effect: Dictionary) -> ShaderMaterial:
	var shader := Shader.new()
	shader.code = """
		shader_type spatial;
		render_mode cull_disabled, diffuse_burley;
		uniform sampler2D albedo_texture : source_color;
		uniform vec4 albedo_color : source_color = vec4(1.0);
		uniform bool has_albedo_texture = false;
		uniform float roughness_value = 0.9;
		uniform float sway_radians = 0.05;
		uniform float gust_period_s = 7.0;
		void vertex() {
			float phase = INSTANCE_CUSTOM.r * 6.2831853;
			float gust = sin(TIME * 6.2831853 / max(gust_period_s, 0.1) + phase);
			float height_weight = clamp(max(VERTEX.y, 0.0) / 18.0, 0.0, 1.0);
			VERTEX.x += gust * sway_radians * height_weight * max(VERTEX.y, 1.0);
		}
		void fragment() {
			vec4 sampled = has_albedo_texture ? texture(albedo_texture, UV) : vec4(1.0);
			vec3 imported_color = max(
				sampled.rgb * albedo_color.rgb,
				vec3(0.0001)
			);
			// source_color inputs are already converted to linear space. A
			// second gamma lift makes dark conifers glow cyan in perspective.
			ALBEDO = imported_color * 0.86;
			ALPHA = sampled.a * albedo_color.a;
			ALPHA_SCISSOR_THRESHOLD = 0.25;
			ROUGHNESS = roughness_value;
		}
	"""
	var material := ShaderMaterial.new()
	material.shader = shader
	var parameters: Dictionary = effect.get("parameters", {}) as Dictionary
	material.set_shader_parameter(
		"sway_radians",
		deg_to_rad(float(parameters.get("sway_degrees", 3.0)))
	)
	material.set_shader_parameter(
		"gust_period_s",
		float(parameters.get("gust_period_s", 7.0))
	)
	if source is BaseMaterial3D:
		var base := source as BaseMaterial3D
		material.set_shader_parameter("albedo_color", base.albedo_color)
		material.set_shader_parameter("roughness_value", base.roughness)
		if base.albedo_texture != null:
			material.set_shader_parameter("albedo_texture", base.albedo_texture)
			material.set_shader_parameter("has_albedo_texture", true)
	return material

func _multimesh_template(
	path: String,
	missing_assets: Array,
	wind_effect: Dictionary = {}
) -> Dictionary:
	var cache_key := path + "::" + (
		JSON.stringify(wind_effect) if not wind_effect.is_empty() else "static"
	)
	if _multimesh_template_cache.has(cache_key):
		var cached: Dictionary = _multimesh_template_cache[cache_key] as Dictionary
		return {
			"mesh": cached["mesh"],
			"local_transform": cached["local_transform"],
			"transforms": [],
			"custom_data": [],
		}
	if not ResourceLoader.exists(path, "PackedScene"):
		if not missing_assets.has(path):
			missing_assets.append(path)
		return {}
	var packed := load(path) as PackedScene
	if packed == null:
		if not missing_assets.has(path):
			missing_assets.append(path)
		return {}
	var instance := packed.instantiate() as Node3D
	if instance == null:
		if not missing_assets.has(path):
			missing_assets.append(path)
		return {}
	var mesh_node := _first_mesh_instance(instance)
	if mesh_node == null or mesh_node.mesh == null:
		instance.free()
		if not missing_assets.has(path):
			missing_assets.append(path)
		return {}
	var mesh := mesh_node.mesh.duplicate(true) as Mesh
	var local_transform := _relative_transform(mesh_node, instance)
	if not wind_effect.is_empty():
		for surface_index in range(mesh.get_surface_count()):
			mesh.surface_set_material(
				surface_index,
				_wind_multimesh_material(
					mesh.surface_get_material(surface_index),
					wind_effect
				)
			)
	instance.free()
	if _multimesh_resource_dir.is_empty():
		_multimesh_resource_errors.append(
			"MultiMesh resource directory was not initialized"
		)
		return {}
	var resource_path := _multimesh_resource_dir.path_join(
		cache_key.sha256_text() + ".res"
	)
	var save_result := ResourceSaver.save(
		mesh,
		resource_path,
		ResourceSaver.FLAG_COMPRESS
	)
	if save_result != OK:
		_multimesh_resource_errors.append(
			"Could not save external MultiMesh mesh: " + resource_path
		)
		return {}
	var external_mesh := ResourceLoader.load(
		resource_path,
		"",
		ResourceLoader.CACHE_MODE_REPLACE
	) as Mesh
	if external_mesh == null:
		_multimesh_resource_errors.append(
			"Could not reload external MultiMesh mesh: " + resource_path
		)
		return {}
	_multimesh_resource_paths[resource_path] = true
	_multimesh_template_cache[cache_key] = {
		"mesh": external_mesh,
		"local_transform": local_transform,
	}
	return {
		"mesh": external_mesh,
		"local_transform": local_transform,
		"transforms": [],
		"custom_data": [],
	}

func _emit_multimesh_groups(
	parent: Node3D,
	groups: Dictionary,
	name_prefix: String,
	material_override: Material = null
) -> Dictionary:
	var batch_count := 0
	var instance_count := 0
	var chunk_size_m := 480.0
	var max_instances_per_chunk := 0
	var sorted_paths := groups.keys()
	sorted_paths.sort()
	for group_key_var in sorted_paths:
		var group_key := str(group_key_var)
		var group: Dictionary = groups[group_key] as Dictionary
		var source_transforms: Array = group.get("transforms", []) as Array
		var source_custom_data: Array = group.get("custom_data", []) as Array
		var chunks: Dictionary = {}
		for transform_index in range(source_transforms.size()):
			var source_transform: Transform3D = source_transforms[transform_index]
			var cell_x := floori(source_transform.origin.x / chunk_size_m)
			var cell_z := floori(source_transform.origin.z / chunk_size_m)
			var chunk_key := "%d:%d" % [cell_x, cell_z]
			if not chunks.has(chunk_key):
				chunks[chunk_key] = {
					"cell_x": cell_x,
					"cell_z": cell_z,
					"transforms": [],
					"custom_data": [],
				}
			var chunk: Dictionary = chunks[chunk_key] as Dictionary
			(chunk["transforms"] as Array).append(source_transform)
			if not source_custom_data.is_empty():
				(chunk["custom_data"] as Array).append(
					source_custom_data[transform_index]
				)
		var sorted_chunk_keys := chunks.keys()
		sorted_chunk_keys.sort()
		for chunk_key_var in sorted_chunk_keys:
			var chunk_key := str(chunk_key_var)
			var chunk: Dictionary = chunks[chunk_key] as Dictionary
			var transforms: Array = chunk.get("transforms", []) as Array
			if transforms.is_empty():
				continue
			var custom_data: Array = chunk.get("custom_data", []) as Array
			var multimesh := MultiMesh.new()
			multimesh.transform_format = MultiMesh.TRANSFORM_3D
			multimesh.use_custom_data = not custom_data.is_empty()
			multimesh.mesh = group.get("mesh") as Mesh
			multimesh.instance_count = transforms.size()
			var stride := 16 if not custom_data.is_empty() else 12
			var buffer := PackedFloat32Array()
			buffer.resize(transforms.size() * stride)
			for index in range(transforms.size()):
				var instance_transform: Transform3D = transforms[index]
				var offset := index * stride
				# RenderingServer's 3D MultiMesh buffer uses row-major Transform3D
				# order: three basis rows with the origin in each fourth slot.
				buffer[offset + 0] = instance_transform.basis.x.x
				buffer[offset + 1] = instance_transform.basis.y.x
				buffer[offset + 2] = instance_transform.basis.z.x
				buffer[offset + 3] = instance_transform.origin.x
				buffer[offset + 4] = instance_transform.basis.x.y
				buffer[offset + 5] = instance_transform.basis.y.y
				buffer[offset + 6] = instance_transform.basis.z.y
				buffer[offset + 7] = instance_transform.origin.y
				buffer[offset + 8] = instance_transform.basis.x.z
				buffer[offset + 9] = instance_transform.basis.y.z
				buffer[offset + 10] = instance_transform.basis.z.z
				buffer[offset + 11] = instance_transform.origin.z
				if not custom_data.is_empty():
					var instance_custom_data: Color = custom_data[index]
					buffer[offset + 12] = instance_custom_data.r
					buffer[offset + 13] = instance_custom_data.g
					buffer[offset + 14] = instance_custom_data.b
					buffer[offset + 15] = instance_custom_data.a
			multimesh.buffer = buffer
			var node := MultiMeshInstance3D.new()
			node.name = "%s_%03d" % [name_prefix, batch_count]
			node.multimesh = multimesh
			node.material_override = material_override
			var lod_tier := str(group.get("lod_tier", ""))
			if lod_tier == "lod0":
				node.visibility_range_end = 240.0
				node.visibility_range_end_margin = 40.0
				node.visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF
			elif lod_tier == "lod1":
				node.visibility_range_begin = 180.0
				node.visibility_range_begin_margin = 40.0
				node.visibility_range_end = 720.0
				node.visibility_range_end_margin = 100.0
				node.visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF
			elif lod_tier == "lod2":
				node.visibility_range_begin = 600.0
				node.visibility_range_begin_margin = 100.0
				node.visibility_range_end = 4500.0
				node.visibility_range_end_margin = 250.0
				node.visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF
			node.set_meta("codeweald_asset_path", str(group.get("asset_path", group_key)))
			node.set_meta("codeweald_instance_count", transforms.size())
			node.set_meta("codeweald_chunk_size_m", chunk_size_m)
			node.set_meta("codeweald_chunk_x", int(chunk.get("cell_x", 0)))
			node.set_meta("codeweald_chunk_z", int(chunk.get("cell_z", 0)))
			node.set_meta("codeweald_lod_tier", lod_tier)
			parent.add_child(node)
			batch_count += 1
			instance_count += transforms.size()
			max_instances_per_chunk = maxi(
				max_instances_per_chunk, transforms.size()
			)
	return {
		"batch_count": batch_count,
		"instance_count": instance_count,
		"chunk_size_m": chunk_size_m,
		"max_instances_per_chunk": max_instances_per_chunk,
	}

func _distance_to_segment(point: Vector2, start: Vector2, finish: Vector2) -> float:
	var segment := finish - start
	var length_squared := segment.length_squared()
	if length_squared <= 0.0001:
		return point.distance_to(start)
	var offset := clampf((point - start).dot(segment) / length_squared, 0.0, 1.0)
	return point.distance_to(start + segment * offset)

func _scatter_excluded(candidate: Vector2, exclusions: Array) -> bool:
	for exclusion_var in exclusions:
		var exclusion := exclusion_var as Dictionary
		var radius := float(exclusion.get("radius_m", 0.0))
		var points := exclusion.get("points", []) as Array
		if points.is_empty() or radius <= 0.0:
			continue
		if points.size() == 1:
			if candidate.distance_to(points[0] as Vector2) < radius:
				return true
			continue
		for index in range(points.size() - 1):
			if _distance_to_segment(candidate, points[index] as Vector2, points[index + 1] as Vector2) < radius:
				return true
	return false

func _scatter_exclusions(features: Array) -> Array:
	var exclusions: Array = []
	for feature_var in features:
		var feature := feature_var as Dictionary
		var semantic := str(feature.get("semantic", ""))
		var geometry := feature.get("geometry", {}) as Dictionary
		var raw_points := geometry.get("points", []) as Array
		if raw_points.is_empty():
			continue
		var points: Array[Vector2] = []
		for raw_point_var in raw_points:
			var raw_point := raw_point_var as Array
			if raw_point.size() >= 2:
				points.append(Vector2(float(raw_point[0]), float(raw_point[1])))
		var properties := feature.get("properties", {}) as Dictionary
		var radius := 0.0
		if semantic == "lane":
			radius = float(properties.get("minimum_width_m", 24.0)) * 0.5 + 7.0
		elif semantic == "stream":
			radius = float(properties.get("width_m", 10.0)) * 0.5 + 5.0
		elif semantic == "faction_keep":
			radius = float(properties.get("scatter_exclusion_radius_m", 150.0))
		elif semantic == "arcane_ruin":
			radius = float(properties.get("scatter_exclusion_radius_m", 58.0))
		elif semantic == "settlement_cluster":
			radius = float(properties.get("scatter_exclusion_radius_m", 44.0))
		if radius > 0.0 and not points.is_empty():
			exclusions.append({"points": points, "radius_m": radius, "feature_id": feature.get("id", "")})
	return exclusions

func _scatter_forest_layer(parent: Node3D, polygon: PackedVector2Array, bounds: Rect2, layer: Dictionary, wind_effect: Dictionary, rng: RandomNumberGenerator, terrain_sampler: Dictionary, missing_assets: Array, exclusions: Array) -> int:
	if not bool(layer.get("runtime_enabled", true)):
		return 0
	var assets: Array = layer.get("assets", []) as Array
	var scales: Array = layer.get("scale_m", [1.0, 1.0]) as Array
	if assets.is_empty() or scales.size() != 2:
		push_warning("No resolved Godot assets for ecological layer " + str(layer.get("id", "unnamed")))
		return 0
	var layer_root := Node3D.new()
	layer_root.name = "Layer_" + str(layer.get("id", "primary"))
	parent.add_child(layer_root)
	var target_count := int(layer.get("instance_count", 0))
	var minimum_spacing := float(layer.get("minimum_spacing_m", 0.0))
	var groups: Dictionary = {}
	var families: Array = []
	var asset_variants: Array = layer.get("asset_variants", []) as Array
	for variant_var in asset_variants:
		var variant := variant_var as Dictionary
		var levels: Dictionary = variant.get("levels", {}) as Dictionary
		var family_groups: Dictionary = {}
		for lod_tier_var in ["lod0", "lod1", "lod2"]:
			var lod_tier := str(lod_tier_var)
			var asset_path := str(levels.get(lod_tier, ""))
			if asset_path.is_empty():
				continue
			var template := _multimesh_template(
				asset_path, missing_assets, wind_effect
			)
			if template.is_empty():
				continue
			var group_key := str(variant.get("family", asset_path)) + "|" + lod_tier
			template["asset_path"] = asset_path
			template["lod_tier"] = lod_tier
			groups[group_key] = template
			family_groups[lod_tier] = group_key
		if (
			family_groups.has("lod0")
			and family_groups.has("lod1")
			and family_groups.has("lod2")
		):
			families.append(family_groups)
	if families.is_empty():
		for asset_var in assets:
			var asset_path := str(asset_var)
			var template := _multimesh_template(
				asset_path, missing_assets, wind_effect
			)
			if not template.is_empty():
				template["asset_path"] = asset_path
				template["lod_tier"] = ""
				groups[asset_path] = template
				families.append({"base": asset_path})
	if families.is_empty():
		return 0
	var placed_positions: Array[Vector2] = []
	var placed := 0
	var attempts := 0
	while placed < target_count and attempts < target_count * 30:
		attempts += 1
		var candidate := Vector2(rng.randf_range(bounds.position.x, bounds.end.x), rng.randf_range(bounds.position.y, bounds.end.y))
		if not Geometry2D.is_point_in_polygon(candidate, polygon):
			continue
		if _scatter_excluded(candidate, exclusions):
			continue
		var overlaps_existing := false
		for existing in placed_positions:
			if minimum_spacing > 0.0 and candidate.distance_to(existing) < minimum_spacing:
				overlaps_existing = true
				break
		if overlaps_existing:
			continue
		var ground_y := ZoneTerrainBuilderScript.sample_height(terrain_sampler, candidate.x, candidate.y)
		var family: Dictionary = families[
			rng.randi_range(0, families.size() - 1)
		] as Dictionary
		var scale_value := rng.randf_range(float(scales[0]), float(scales[1]))
		var rotation_y := rng.randf_range(0.0, TAU)
		var wind_phase := rng.randf_range(0.0, TAU)
		var placement := Transform3D(
			Basis(Vector3.UP, rotation_y).scaled(Vector3.ONE * scale_value),
			Vector3(candidate.x, ground_y, candidate.y)
		)
		for group_key_var in family.values():
			var group_key := str(group_key_var)
			var group: Dictionary = groups[group_key] as Dictionary
			var local_transform: Transform3D = group["local_transform"]
			(group["transforms"] as Array).append(
				placement * local_transform
			)
			if not wind_effect.is_empty():
				(group["custom_data"] as Array).append(
					Color(wind_phase / TAU, 0.0, 0.0, 1.0)
				)
		placed += 1
		placed_positions.append(candidate)
	var emitted := _emit_multimesh_groups(
		layer_root,
		groups,
		"MultiMesh_" + str(layer.get("id", "primary"))
	)
	layer_root.set_meta("codeweald_multimesh_batches", emitted["batch_count"])
	layer_root.set_meta("codeweald_instances", placed)
	return placed

func _add_forest_assets(scene_root: Node3D, feature: Dictionary, assignment: Dictionary, wind_effect: Dictionary, rng: RandomNumberGenerator, terrain_sampler: Dictionary, missing_assets: Array, exclusions: Array) -> int:
	var points: Array = feature.get("geometry", {}).get("points", [])
	if points.size() < 3:
		return 0
	var polygon := PackedVector2Array()
	var min_x := INF
	var max_x := -INF
	var min_z := INF
	var max_z := -INF
	for point_var in points:
		var point: Array = point_var as Array
		var x := float(point[0])
		var z := float(point[1])
		polygon.append(Vector2(x, z))
		min_x = minf(min_x, x); max_x = maxf(max_x, x)
		min_z = minf(min_z, z); max_z = maxf(max_z, z)
	var forest := Node3D.new()
	forest.name = "Foliage_" + str(feature.get("id", "forest"))
	scene_root.add_child(forest)
	var authored_layers: Array = assignment.get("layers", []) as Array
	if authored_layers.is_empty():
		authored_layers = [assignment]
	var placed := 0
	var scatter_bounds := Rect2(Vector2(min_x, min_z), Vector2(max_x - min_x, max_z - min_z))
	for layer_var in authored_layers:
		if layer_var is Dictionary:
			var layer: Dictionary = layer_var as Dictionary
			# Rocks are part of the ecological dressing but should not bend in wind.
			var layer_wind := (
				wind_effect
				if str(layer.get("role", "")) in [
					"canopy",
					"undergrowth",
					"conifer_canopy",
					"highland_understory",
					"highland_groundcover",
				]
				else {}
			)
			placed += _scatter_forest_layer(forest, polygon, scatter_bounds, layer, layer_wind, rng, terrain_sampler, missing_assets, exclusions)
	forest.set_meta("codeweald_instances", placed)
	return placed

func _add_landform_assets(
	scene_root: Node3D,
	feature: Dictionary,
	assignment: Dictionary,
	placement_plan: Dictionary,
	asset_paths_by_sha256: Dictionary,
	terrain_sampler: Dictionary,
	missing_assets: Array,
	material_override: Material = null
) -> int:
	if not bool(assignment.get("runtime_enabled", true)):
		return 0
	var feature_id := str(feature.get("id", "landform"))
	var solved: Array[Dictionary] = []
	for placement_var in placement_plan.get("placements", []):
		var placement_record := placement_var as Dictionary
		if str(placement_record.get("feature_id", "")) == feature_id:
			solved.append(placement_record)
	var target_count := int(assignment.get("instance_count", 0))
	if solved.size() != target_count:
		push_error(
			"Certified placement plan contains %d of %d transforms for %s"
			% [solved.size(), target_count, feature_id]
		)
		return 0
	var groups: Dictionary = {}
	for placement_record in solved:
		var asset_sha256 := str(placement_record.get("asset_sha256", ""))
		var asset_path := str(asset_paths_by_sha256.get(asset_sha256, ""))
		if asset_path.is_empty():
			push_error("Certified placement references unmapped asset " + asset_sha256)
			continue
		if groups.has(asset_path):
			continue
		var template := _multimesh_template(asset_path, missing_assets)
		if not template.is_empty():
			groups[asset_path] = template
	if groups.is_empty():
		return 0
	var crags := Node3D.new()
	crags.name = "Crags_" + feature_id
	scene_root.add_child(crags)
	var placed := 0
	var maximum_grounding_residual := 0.0
	for placement_record in solved:
		var position_values: Array = placement_record.get("position_m", []) as Array
		if position_values.size() != 3:
			continue
		var position := Vector3(
			float(position_values[0]),
			float(position_values[1]),
			float(position_values[2])
		)
		var sampled_ground := ZoneTerrainBuilderScript.sample_height(
			terrain_sampler, position.x, position.z
		)
		maximum_grounding_residual = maxf(
			maximum_grounding_residual, absf(sampled_ground - position.y)
		)
		var asset_path := str(
			asset_paths_by_sha256.get(
				str(placement_record.get("asset_sha256", "")), ""
			)
		)
		if not groups.has(asset_path):
			continue
		var scale_value := float(placement_record.get("scale", 0.0))
		var rotation_y := deg_to_rad(
			float(placement_record.get("yaw_degrees", 0.0))
		)
		var placement := Transform3D(
			Basis(Vector3.UP, rotation_y).scaled(Vector3.ONE * scale_value),
			position
		)
		var group: Dictionary = groups[asset_path] as Dictionary
		var local_transform: Transform3D = group["local_transform"]
		(group["transforms"] as Array).append(
			placement * local_transform
		)
		placed += 1
	var emitted := _emit_multimesh_groups(
		crags,
		groups,
		"MultiMeshCrag",
		material_override
	)
	crags.set_meta("codeweald_multimesh_batches", emitted["batch_count"])
	crags.set_meta("codeweald_instances", placed)
	crags.set_meta("codeweald_placement_authority", "julia_rust_certified")
	crags.set_meta(
		"codeweald_maximum_grounding_residual_m", maximum_grounding_residual
	)
	return placed

func _add_camera(scene_root: Node3D, world_bounds: Dictionary) -> void:
	var camera := Camera3D.new()
	camera.name = "ZoneOverviewCamera"
	scene_root.add_child(camera)
	camera.projection = Camera3D.PROJECTION_ORTHOGONAL
	camera.keep_aspect = Camera3D.KEEP_HEIGHT
	var width := maxf(float(world_bounds.get("width", 2400.0)), 1.0)
	var length := maxf(float(world_bounds.get("length", 1600.0)), 1.0)
	var span := maxf(width, length)
	camera.size = span * 0.83
	# The concept is an oblique cartographic painting, not a satellite image.
	# Match that evidence camera so tree, keep, and mountain silhouettes remain
	# visible in the primary fidelity capture.
	camera.look_at_from_position(
		Vector3(0.0, span * 0.86, -span * 0.50),
		Vector3(0.0, span * 0.02, 0.0),
		Vector3(0.0, 0.0, 1.0)
	)
	# Godot's camera basis mirrors screen X when this south-side oblique view
	# uses world +Z as screen-up. Reflecting camera-local X restores the
	# ZoneSpec cartographic contract: world +X is source-image right.
	camera.scale.x = -0.82
	camera.make_current()

func _landform_material(terrain_manifest_path: String) -> Material:
	# Generated crag meshes do not share a reliable UV convention. Applying the
	# terrain PBR bundle as a UV texture produced bright silver striping, while
	# importer fallbacks often rendered white. Use the reviewed rock palette as
	# a deterministic matte material until the asset contract grows triplanar
	# coordinates or verified UV metadata.
	var manifest := _read_json(terrain_manifest_path)
	var palette: Dictionary = manifest.get("style_palette_srgb", {}) as Dictionary
	var rock_channels: Array = palette.get("rock", [0.20, 0.22, 0.23]) as Array
	if rock_channels.size() != 3:
		rock_channels = [0.20, 0.22, 0.23]
	var shader := Shader.new()
	shader.code = """
		shader_type spatial;
		render_mode diffuse_burley;
		uniform vec3 rock_color = vec3(0.14, 0.14, 0.11);
		varying vec3 codeweald_world_position;
		void vertex() {
			codeweald_world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
		}
		void fragment() {
			float facet = (
				sin(codeweald_world_position.x * 0.31)
				* sin(codeweald_world_position.y * 0.27)
				* cos(codeweald_world_position.z * 0.23)
			);
			float rim = pow(
				1.0 - clamp(dot(normalize(NORMAL), normalize(VIEW)), 0.0, 1.0),
				2.4
			);
			ALBEDO = rock_color * (0.82 + facet * 0.18);
			// A small view-dependent mineral edge preserves the sharp granite
			// silhouette visible in the concept without turning whole crags
			// white under the player camera.
			EMISSION = rock_color * rim * 0.16;
			ROUGHNESS = 0.94;
			METALLIC = 0.0;
		}
	"""
	var material := ShaderMaterial.new()
	material.resource_name = "CodewealdMatteAlpineGranite"
	material.shader = shader
	material.set_shader_parameter(
		"rock_color",
		Vector3(
			float(rock_channels[0]) * 0.82,
			float(rock_channels[1]) * 0.82,
			float(rock_channels[2]) * 0.82
		)
	)
	return material

func _assignment_by_feature(asset_plan: Dictionary, feature_id: String) -> Dictionary:
	for assignment_var in asset_plan.get("assignments", []):
		var assignment := assignment_var as Dictionary
		if str(assignment.get("feature_id", "")) == feature_id:
			return assignment
	return {}

func _collect_multimesh_evidence(node: Node, evidence: Dictionary) -> void:
	if node is MultiMeshInstance3D:
		var multimesh := (node as MultiMeshInstance3D).multimesh
		if multimesh != null:
			evidence["batch_count"] = int(evidence["batch_count"]) + 1
			evidence["instance_count"] = (
				int(evidence["instance_count"]) + multimesh.instance_count
			)
			var stride := 12
			if multimesh.use_colors:
				stride += 4
			if multimesh.use_custom_data:
				stride += 4
			var buffer_float_count := multimesh.buffer.size()
			var expected_buffer_float_count := multimesh.instance_count * stride
			evidence["buffer_float_count"] = (
				int(evidence["buffer_float_count"]) + buffer_float_count
			)
			evidence["expected_buffer_float_count"] = (
				int(evidence["expected_buffer_float_count"])
				+ expected_buffer_float_count
			)
			if buffer_float_count == expected_buffer_float_count:
				evidence["serialized_instance_count"] = (
					int(evidence["serialized_instance_count"])
					+ multimesh.instance_count
				)
			else:
				evidence["invalid_buffer_batch_count"] = (
					int(evidence["invalid_buffer_batch_count"]) + 1
				)
			if node.has_meta("codeweald_chunk_size_m"):
				evidence["spatial_chunk_batch_count"] = (
					int(evidence["spatial_chunk_batch_count"]) + 1
				)
				evidence["chunk_size_m"] = float(
					node.get_meta("codeweald_chunk_size_m")
				)
				evidence["max_instances_per_chunk"] = maxi(
					int(evidence["max_instances_per_chunk"]),
					multimesh.instance_count
				)
			var lod_tier := str(node.get_meta("codeweald_lod_tier", ""))
			if not lod_tier.is_empty():
				var lod_counts: Dictionary = evidence["lod_instance_counts"]
				lod_counts[lod_tier] = (
					int(lod_counts.get(lod_tier, 0)) + multimesh.instance_count
				)
	for child in node.get_children():
		_collect_multimesh_evidence(child, evidence)

func _collect_terrain_material_evidence(
	node: Node,
	terrain_manifest: Dictionary,
	evidence: Dictionary
) -> void:
	if (
		node is MeshInstance3D
		and node.has_meta("codeweald_terrain_artifacts")
	):
		var material := (node as MeshInstance3D).material_override as ShaderMaterial
		var contracts: Dictionary = terrain_manifest.get(
			"terrain_materials", {}
		) as Dictionary
		evidence["requested_layer_count"] = contracts.size()
		var minimum_texels := INF
		for layer_var in contracts:
			var layer := str(layer_var)
			var contract: Dictionary = contracts[layer] as Dictionary
			var maps: Dictionary = contract.get("maps", {}) as Dictionary
			var maps_bound := material != null
			for kind in ["albedo", "normal", "roughness"]:
				if (
					not maps.has(kind)
					or material == null
					or material.get_shader_parameter(
						layer + "_" + str(kind)
					) == null
				):
					maps_bound = false
			if maps_bound:
				evidence["bound_layer_count"] = (
					int(evidence["bound_layer_count"]) + 1
				)
			if (
				material != null
				and bool(
					material.get_shader_parameter("has_" + layer + "_pbr")
				)
			):
				evidence["pbr_layer_count"] = (
					int(evidence["pbr_layer_count"]) + 1
				)
			var texels_per_meter := float(
				contract.get("minimum_texels_per_meter", 0.0)
			)
			minimum_texels = minf(minimum_texels, texels_per_meter)
			(evidence["meters_per_repeat"] as Dictionary)[layer] = float(
				contract.get("meters_per_repeat", 0.0)
			)
		evidence["minimum_texels_per_meter"] = (
			0.0 if minimum_texels == INF else minimum_texels
		)
		evidence["wetland_mask_bound"] = (
			material != null
			and material.get_shader_parameter("wetland_mask") != null
		)
	for child in node.get_children():
		_collect_terrain_material_evidence(
			child, terrain_manifest, evidence
		)

func _collect_navigation_link_evidence(
	node: Node,
	evidence: Dictionary
) -> void:
	if (
		node is NavigationLink3D
		and node.has_meta("codeweald_off_mesh_link")
	):
		evidence["link_count"] = int(evidence["link_count"]) + 1
		if (node as NavigationLink3D).enabled:
			evidence["enabled_link_count"] = (
				int(evidence["enabled_link_count"]) + 1
			)
		if (
			(node as NavigationLink3D).start_position.distance_to(
				(node as NavigationLink3D).end_position
			) >= 20.0
		):
			evidence["full_span_link_count"] = (
				int(evidence["full_span_link_count"]) + 1
			)
	for child in node.get_children():
		_collect_navigation_link_evidence(child, evidence)

func _collect_source_asset_paths(node: Node, paths: Array) -> void:
	if node.has_meta("codeweald_source_asset"):
		var path := str(node.get_meta("codeweald_source_asset"))
		if not path.is_empty() and not paths.has(path):
			paths.append(path)
	for child in node.get_children():
		_collect_source_asset_paths(child, paths)

func _collect_profile_variant_evidence(
	node: Node,
	evidence: Dictionary
) -> void:
	if node.has_meta("codeweald_profile_id"):
		var profile_id := str(node.get_meta("codeweald_profile_id"))
		if not profile_id.is_empty():
			var paths: Array = []
			_collect_source_asset_paths(node, paths)
			if not evidence.has(profile_id):
				evidence[profile_id] = []
			var profile_paths: Array = evidence[profile_id] as Array
			for path in paths:
				if not profile_paths.has(path):
					profile_paths.append(path)
	for child in node.get_children():
		_collect_profile_variant_evidence(child, evidence)

func _init() -> void:
	var batch_path := _batch_path()
	if batch_path.is_empty():
		quit(1)
		return
	_multimesh_resource_dir = batch_path.path_join(
		"terrain/multimesh_meshes"
	)
	var resource_dir_result := DirAccess.make_dir_recursive_absolute(
		ProjectSettings.globalize_path(_multimesh_resource_dir)
	)
	if resource_dir_result != OK:
		printerr(
			"ZoneSpec scene compiler could not create MultiMesh resource directory: "
			+ _multimesh_resource_dir
		)
		quit(1)
		return
	var output_scene_path := _output_scene_path()
	if output_scene_path.is_empty():
		quit(1)
		return
	var zone_spec_path := batch_path.path_join("zone_spec.json")
	var terrain_manifest_path := batch_path.path_join("terrain/terrain_manifest.json")
	var godot_asset_plan_path := batch_path.path_join("godot_asset_plan.json")
	var placement_plan_path := batch_path.path_join("placement_plan.json")
	var runtime_effects_path := batch_path.path_join("runtime_effects.json")
	var style_reference_path := batch_path.path_join("style_reference.json")
	var style_calibration_path := batch_path.path_join("style_calibration.json")
	var build_report_path := batch_path.path_join("terrain/godot_build_report.json")
	var zone_spec := _read_json(zone_spec_path)
	if zone_spec.is_empty():
		quit(1)
		return
	var asset_plan := _read_json(godot_asset_plan_path)
	if asset_plan.is_empty() or str(asset_plan.get("zone_id", "")) != str(zone_spec.get("zone", {}).get("id", "")):
		printerr("ZoneSpec scene compiler requires a matching Godot asset plan")
		quit(1)
		return
	var placement_plan := _read_json(placement_plan_path)
	if (
		placement_plan.is_empty()
		or str(placement_plan.get("schema_version", ""))
		!= "codeweald.placement-plan/v1"
	):
		printerr("ZoneSpec scene compiler requires a certified placement plan")
		quit(1)
		return
	var asset_paths_by_sha256: Dictionary = (
		asset_plan.get("asset_paths_by_sha256", {}) as Dictionary
	)
	var runtime_effects := _read_json(runtime_effects_path)
	if runtime_effects.is_empty() or str(runtime_effects.get("zone_id", "")) != str(zone_spec.get("zone", {}).get("id", "")):
		printerr("ZoneSpec scene compiler requires matching runtime effects")
		quit(1)
		return
	var style_reference := _read_json(style_reference_path)
	if style_reference.is_empty():
		printerr("ZoneSpec scene compiler requires a source style reference")
		quit(1)
		return
	var style_calibration := _read_json(style_calibration_path)
	if (
		style_calibration.is_empty()
		or str(style_calibration.get("source_sha256", ""))
			!= str((style_reference.get("source", {}) as Dictionary).get("sha256", ""))
	):
		printerr("ZoneSpec scene compiler requires matching style calibration")
		quit(1)
		return
	var scene_root := Node3D.new()
	scene_root.name = "CodewealdZone_" + str(zone_spec.get("zone", {}).get("id", "generated"))
	scene_root.set_meta("codeweald_zone_spec", zone_spec_path)
	scene_root.set_meta("codeweald_style_reference", style_reference_path)
	scene_root.set_meta("codeweald_style_calibration", style_calibration_path)
	scene_root.set_meta("codeweald_generated", true)
	_add_environment(scene_root, style_reference, style_calibration)
	var terrain := ZoneTerrainBuilderScript.build_from_manifest(terrain_manifest_path)
	if terrain.mesh == null:
		printerr("ZoneSpec scene compiler could not build terrain")
		quit(1)
		return
	var terrain_resource_path := batch_path.path_join(
		"terrain/terrain_mesh.res"
	)
	var terrain_save_result := ResourceSaver.save(
		terrain.mesh,
		terrain_resource_path,
		ResourceSaver.FLAG_COMPRESS
	)
	if terrain_save_result != OK:
		printerr(
			"ZoneSpec scene compiler could not save external terrain mesh: "
			+ terrain_resource_path
		)
		quit(1)
		return
	var external_terrain_mesh := ResourceLoader.load(
		terrain_resource_path,
		"",
		ResourceLoader.CACHE_MODE_REPLACE
	) as Mesh
	if external_terrain_mesh == null:
		printerr("ZoneSpec scene compiler could not reload external terrain mesh")
		quit(1)
		return
	terrain.mesh = external_terrain_mesh
	scene_root.add_child(terrain)
	var terrain_sampler := ZoneTerrainBuilderScript.load_height_sampler(terrain_manifest_path)
	if terrain_sampler.is_empty():
		printerr("ZoneSpec scene compiler could not load terrain height sampler")
		quit(1)
		return
	var collision_evidence := _add_collision(
		scene_root,
		terrain_sampler,
		batch_path.path_join("terrain/terrain_collision.res")
	)
	if (
		int(collision_evidence.get("sample_count", 0)) <= 0
		or not bool(collision_evidence.get("external", false))
	):
		printerr("ZoneSpec scene compiler could not build external terrain collision")
		quit(1)
		return
	var navigation_evidence := _add_navigation_region(
		scene_root, terrain_sampler, zone_spec
	)
	var rng := RandomNumberGenerator.new()
	rng.seed = int(zone_spec.get("generation_seed", 1))
	var landform_material := _landform_material(terrain_manifest_path)
	var lane_material := _lane_surface_material(terrain_manifest_path)
	var scatter_exclusions := _scatter_exclusions(zone_spec.get("features", []) as Array)
	var foliage_instances := 0
	var crag_instances := 0
	var keep_count := 0
	var lane_path_count := 0
	var lane_surface_count := 0
	var waterway_count := 0
	var bridge_count := 0
	var ruin_count := 0
	var settlement_count := 0
	var missing_assets: Array = []
	for feature_var in zone_spec.get("features", []):
		var feature: Dictionary = feature_var as Dictionary
		if feature.get("category") == "landmark" and feature.get("semantic") == "faction_keep":
			if _add_keep(scene_root, feature, _effect_by_feature(runtime_effects, str(feature.get("id", ""))), _assignment_by_feature(asset_plan, str(feature.get("id", ""))), terrain_sampler, missing_assets):
				keep_count += 1
		elif feature.get("category") == "corridor" and feature.get("semantic") == "lane":
			if _add_lane_path(scene_root, feature, terrain_sampler, lane_material):
				lane_path_count += 1
				lane_surface_count += 1
		elif feature.get("category") == "hydrology" and feature.get("semantic") == "stream":
			if _add_stream(scene_root, feature, _effect_by_feature(runtime_effects, str(feature.get("id", ""))), terrain_sampler):
				waterway_count += 1
		elif feature.get("category") == "structure" and feature.get("semantic") == "bridge":
			if _add_bridge(scene_root, feature, _assignment_by_feature(asset_plan, str(feature.get("id", ""))), terrain_sampler, missing_assets):
				bridge_count += 1
		elif feature.get("category") == "landmark" and feature.get("semantic") == "arcane_ruin":
			if _add_arcane_ruin(scene_root, feature, _assignment_by_feature(asset_plan, str(feature.get("id", ""))), _effect_by_feature(runtime_effects, str(feature.get("id", ""))), terrain_sampler, missing_assets):
				ruin_count += 1
		elif feature.get("category") == "landmark" and feature.get("semantic") == "settlement_cluster":
			if _add_settlement(scene_root, feature, _assignment_by_feature(asset_plan, str(feature.get("id", ""))), terrain_sampler, missing_assets):
				settlement_count += 1
		elif feature.get("category") == "biome" and feature.get("semantic") == "forest":
			foliage_instances += _add_forest_assets(scene_root, feature, _assignment_by_feature(asset_plan, str(feature.get("id", ""))), _effect_by_feature(runtime_effects, str(feature.get("id", ""))), rng, terrain_sampler, missing_assets, scatter_exclusions)
		elif (
			feature.get("category") == "landform"
			and not str(feature.get("generation", {}).get("asset_profile", "")).is_empty()
		):
			crag_instances += _add_landform_assets(
				scene_root,
				feature,
				_assignment_by_feature(asset_plan, str(feature.get("id", ""))),
				placement_plan,
				asset_paths_by_sha256,
				terrain_sampler,
				missing_assets,
				landform_material
			)
	for assignment_var in asset_plan.get("assignments", []):
		var assignment := assignment_var as Dictionary
		for unavailable_var in assignment.get("unavailable_assets", []):
			var unavailable := str(unavailable_var)
			if not missing_assets.has(unavailable):
				missing_assets.append(unavailable)
	if not _multimesh_resource_errors.is_empty():
		for resource_error in _multimesh_resource_errors:
			printerr(resource_error)
		quit(1)
		return
	_add_camera(scene_root, zone_spec.get("zone", {}).get("world_bounds", {}) as Dictionary)
	_set_owner_recursive(scene_root, scene_root)
	var packed_scene := PackedScene.new()
	var pack_result := packed_scene.pack(scene_root)
	if pack_result != OK:
		printerr("ZoneSpec scene compiler could not pack scene")
		quit(1)
		return
	var save_result := ResourceSaver.save(packed_scene, output_scene_path)
	if save_result != OK:
		printerr("ZoneSpec scene compiler could not save candidate scene: " + output_scene_path)
		quit(1)
		return
	# Acceptance evidence must describe the scene Godot can load back from disk,
	# not the mutable resources that happened to exist before ResourceSaver ran.
	var multimesh_evidence := {
		"batch_count": 0,
		"instance_count": 0,
		"serialized_instance_count": 0,
		"buffer_float_count": 0,
		"expected_buffer_float_count": 0,
		"invalid_buffer_batch_count": 0,
		"spatial_chunk_batch_count": 0,
		"chunk_size_m": 0.0,
		"max_instances_per_chunk": 0,
		"lod_instance_counts": {},
	}
	var terrain_manifest := _read_json(terrain_manifest_path)
	var terrain_material_evidence := {
		"requested_layer_count": 0,
		"bound_layer_count": 0,
		"pbr_layer_count": 0,
		"minimum_texels_per_meter": 0.0,
		"meters_per_repeat": {},
		"wetland_mask_bound": false,
	}
	var navigation_link_evidence := {
		"link_count": 0,
		"enabled_link_count": 0,
		"full_span_link_count": 0,
	}
	var profile_variant_evidence := {}
	var saved_scene := ResourceLoader.load(
		output_scene_path,
		"",
		ResourceLoader.CACHE_MODE_REPLACE
	) as PackedScene
	if saved_scene != null:
		var saved_root := saved_scene.instantiate()
		if saved_root != null:
			_collect_multimesh_evidence(saved_root, multimesh_evidence)
			_collect_terrain_material_evidence(
				saved_root,
				terrain_manifest,
				terrain_material_evidence
			)
			_collect_navigation_link_evidence(
				saved_root, navigation_link_evidence
			)
			_collect_profile_variant_evidence(
				saved_root, profile_variant_evidence
			)
			saved_root.free()
	var build_report := {
		"schema_version": "codeweald.godot-zone-build/v1",
		"zone_id": zone_spec.get("zone", {}).get("id", "unknown"),
		"scene_path": output_scene_path,
		"scene_written": true,
		"keep_count": keep_count,
		"lane_path_count": lane_path_count,
		"lane_surface_count": lane_surface_count,
		"navigation_vertex_count": navigation_evidence.get("vertex_count", 0),
		"navigation_polygon_count": navigation_evidence.get("polygon_count", 0),
			"navigation_cell_size_m": navigation_evidence.get("cell_size_m", 0.0),
			"navigation_off_mesh_link_count": navigation_link_evidence["link_count"],
			"navigation_enabled_off_mesh_link_count": navigation_link_evidence["enabled_link_count"],
			"navigation_full_span_off_mesh_link_count": navigation_link_evidence["full_span_link_count"],
			"landmark_asset_variants": profile_variant_evidence,
		"waterway_count": waterway_count,
		"bridge_count": bridge_count,
		"ruin_count": ruin_count,
		"settlement_count": settlement_count,
		"settlement_grounded_component_count": _settlement_grounded_component_count,
		"settlement_grounding_max_adjustment_m": snappedf(
			_settlement_grounding_max_adjustment_m, 0.0001
		),
		"settlement_grounding_max_residual_m": snappedf(
			_settlement_grounding_max_residual_m, 0.0001
		),
		"runtime_effect_count": runtime_effects.get("effects", []).size(),
		"foliage_instances": foliage_instances,
		"crag_instances": crag_instances,
		"placement_authority": "julia_rust_certified",
		"certified_landform_placement_count": (
			placement_plan.get("placements", []) as Array
		).size(),
		"placement_solver": placement_plan.get("solver", {}),
		"multimesh_batch_count": multimesh_evidence["batch_count"],
		"multimesh_instance_count": multimesh_evidence["instance_count"],
		"multimesh_serialized_instance_count": multimesh_evidence["serialized_instance_count"],
		"multimesh_buffer_float_count": multimesh_evidence["buffer_float_count"],
		"multimesh_expected_buffer_float_count": multimesh_evidence["expected_buffer_float_count"],
		"multimesh_invalid_buffer_batch_count": multimesh_evidence["invalid_buffer_batch_count"],
		"multimesh_spatial_chunk_batch_count": multimesh_evidence["spatial_chunk_batch_count"],
		"multimesh_chunk_size_m": multimesh_evidence["chunk_size_m"],
		"multimesh_max_instances_per_chunk": multimesh_evidence["max_instances_per_chunk"],
		"multimesh_lod_instance_counts": multimesh_evidence["lod_instance_counts"],
		"multimesh_buffers_valid": (
			int(multimesh_evidence["batch_count"]) > 0
			and int(multimesh_evidence["invalid_buffer_batch_count"]) == 0
			and int(multimesh_evidence["serialized_instance_count"])
				== int(multimesh_evidence["instance_count"])
		),
		"multimesh_mesh_resources_external": true,
		"multimesh_mesh_resource_count": _multimesh_resource_paths.size(),
		"multimesh_mesh_resource_bytes": _multimesh_resource_paths.keys().reduce(
			func(total: int, resource_path: String) -> int:
				return total + FileAccess.get_file_as_bytes(resource_path).size(),
			0
		),
		"terrain_mesh_external": (
			terrain.mesh != null
			and terrain.mesh.resource_path == terrain_resource_path
		),
		"terrain_mesh_resource_path": terrain_resource_path,
		"terrain_mesh_resource_bytes": FileAccess.get_file_as_bytes(
			terrain_resource_path
		).size(),
		"terrain_collision_external": collision_evidence.get("external", false),
		"terrain_collision_sample_count": collision_evidence.get("sample_count", 0),
		"terrain_collision_resource_path": collision_evidence.get("resource_path", ""),
			"terrain_collision_resource_bytes": collision_evidence.get("resource_bytes", 0),
			"terrain_material_requested_layer_count": terrain_material_evidence["requested_layer_count"],
			"terrain_material_bound_layer_count": terrain_material_evidence["bound_layer_count"],
			"terrain_material_pbr_layer_count": terrain_material_evidence["pbr_layer_count"],
			"terrain_material_minimum_texels_per_meter": terrain_material_evidence["minimum_texels_per_meter"],
			"terrain_material_meters_per_repeat": terrain_material_evidence["meters_per_repeat"],
			"terrain_wetland_mask_bound": terrain_material_evidence["wetland_mask_bound"],
			"scene_bytes": FileAccess.get_file_as_bytes(output_scene_path).size(),
		"asset_assignment_count": asset_plan.get("assignments", []).size(),
		"missing_assets": missing_assets
	}
	var report_file := FileAccess.open(build_report_path, FileAccess.WRITE)
	if report_file == null:
		printerr("ZoneSpec scene compiler could not write build report")
		quit(1)
		return
	report_file.store_string(JSON.stringify(build_report, "\t") + "\n")
	print("Rebuilt candidate scene %s with %d selected environment assets" % [output_scene_path, foliage_instances + crag_instances + settlement_count + bridge_count])
	scene_root.free()
	quit(0)
