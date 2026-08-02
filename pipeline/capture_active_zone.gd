@tool
extends SceneTree

const ZoneTerrainBuilderScript = preload("res://zone_terrain_builder.gd")

func _batch_path() -> String:
	var arguments := OS.get_cmdline_user_args()
	for index in range(arguments.size() - 1):
		if arguments[index] == "--batch":
			var batch := str(arguments[index + 1]).replace("\\", "/")
			if batch.begins_with("res://"):
				batch = batch.trim_prefix("res://")
			if not batch.is_empty() and not batch.begins_with("/") and not (".." in batch.split("/")):
				return "res://" + batch.trim_suffix("/")
	return "res://concept_batches/caledonia_v1"

func _output_path() -> String:
	return _batch_path().path_join("terrain/godot_active_scene.png")

func _scene_path() -> String:
	var arguments := OS.get_cmdline_user_args()
	for index in range(arguments.size() - 1):
		if arguments[index] == "--scene":
			var requested := str(arguments[index + 1]).replace("\\", "/")
			if requested.begins_with("res://") and requested.ends_with(".tscn") and not (".." in requested.trim_prefix("res://").split("/")):
				return requested
			return ""
	return "res://moba_3d.tscn"

func _read_json(path: String) -> Dictionary:
	var source := FileAccess.open(path, FileAccess.READ)
	if source == null:
		return {}
	var parsed: Variant = JSON.parse_string(source.get_as_text())
	if parsed is Dictionary:
		return parsed as Dictionary
	return {}

func _find_feature_anchor(node: Node, feature_id: String) -> Dictionary:
	if str(node.get_meta("codeweald_feature", "")) == feature_id and node is Node3D:
		var node_3d := node as Node3D
		var anchor: Variant = node.get_meta("codeweald_anchor_world", node_3d.global_position)
		return {"found": true, "world": anchor if anchor is Vector3 else node_3d.global_position}
	for child in node.get_children():
		var result := _find_feature_anchor(child, feature_id)
		if bool(result.get("found", false)):
			return result
	return {"found": false}

func _find_feature_node(node: Node, feature_id: String) -> Node:
	if str(node.get_meta("codeweald_feature", "")) == feature_id:
		return node
	for child in node.get_children():
		var result := _find_feature_node(child, feature_id)
		if result != null:
			return result
	return null

func _normalized_screen(camera: Camera3D, world: Vector3, viewport_size: Vector2i) -> Array:
	var screen := camera.unproject_position(world)
	return [screen.x / float(viewport_size.x), screen.y / float(viewport_size.y)]

func _aabb_corners(bounds: AABB) -> Array[Vector3]:
	var corners: Array[Vector3] = []
	for x_side in [0.0, 1.0]:
		for y_side in [0.0, 1.0]:
			for z_side in [0.0, 1.0]:
				corners.append(
					bounds.position
					+ Vector3(
						bounds.size.x * x_side,
						bounds.size.y * y_side,
						bounds.size.z * z_side
					)
				)
	return corners

func _relative_transform_to(node: Node3D, root_node: Node3D) -> Transform3D:
	if node == root_node:
		return Transform3D.IDENTITY
	var result := node.transform
	var current := node.get_parent()
	while current != null and current != root_node:
		if current is Node3D:
			result = (current as Node3D).transform * result
		current = current.get_parent()
	return result

func _collect_mesh_bounds(
	node: Node,
	root_node: Node3D,
	points: Array[Vector3]
) -> void:
	if node is MeshInstance3D:
		var mesh_instance := node as MeshInstance3D
		if mesh_instance.mesh != null:
			# Imported nested PackedScenes can expose stale per-child global
			# transforms to a tooling SceneTree. Compose the serialized local
			# chain explicitly from the feature root so evidence matches the
			# transform hierarchy the renderer consumes.
			var mesh_world_transform := (
				root_node.global_transform
				* _relative_transform_to(mesh_instance, root_node)
			)
			for corner in _aabb_corners(mesh_instance.get_aabb()):
				points.append(mesh_world_transform * corner)
	for child in node.get_children():
		_collect_mesh_bounds(child, root_node, points)

