@tool
extends SceneTree

func _init() -> void:
	print("==========================================")
	print("🛠️ SAVING GENERATED NODES INTO MOBA_3D.TSCN...")
	print("==========================================")
	
	var scene_path = "res://moba_3d.tscn"
	var spec_path = "res://assets/generated/map_layout_spec.json"
	
	if not FileAccess.file_exists(spec_path):
		printerr("❌ Spec file missing")
		quit(1)
		return
		
	var file = FileAccess.open(spec_path, FileAccess.READ)
	var json_str = file.get_as_text()
	file.close()
	
	var json = JSON.new()
	json.parse(json_str)
	var spec = json.data
	
	var packed_scene = load(scene_path) as PackedScene
	if not packed_scene:
		printerr("❌ Could not load scene: ", scene_path)
		quit(1)
		return
		
	var root = packed_scene.instantiate()
	var env_node = root.get_node_or_null("Environment3D")
	if not env_node:
		env_node = Node3D.new()
		env_node.name = "Environment3D"
		root.add_child(env_node)
		env_node.owner = root

	var lanes_container = env_node.get_node_or_null("LanesContainer")
	if not lanes_container:
		lanes_container = Node3D.new()
		lanes_container.name = "LanesContainer"
		env_node.add_child(lanes_container)
		lanes_container.owner = root

	var lanes_dict = spec.get("lanes", {})
	for lane_name in lanes_dict:
		var pts = lanes_dict[lane_name]
		var node_name = lane_name.capitalize() + "Path"
		var path_node = lanes_container.get_node_or_null(node_name) as Path3D
		if not path_node:
			path_node = Path3D.new()
			path_node.name = node_name
			lanes_container.add_child(path_node)
			path_node.owner = root
			
		var curve = Curve3D.new()
		for pt in pts:
			curve.add_point(Vector3(pt[0], 1.2, pt[1]))
		path_node.curve = curve

	var new_packed = PackedScene.new()
	new_packed.pack(root)
	ResourceSaver.save(new_packed, scene_path)
	print("✅ Successfully packed and saved updated moba_3d.tscn scene file!")
	print("==========================================")
	quit(0)
