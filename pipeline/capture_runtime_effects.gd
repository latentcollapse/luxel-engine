@tool
extends SceneTree

## Captures and compares two runtime frames from a freshly loaded candidate.
## This proves serialized autoplay/shader effects survive compilation and tick.

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

func _scene_path() -> String:
	var arguments := OS.get_cmdline_user_args()
	for index in range(arguments.size() - 1):
		if arguments[index] == "--scene":
			var requested := str(arguments[index + 1]).replace("\\", "/")
			if requested.begins_with("res://") and requested.ends_with(".tscn") and not (".." in requested.trim_prefix("res://").split("/")):
				return requested
			return ""
	return "res://moba_3d.tscn"

func _save(image: Image, path: String) -> bool:
	return image != null and not image.is_empty() and image.save_png(ProjectSettings.globalize_path(path)) == OK

func _read_effect_count(batch_path: String) -> int:
	var source := FileAccess.open(batch_path.path_join("runtime_effects.json"), FileAccess.READ)
	if source == null:
		return 0
	var parsed: Variant = JSON.parse_string(source.get_as_text())
	if parsed is Dictionary:
		return ((parsed as Dictionary).get("effects", []) as Array).size()
	return 0

func _read_json(path: String) -> Dictionary:
	var source := FileAccess.open(path, FileAccess.READ)
	if source == null:
		return {}
	var parsed: Variant = JSON.parse_string(source.get_as_text())
	return parsed as Dictionary if parsed is Dictionary else {}

func _camera_pose(zone_spec: Dictionary) -> Array:
	var bounds := zone_spec.get("zone", {}).get("world_bounds", {}) as Dictionary
	var width := maxf(float(bounds.get("width", 256.0)), 1.0)
	var length := maxf(float(bounds.get("length", 256.0)), 1.0)
	var span := maxf(width, length)
	return [
		Vector3(-width * 0.35, span * 0.16, length * 0.35),
		Vector3(0.0, span * 0.025, 0.0)
	]

func _frame_delta(first: Image, second: Image) -> Dictionary:
	if first == null or second == null or first.get_size() != second.get_size():
		return {"mean_absolute_rgb_delta": 0.0, "changed_pixel_count": 0, "changed_pixel_fraction": 0.0}
	var total_delta := 0.0
	var changed_pixels := 0
	var size := first.get_size()
	for y in range(size.y):
		for x in range(size.x):
			var first_pixel := first.get_pixel(x, y)
			var second_pixel := second.get_pixel(x, y)
			var delta := (
				absf(first_pixel.r - second_pixel.r)
				+ absf(first_pixel.g - second_pixel.g)
				+ absf(first_pixel.b - second_pixel.b)
			) / 3.0
			total_delta += delta
			if delta >= 0.002:
				changed_pixels += 1
	var pixel_count := size.x * size.y
	return {
		"mean_absolute_rgb_delta": total_delta / float(maxi(pixel_count, 1)),
		"changed_pixel_count": changed_pixels,
		"changed_pixel_fraction": float(changed_pixels) / float(maxi(pixel_count, 1)),
	}

func _init() -> void:
	var batch_path := _batch_path()
	var scene_path := _scene_path()
	var packed_scene := load(scene_path) as PackedScene
	if packed_scene == null:
		printerr("Cannot capture runtime effects from ZoneSpec scene: " + scene_path)
		quit(1)
		return
	var viewport := SubViewport.new()
	viewport.size = Vector2i(640, 480)
	viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	root.add_child(viewport)
	viewport.add_child(packed_scene.instantiate())
	var camera := Camera3D.new()
	camera.fov = 50.0
	viewport.add_child(camera)
	var pose := _camera_pose(
		_read_json(batch_path.path_join("zone_spec.json"))
	)
	camera.look_at_from_position(pose[0], pose[1], Vector3.UP)
	camera.make_current()
	for _frame in range(6):
		await RenderingServer.frame_post_draw
	var first := viewport.get_texture().get_image()
	var first_output := batch_path.path_join("terrain/runtime_frame_01.png")
	if not _save(first, first_output):
		printerr("First runtime capture failed")
		quit(1)
		return
	for _frame in range(46):
		await RenderingServer.frame_post_draw
	var second := viewport.get_texture().get_image()
	var second_output := batch_path.path_join("terrain/runtime_frame_02.png")
	if not _save(second, second_output):
		printerr("Second runtime capture failed")
		quit(1)
		return
	var delta := _frame_delta(first, second)
	var effect_count := _read_effect_count(batch_path)
	var passed := (
		effect_count > 0
		and int(delta["changed_pixel_count"]) >= 64
		and float(delta["mean_absolute_rgb_delta"]) >= 0.00001
	)
	var report := {
		"schema_version": "codeweald.godot-runtime-effects/v1",
		"scene_path": scene_path,
		"effect_count": effect_count,
		"frame_gap": 46,
		"mean_absolute_rgb_delta": delta["mean_absolute_rgb_delta"],
		"changed_pixel_count": delta["changed_pixel_count"],
		"changed_pixel_fraction": delta["changed_pixel_fraction"],
		"status": "passed" if passed else "failed",
	}
	var report_file := FileAccess.open(
		batch_path.path_join("terrain/runtime_effects_acceptance_report.json"),
		FileAccess.WRITE
	)
	if report_file == null:
		printerr("Runtime-effect report could not be written")
		quit(1)
		return
	report_file.store_string(JSON.stringify(report, "\t") + "\n")
	print(
		"ZoneSpec runtime effect frames saved with %.6f mean delta"
		% float(delta["mean_absolute_rgb_delta"])
	)
	viewport.free()
	await process_frame
	# A red quality report is valid evidence, not a process failure. The Python
	# orchestrator reads status and decides whether the candidate may promote.
	quit(0)