func _project_node_bounds(
	node: Node,
	camera: Camera3D,
	viewport_size: Vector2i
) -> Dictionary:
	if node == null:
		return {"found": false}
	if not node is Node3D:
		return {"found": false}
	var world_points: Array[Vector3] = []
	_collect_mesh_bounds(node, node as Node3D, world_points)
	if world_points.is_empty():
		return {"found": false}
	var projected: Array = []
	var world_min := Vector3(INF, INF, INF)
	var world_max := Vector3(-INF, -INF, -INF)
	for world_point in world_points:
		projected.append(_normalized_screen(camera, world_point, viewport_size))
		world_min = world_min.min(world_point)
		world_max = world_max.max(world_point)
	var min_x := INF
	var min_y := INF
	var max_x := -INF
	var max_y := -INF
	for point_var in projected:
		var point := point_var as Array
		min_x = minf(min_x, float(point[0]))
		min_y = minf(min_y, float(point[1]))
		max_x = maxf(max_x, float(point[0]))
		max_y = maxf(max_y, float(point[1]))
	return {
		"found": true,
		"bounds_normalized": [min_x, min_y, max_x, max_y],
		"corner_count": projected.size(),
		"world_min": [world_min.x, world_min.y, world_min.z],
		"world_max": [world_max.x, world_max.y, world_max.z],
	}

func _project_feature_polygon(
	feature: Dictionary,
	terrain_sampler: Dictionary,
	camera: Camera3D,
	viewport_size: Vector2i
) -> Dictionary:
	var screen_points: Array = []
	for raw_point_var in feature.get("geometry", {}).get("points", []):
		var raw_point := raw_point_var as Array
		if raw_point.size() < 2:
			continue
		var x := float(raw_point[0])
		var z := float(raw_point[1])
		var y := ZoneTerrainBuilderScript.sample_height(terrain_sampler, x, z)
		screen_points.append(
			_normalized_screen(camera, Vector3(x, y + 1.0, z), viewport_size)
		)
	return {
		"feature_id": feature.get("id", ""),
		"semantic": feature.get("semantic", ""),
		"found": screen_points.size() >= 3,
		"screen_normalized": screen_points,
		"source_points": feature.get("geometry", {}).get("source_points", []),
	}

func _project_lane(path: Path3D, camera: Camera3D, viewport_size: Vector2i, half_width_m: float) -> Dictionary:
	if path.curve == null or path.curve.point_count < 2:
		return {"found": false}
	var length := path.curve.get_baked_length()
	var sample_count := clampi(ceili(length / 75.0) + 1, 3, 32)
	var projected: Array = []
	var half_width_pixels: Array = []
	for index in range(sample_count):
		var distance := length * float(index) / float(sample_count - 1)
		var center_local := path.curve.sample_baked(distance, true)
		var before_local := path.curve.sample_baked(maxf(0.0, distance - 2.0), true)
		var after_local := path.curve.sample_baked(minf(length, distance + 2.0), true)
		var tangent := Vector2(after_local.x - before_local.x, after_local.z - before_local.z).normalized()
		var side := Vector3(-tangent.y, 0.0, tangent.x)
		var center_world := path.to_global(center_local)
		var side_world := path.to_global(center_local + side * half_width_m)
		var center_screen := camera.unproject_position(center_world)
		var side_screen := camera.unproject_position(side_world)
		projected.append([center_screen.x / float(viewport_size.x), center_screen.y / float(viewport_size.y)])
		half_width_pixels.append(center_screen.distance_to(side_screen))
	return {
		"found": true,
		"screen_normalized": projected,
		"half_width_pixels": half_width_pixels,
	}

