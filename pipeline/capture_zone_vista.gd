@tool
extends SceneTree

## Perspective evidence capture for terrain/asset review. The overview capture
## checks topology; this view exposes whether mountains are actually 3D.

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

func _camera_pose(zone_spec: Dictionary) -> Array:
	var bounds := zone_spec.get("zone", {}).get("world_bounds", {}) as Dictionary
	var width := maxf(float(bounds.get("width", 2400.0)), 1.0)
	var length := maxf(float(bounds.get("length", 1600.0)), 1.0)
	var span := maxf(width, length)
	return [
		Vector3(-width * 0.43, span * 0.18, length * 0.43),
		Vector3(0.0, span * 0.025, 0.0)
	]

func _init() -> void:
	var scene_path := _scene_path()
	var packed_scene := load(scene_path) as PackedScene
	if packed_scene == null:
		printerr("Cannot capture ZoneSpec scene: " + scene_path)
		quit(1)
		return
	var viewport := SubViewport.new()
	viewport.size = Vector2i(1280, 720)
	viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	root.add_child(viewport)
	var instance := packed_scene.instantiate() as Node3D
	viewport.add_child(instance)
	var camera := Camera3D.new()
	camera.fov = 52.0
	viewport.add_child(camera)
	var batch_path := _batch_path()
	var pose := _camera_pose(_read_json(batch_path.path_join("zone_spec.json")))
	camera.look_at_from_position(pose[0], pose[1], Vector3.UP)
	camera.make_current()
	for _frame in range(8):
		await RenderingServer.frame_post_draw
	var image := viewport.get_texture().get_image()
	var destination := ProjectSettings.globalize_path(batch_path.path_join("terrain/godot_zone_vista.png"))
	if image == null or image.is_empty() or image.save_png(destination) != OK:
		printerr("Zone vista screenshot failed")
		quit(1)
		return
	print("ZoneSpec vista saved: " + destination)
	viewport.free()
	await process_frame
	quit(0)
