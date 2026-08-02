@tool
extends SceneTree

func _init() -> void:
	print("📸 CAPTURING PANORAMIC VIEW OF SHARP-FACETED 3D SWISS ALPS...")
	var scene: PackedScene = load("res://moba_3d.tscn")
	if not scene:
		printerr("❌ Could not load moba_3d.tscn")
		quit(1)
		return

	var root_node = scene.instantiate()
	root.add_child(root_node)

	var sub_vp = SubViewport.new()
	sub_vp.size = Vector2i(1280, 720)
	sub_vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	root.add_child(sub_vp)

	var cam = root_node.get_node_or_null("CameraPivot/Camera3D") as Camera3D
	if cam:
		cam.owner = null
		cam.reparent(sub_vp)
		# Panoramic camera position showing valley floor & sharp-faceted Alpine massif walls
		cam.position = Vector3(0, 45, 120)
		cam.rotation_degrees = Vector3(-12, -25, 0)
		cam.make_current()

	await process_frame
	await process_frame

	var img = sub_vp.get_texture().get_image()
	var out_path = "/home/matt/.gemini/antigravity-cli/brain/c5b9ae66-de16-4863-b6a1-24dbf8a65761/current_view.png"
	if img:
		img.save_png(out_path)
		print("✅ Panoramic Sharp 3D Alp View saved to: ", out_path)

	quit(0)