func _project_landform(feature: Dictionary, terrain_sampler: Dictionary, camera: Camera3D, viewport_size: Vector2i) -> Dictionary:
	var raw_points: Array = feature.get("geometry", {}).get("points", []) as Array
	if raw_points.size() < 3 or terrain_sampler.is_empty():
		return {"found": false}
	var polygon := PackedVector2Array()
	var min_x := INF
	var max_x := -INF
	var min_z := INF
	var max_z := -INF
	for raw_point_var in raw_points:
		var raw_point := raw_point_var as Array
		var point := Vector2(float(raw_point[0]), float(raw_point[1]))
		polygon.append(point)
		min_x = minf(min_x, point.x)
		max_x = maxf(max_x, point.x)
		min_z = minf(min_z, point.y)
		max_z = maxf(max_z, point.y)
	var screen_samples: Array = []
	var relief_pixels: Array = []
	var heights: Array = []
	var grid := 13
	for z_index in range(grid):
		for x_index in range(grid):
			var candidate := Vector2(
				lerpf(min_x, max_x, (float(x_index) + 0.5) / float(grid)),
				lerpf(min_z, max_z, (float(z_index) + 0.5) / float(grid))
			)
			if not Geometry2D.is_point_in_polygon(candidate, polygon):
				continue
			var height := ZoneTerrainBuilderScript.sample_height(terrain_sampler, candidate.x, candidate.y)
			var elevated_screen := camera.unproject_position(Vector3(candidate.x, height, candidate.y))
			var flat_screen := camera.unproject_position(Vector3(candidate.x, 0.0, candidate.y))
			screen_samples.append([
				elevated_screen.x / float(viewport_size.x),
				elevated_screen.y / float(viewport_size.y),
			])
			relief_pixels.append(elevated_screen.distance_to(flat_screen))
			heights.append(height)
	var evidence: Array = feature.get("evidence", []) as Array
	var evidence_region: Array = []
	if not evidence.is_empty():
		evidence_region = (evidence[0] as Dictionary).get("region", []) as Array
	return {
		"found": not screen_samples.is_empty(),
		"feature_id": feature.get("id", ""),
		"semantic": feature.get("semantic", ""),
		"profile": feature.get("generation", {}).get("profile", ""),
		"screen_samples": screen_samples,
		"sample_count": screen_samples.size(),
		"maximum_height_m": heights.max() if not heights.is_empty() else 0.0,
		"maximum_projected_relief_pixels": relief_pixels.max() if not relief_pixels.is_empty() else 0.0,
		"mean_projected_relief_pixels": relief_pixels.reduce(func(total: float, value: float) -> float: return total + value, 0.0) / float(maxi(relief_pixels.size(), 1)),
		"source_evidence_region": evidence_region,
	}

func _evidence_region_for_image(
	feature: Dictionary, image_id: String
) -> Array:
	for evidence_var in feature.get("evidence", []):
		var evidence := evidence_var as Dictionary
		if str(evidence.get("image_id", "")) == image_id:
			return evidence.get("region", []) as Array
	return []

