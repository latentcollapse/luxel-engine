@tool
extends SceneTree

const ZoneTerrainBuilderScript = preload("res://zone_terrain_builder.gd")

func _init() -> void:
	var viewport := SubViewport.new()
	viewport.size = Vector2i(1280, 720)
	viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	root.add_child(viewport)
	var world := Node3D.new()
	viewport.add_child(world)
	var environment := WorldEnvironment.new()
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.09, 0.12, 0.16)
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color(0.55, 0.63, 0.75)
	env.ambient_light_energy = 0.8
	environment.environment = env
	world.add_child(environment)
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-52.0, -28.0, 0.0)
	sun.light_energy = 1.6
	sun.shadow_enabled = true
	world.add_child(sun)
	var terrain := ZoneTerrainBuilderScript.build_from_manifest("res://concept_batches/caledonia_v1/terrain/terrain_manifest.json")
	if terrain.mesh == null:
		printerr("ZoneSpec terrain render failed to build a mesh")
		quit(1)
		return
	world.add_child(terrain)
	var camera := Camera3D.new()
	world.add_child(camera)
	camera.position = Vector3(0.0, 1050.0, 1160.0)
	camera.look_at(Vector3(0.0, 95.0, 0.0), Vector3.UP)
	camera.make_current()
	for _frame in range(8):
		await RenderingServer.frame_post_draw
	var image := viewport.get_texture().get_image()
	var output_path := "res://concept_batches/caledonia_v1/terrain/godot_terrain_preview.png"
	if image == null or image.is_empty() or image.save_png(ProjectSettings.globalize_path(output_path)) != OK:
		printerr("ZoneSpec terrain render did not produce a screenshot")
		quit(1)
		return
	print("ZoneSpec Godot terrain preview saved: " + output_path)
	quit(0)
