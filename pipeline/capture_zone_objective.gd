@tool
extends SceneTree

## Close evidence capture for the semantic central landmark. The broad overview
## cannot prove that a generated objective kit has a readable silhouette.

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

func _read_json(path: String) -> Dictionary:
	var source := FileAccess.open(path, FileAccess.READ)
	if source == null:
		return {}
	var parsed: Variant = JSON.parse_string(source.get_as_text())
	if parsed is Dictionary:
		return parsed as Dictionary
	return {}

func _scene_path() -> String:
	var arguments := OS.get_cmdline_user_args()
	for index in range(arguments.size() - 1):
		if arguments[index] == "--scene":
			var requested := str(arguments[index + 1]).replace("\\", "/")
			if requested.begins_with("res://") and requested.ends_with(".tscn") and not (".." in requested.trim_prefix("res://").split("/")):
				return requested
			return ""
	return "res://moba_3d.tscn"

func _feature_anchor(node: Node, feature_id: String) -> Dictionary:
	if str(node.get_meta("codeweald_feature", "")) == feature_id and node is Node3D:
		var node_3d := node as Node3D
		var authored_anchor: Variant = node.get_meta(
			"codeweald_anchor_world", node_3d.global_position
		)
		return {
			"found": true,
			"position": (
				authored_anchor
				if authored_anchor is Vector3
				else node_3d.global_position
			),
		}
	for child in node.get_children():
		var result := _feature_anchor(child, feature_id)
		if bool(result.get("found", false)):
			return result
	return {"found": false}

func _world_span(zone_spec: Dictionary) -> float:
	var bounds := zone_spec.get("zone", {}).get("world_bounds", {}) as Dictionary
	return maxf(
		float(bounds.get("width", 256.0)),
		float(bounds.get("length", 256.0))
	)

func _init() -> void:
	var scene_path := _scene_path()
	var packed_scene := load(scene_path) as PackedScene
	if packed_scene == null:
		printerr("Cannot capture ZoneSpec scene: " + scene_path)
		quit(1)
		return
	var viewport := SubViewport.new()
	viewport.size = Vector2i(1024, 768)
	viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	root.add_child(viewport)
	var instance := packed_scene.instantiate() as Node3D
	viewport.add_child(instance)
	var camera := Camera3D.new()
	camera.fov = 50.0
	viewport.add_child(camera)
	var batch_path := _batch_path()
	var zone_spec := _read_json(batch_path.path_join("zone_spec.json"))
	var anchor := _feature_anchor(instance, "central_ruin")
	var target := (
		(anchor.get("position", Vector3.ZERO) as Vector3) + Vector3(0.0, 3.0, 0.0)
		if bool(anchor.get("found", false))
		else Vector3.ZERO
	)
	var span := _world_span(zone_spec)
	var horizontal_offset := clampf(span * 0.12, 20.0, 50.0)
	var vertical_offset := clampf(span * 0.06, 10.0, 24.0)
	camera.look_at_from_position(
		target + Vector3(-horizontal_offset, vertical_offset, horizontal_offset),
		target,
		Vector3.UP
	)
	camera.make_current()
	for _frame in range(10):
		await RenderingServer.frame_post_draw
	var image := viewport.get_texture().get_image()
	var destination := ProjectSettings.globalize_path(batch_path.path_join("terrain/godot_objective.png"))
	if image == null or image.is_empty() or image.save_png(destination) != OK:
		printerr("Zone objective screenshot failed")
		quit(1)
		return
	print("ZoneSpec objective saved: " + destination)
	viewport.free()
	await process_frame
	quit(0)