func _projection_report(instance: Node3D, camera: Camera3D, viewport_size: Vector2i, scene_path: String) -> Dictionary:
	var batch_path := _batch_path()
	var zone_spec := _read_json(batch_path.path_join("zone_spec.json"))
	var canonical_map_image_id := str(
		zone_spec.get("zone", {})
			.get("image_reconciliation", {})
			.get("canonical_map_image_id", "")
	)
	# Topology evidence and the beauty render have different jobs. The beauty
	# camera is oblique so vertical assets have readable silhouettes; a second
	# orthographic evidence camera preserves the source's cartographic X/Z
	# coordinates for lane/landmark/landform containment checks.
	var bounds: Dictionary = zone_spec.get("zone", {}).get("world_bounds", {}) as Dictionary
	var evidence_camera := Camera3D.new()
	evidence_camera.name = "ZoneEvidenceProjectionCamera"
	evidence_camera.projection = Camera3D.PROJECTION_ORTHOGONAL
	evidence_camera.keep_aspect = Camera3D.KEEP_HEIGHT
	evidence_camera.size = float(bounds.get("length", 1600.0))
	instance.add_child(evidence_camera)
	evidence_camera.look_at_from_position(
		Vector3(0.0, 3000.0, 0.0),
		Vector3.ZERO,
		Vector3(0.0, 0.0, 1.0)
	)
	evidence_camera.scale.x = -1.0
	evidence_camera.force_update_transform()
	var center := _normalized_screen(evidence_camera, Vector3.ZERO, viewport_size)
	var positive_x := _normalized_screen(evidence_camera, Vector3(100.0, 0.0, 0.0), viewport_size)
	var positive_z := _normalized_screen(evidence_camera, Vector3(0.0, 0.0, 100.0), viewport_size)
	var landmarks: Array = []
	var corridors: Array = []
	var landforms: Array = []
	var biomes: Array = []
	var terrain_sampler := ZoneTerrainBuilderScript.load_height_sampler(
		batch_path.path_join("terrain/terrain_manifest.json")
	)
	for feature_var in zone_spec.get("features", []):
		var feature := feature_var as Dictionary
		var feature_id := str(feature.get("id", ""))
		if str(feature.get("category", "")) == "biome":
			biomes.append(
				_project_feature_polygon(
					feature, terrain_sampler, camera, viewport_size
				)
			)
			continue
		if str(feature.get("category", "")) == "landform":
			var landform_projection := _project_landform(feature, terrain_sampler, camera, viewport_size)
			var landform_evidence_projection := _project_landform(
				feature, terrain_sampler, evidence_camera, viewport_size
			)
			landform_projection["source_evidence_region"] = (
				_evidence_region_for_image(feature, canonical_map_image_id)
			)
			landform_projection["evidence_screen_samples"] = landform_evidence_projection.get("screen_samples", [])
			landforms.append(landform_projection)
			continue
		if str(feature.get("category", "")) == "corridor" and str(feature.get("semantic", "")) == "lane":
			var lane_node := _find_feature_node(instance, feature_id)
			var properties := feature.get("properties", {}) as Dictionary
			var lane_projection := {"found": false}
			if lane_node is Path3D:
				lane_projection = _project_lane(
					lane_node as Path3D,
					camera,
					viewport_size,
					float(properties.get("minimum_width_m", 20.0)) * 0.5
				)
				var lane_evidence_projection := _project_lane(
					lane_node as Path3D,
					evidence_camera,
					viewport_size,
					float(properties.get("minimum_width_m", 20.0)) * 0.5
				)
				lane_projection["evidence_screen_normalized"] = lane_evidence_projection.get("screen_normalized", [])
				lane_projection["evidence_half_width_pixels"] = lane_evidence_projection.get("half_width_pixels", [])
			lane_projection["feature_id"] = feature_id
			lane_projection["lane_id"] = properties.get("lane_id", "")
			lane_projection["source_points"] = feature.get("geometry", {}).get("source_points", [])
			corridors.append(lane_projection)
			continue
		if str(feature.get("category", "")) != "landmark":
			continue
		var anchor := _find_feature_anchor(instance, feature_id)
		var region := _evidence_region_for_image(
			feature, canonical_map_image_id
		)
		var entry := {
			"feature_id": feature_id,
			"semantic": feature.get("semantic", ""),
			"found": bool(anchor.get("found", false)),
			"source_evidence_region": region,
		}
		if bool(anchor.get("found", false)):
			entry["screen_normalized"] = _normalized_screen(camera, anchor["world"] as Vector3, viewport_size)
			entry["evidence_screen_normalized"] = _normalized_screen(
				evidence_camera, anchor["world"] as Vector3, viewport_size
			)
		var landmark_node := _find_feature_node(instance, feature_id)
		var beauty_bounds := _project_node_bounds(
			landmark_node, camera, viewport_size
		)
		var evidence_bounds := _project_node_bounds(
			landmark_node, evidence_camera, viewport_size
		)
		entry["screen_bounds_found"] = bool(
			beauty_bounds.get("found", false)
		)
		entry["screen_bounds_normalized"] = beauty_bounds.get(
			"bounds_normalized", []
		)
		entry["evidence_screen_bounds_normalized"] = evidence_bounds.get(
			"bounds_normalized", []
		)
		entry["world_bounds_min"] = evidence_bounds.get("world_min", [])
		entry["world_bounds_max"] = evidence_bounds.get("world_max", [])
		landmarks.append(entry)
	return {
		"schema_version": "codeweald.godot-overview-projection/v1",
		"zone_id": zone_spec.get("zone", {}).get("id", "unknown"),
		"scene_path": scene_path,
		"viewport_size": [viewport_size.x, viewport_size.y],
		"axis_projection": {
			"positive_x_delta": [float(positive_x[0]) - float(center[0]), float(positive_x[1]) - float(center[1])],
			"positive_z_delta": [float(positive_z[0]) - float(center[0]), float(positive_z[1]) - float(center[1])],
		},
		"landmarks": landmarks,
		"corridors": corridors,
		"landforms": landforms,
		"biomes": biomes,
	}

func _vector_array(value: Vector3) -> Array:
	return [value.x, value.y, value.z]

func _navigation_report(instance: Node3D, scene_path: String) -> Dictionary:
	var zone_spec := _read_json(_batch_path().path_join("zone_spec.json"))
	var region := instance.find_child(
		"ZoneNavigationRegion", true, false
	) as NavigationRegion3D
	var routes: Array = []
	if region == null or region.navigation_mesh == null:
		return {
			"schema_version": "codeweald.godot-navigation-probe/v1",
			"zone_id": zone_spec.get("zone", {}).get("id", "unknown"),
			"scene_path": scene_path,
			"navigation_found": false,
			"navigation_vertex_count": 0,
			"navigation_polygon_count": 0,
			"routes": routes,
		}
	var keep_ids: Array[String] = []
	var objective_ids: Array[String] = []
	for feature_var in zone_spec.get("features", []):
		var feature := feature_var as Dictionary
		if (
			str(feature.get("category", "")) == "landmark"
			and str(feature.get("semantic", "")) == "faction_keep"
		):
			keep_ids.append(str(feature.get("id", "")))
		elif (
			str(feature.get("category", "")) == "landmark"
			and str(feature.get("semantic", "")) == "arcane_ruin"
		):
			objective_ids.append(str(feature.get("id", "")))
	var route_pairs: Array = []
	if keep_ids.size() >= 2:
		route_pairs.append([keep_ids[0], keep_ids[1], "keep_to_keep"])
	for keep_id in keep_ids:
		for objective_id in objective_ids:
			route_pairs.append([keep_id, objective_id, "keep_to_objective"])
	var map_rid := region.get_navigation_map()
	var off_mesh_link_count := 0
	var enabled_off_mesh_link_count := 0
	for link_var in instance.find_children(
		"BridgeNavigationLink", "NavigationLink3D", true, false
	):
		var link := link_var as NavigationLink3D
		if link != null and link.has_meta("codeweald_off_mesh_link"):
			off_mesh_link_count += 1
			if link.enabled:
				enabled_off_mesh_link_count += 1
	for pair_var in route_pairs:
		var pair := pair_var as Array
		var start_anchor := _find_feature_anchor(instance, str(pair[0]))
		var finish_anchor := _find_feature_anchor(instance, str(pair[1]))
		var route := {
			"from_feature_id": pair[0],
			"to_feature_id": pair[1],
			"role": pair[2],
			"found": false,
		}
		if (
			bool(start_anchor.get("found", false))
			and bool(finish_anchor.get("found", false))
			and map_rid.is_valid()
		):
			var requested_start := start_anchor["world"] as Vector3
			var requested_finish := finish_anchor["world"] as Vector3
			var mapped_start := NavigationServer3D.map_get_closest_point(
				map_rid, requested_start
			)
			var mapped_finish := NavigationServer3D.map_get_closest_point(
				map_rid, requested_finish
			)
			var path := NavigationServer3D.map_get_path(
				map_rid, mapped_start, mapped_finish, true
			)
			var path_length := 0.0
			for index in range(path.size() - 1):
				path_length += path[index].distance_to(path[index + 1])
			route["found"] = path.size() >= 2
			route["path_point_count"] = path.size()
			route["path_length_m"] = path_length
			route["straight_line_distance_m"] = requested_start.distance_to(
				requested_finish
			)
			route["start_snap_distance_m"] = requested_start.distance_to(
				mapped_start
			)
			route["finish_snap_distance_m"] = requested_finish.distance_to(
				mapped_finish
			)
			route["mapped_start"] = _vector_array(mapped_start)
			route["mapped_finish"] = _vector_array(mapped_finish)
		routes.append(route)
	return {
		"schema_version": "codeweald.godot-navigation-probe/v1",
		"zone_id": zone_spec.get("zone", {}).get("id", "unknown"),
		"scene_path": scene_path,
		"navigation_found": true,
		"navigation_vertex_count": region.navigation_mesh.vertices.size(),
			"navigation_polygon_count": region.navigation_mesh.get_polygon_count(),
			"off_mesh_link_count": off_mesh_link_count,
			"enabled_off_mesh_link_count": enabled_off_mesh_link_count,
			"routes": routes,
	}

func _init() -> void:
	var scene_path := _scene_path()
	var packed_scene := load(scene_path) as PackedScene
	if packed_scene == null:
		printerr("Cannot capture ZoneSpec scene: " + scene_path)
		quit(1)
		return
	var viewport := SubViewport.new()
	# The authoritative concept overview is 3:2. Matching its aspect ratio keeps
	# the complete 2400 x 1600 zone in frame without scoring empty side bands as
	# if they were part of the map's art direction.
	viewport.size = Vector2i(1200, 800)
	viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	root.add_child(viewport)
	var instance := packed_scene.instantiate() as Node3D
	viewport.add_child(instance)
	var camera := instance.get_node_or_null("ZoneOverviewCamera") as Camera3D
	if camera == null:
		printerr("Active ZoneSpec scene has no overview camera")
		quit(1)
		return
	camera.make_current()
	for _frame in range(4):
		await physics_frame
	for _frame in range(8):
		await RenderingServer.frame_post_draw
	var image := viewport.get_texture().get_image()
	var destination := ProjectSettings.globalize_path(_output_path())
	if image == null or image.is_empty() or image.save_png(destination) != OK:
		printerr("Active ZoneSpec scene screenshot failed")
		quit(1)
		return
	var projection_path := _batch_path().path_join("terrain/godot_overview_projection.json")
	var projection_file := FileAccess.open(projection_path, FileAccess.WRITE)
	if projection_file == null:
		printerr("Cannot write Godot overview projection evidence")
		quit(1)
		return
	projection_file.store_string(JSON.stringify(_projection_report(instance, camera, viewport.size, scene_path), "\t") + "\n")
	var navigation_path := _batch_path().path_join(
		"terrain/godot_navigation_probe.json"
	)
	var navigation_file := FileAccess.open(navigation_path, FileAccess.WRITE)
	if navigation_file == null:
		printerr("Cannot write Godot navigation probe evidence")
		quit(1)
		return
	navigation_file.store_string(
		JSON.stringify(_navigation_report(instance, scene_path), "\t") + "\n"
	)
	print("Active ZoneSpec scene preview saved: " + destination)
	viewport.free()
	await process_frame
	quit(0)
