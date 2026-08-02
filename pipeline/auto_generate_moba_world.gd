@tool
extends SceneTree

func set_owner_recursive(node: Node, owner_node: Node) -> void:
	for child in node.get_children():
		child.owner = owner_node
		set_owner_recursive(child, owner_node)

func _init() -> void:
	print("==========================================")
	print("🏔️ BUILDING SHARP-FACETED SWISS ALP MOUNTAIN CHAIN...")
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

	# 1. 500x500 Gentle Highland Terrain Floor
	var terrain_node = env_node.get_node_or_null("GeneratedTerrain") as MeshInstance3D
	if not terrain_node:
		terrain_node = MeshInstance3D.new()
		terrain_node.name = "GeneratedTerrain"
		env_node.add_child(terrain_node)

	var plane = PlaneMesh.new()
	plane.size = Vector2(500, 500)
	plane.subdivide_width = 240
	plane.subdivide_depth = 240
	terrain_node.mesh = plane

	# 2. Shader for Gentle Highland Basin Floor
	var mat = ShaderMaterial.new()
	var shader = Shader.new()
	shader.code = """
	shader_type spatial;
	uniform sampler2D heightmap_tex;
	uniform sampler2D splatmap_tex;
	uniform sampler2D normalmap_tex;
	
	uniform float height_scale = 16.0;

	varying vec3 w_normal;
	varying vec2 uv_var;

	void vertex() {
		float h = texture(heightmap_tex, UV).r;
		vec4 splat = texture(splatmap_tex, UV);
		
		float interior_h = mix(h * 4.0, 0.4, clamp(splat.g * 2.0, 0.0, 1.0));
		float final_h = mix(interior_h, h * height_scale, 0.3);

		VERTEX.y += final_h;
		w_normal = (MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz;
		uv_var = UV;
	}

	void fragment() {
		vec4 splat = texture(splatmap_tex, uv_var);
		vec3 baked_norm = texture(normalmap_tex, uv_var).rgb * 2.0 - 1.0;

		vec3 grass_col = vec3(0.44, 0.45, 0.22); // Warm Yellowish Highland Grass
		vec3 road_col  = vec3(0.58, 0.50, 0.38); // Warm Dirt Pathway
		vec3 rock_col  = vec3(0.32, 0.32, 0.32); // Slate Grey Granite
		vec3 snow_col  = vec3(0.94, 0.95, 0.98); // Snow Peak

		vec3 col = grass_col * splat.r + road_col * splat.g + rock_col * splat.b + snow_col * splat.a;
		float total_w = splat.r + splat.g + splat.b + splat.a + 1e-5;
		col /= total_w;

		ALBEDO = col;
		NORMAL_MAP = baked_norm;
		ROUGHNESS = 0.85;
	}
	"""
	mat.shader = shader

	var h_tex = load("res://assets/generated/heightmap.png")
	var s_tex = load("res://assets/generated/splatmap.png")
	var n_tex = load("res://assets/generated/normalmap.png")

	if h_tex: mat.set_shader_parameter("heightmap_tex", h_tex)
	if s_tex: mat.set_shader_parameter("splatmap_tex", s_tex)
	if n_tex: mat.set_shader_parameter("normalmap_tex", n_tex)

	terrain_node.material_override = mat

	# 3. Rescale Lane Curves
	var lanes_container = env_node.get_node_or_null("LanesContainer")
	if not lanes_container:
		lanes_container = Node3D.new()
		lanes_container.name = "LanesContainer"
		env_node.add_child(lanes_container)

	var scale_f = 0.30
	var lanes_dict = spec.get("lanes", {})
	for lane_name in lanes_dict:
		var pts = lanes_dict[lane_name]
		var node_name = lane_name.capitalize() + "Path"
		var path_node = lanes_container.get_node_or_null(node_name) as Path3D
		if not path_node:
			path_node = Path3D.new()
			path_node.name = node_name
			lanes_container.add_child(path_node)
			
		var curve = Curve3D.new()
		for pt in pts:
			curve.add_point(Vector3(pt[0] * scale_f, 0.5, pt[1] * scale_f))
		path_node.curve = curve

	# 4. Construct Outer Perimeter 3D Sharp-Faceted Alpine Mountain Massif Chain
	var crags_node = env_node.get_node_or_null("MountainCrags3D")
	if not crags_node:
		crags_node = Node3D.new()
		crags_node.name = "MountainCrags3D"
		env_node.add_child(crags_node)
	else:
		for c in crags_node.get_children():
			c.queue_free()

	# Flat-Faceted PBR Alpine Mountain Shader (Slate Grey Granite, Snow Capping, Yellow Grass Base)
	var mtn_mat = ShaderMaterial.new()
	var mtn_shader = Shader.new()
	mtn_shader.code = """
	shader_type spatial;
	render_mode cull_back;

	varying vec3 w_norm;
	varying vec3 w_pos;

	void vertex() {
		w_norm = (MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz;
		w_pos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	}

	void fragment() {
		vec3 norm = normalize(w_norm);
		float slope = 1.0 - clamp(norm.y, 0.0, 1.0);

		vec3 grass_col  = vec3(0.44, 0.45, 0.22); // Warm Yellowish Highland Grass
		vec3 slate_rock = vec3(0.32, 0.32, 0.32); // Slate Grey Granite
		vec3 snow_peak  = vec3(0.94, 0.95, 0.98); // Pure Snow Peak

		float rock_blend = smoothstep(0.18, 0.45, slope);
		float snow_blend = smoothstep(36.0, 58.0, w_pos.y);

		vec3 col = mix(grass_col, slate_rock, rock_blend);
		col = mix(col, snow_peak, snow_blend);

		ALBEDO = col;
		ROUGHNESS = mix(0.85, 0.45, rock_blend);
	}
	"""
	mtn_mat.shader = mtn_shader

	# Load Sharp-Faceted 3D Mountain Massif Mesh Assets
	var mtn_asset_paths = [
		"res://assets/generated/mountains/massif_broad.res",
		"res://assets/generated/mountains/massif_jagged.res",
		"res://assets/generated/mountains/massif_peak.res",
		"res://assets/generated/mountains/massif_foothill.res"
	]
	
	var loaded_massifs = []
	for p in mtn_asset_paths:
		if ResourceLoader.exists(p):
			var m = load(p) as ArrayMesh
			if m: loaded_massifs.append(m)

	if loaded_massifs.size() > 0:
		var rng = RandomNumberGenerator.new()
		rng.seed = 2026
		
		# Build 24 Overlapping 3D Mountain Massif Instances along outer boundary (Radius 240 - 265)
		var total_mountains = 24
		for i in range(total_mountains):
			var angle = (float(i) / float(total_mountains)) * TAU
			angle += rng.randf_range(-0.04, 0.04)
			
			var radius = rng.randf_range(240.0, 265.0) # Outer boundary ring (outside arena floor)
			var pos_x = cos(angle) * radius
			var pos_z = sin(angle) * radius
			
			var m_mesh = loaded_massifs[rng.randi() % loaded_massifs.size()] as ArrayMesh
			var inst = MeshInstance3D.new()
			inst.mesh = m_mesh
			inst.material_override = mtn_mat

			# Position & Scale: Massive broad-shouldered 3D mountain massifs with sharp peaks
			inst.position = Vector3(pos_x, -4.0, pos_z)
			var scale_x = rng.randf_range(1.4, 2.0)
			var scale_y = rng.randf_range(1.1, 1.6)
			var scale_z = rng.randf_range(1.4, 2.0)
			inst.scale = Vector3(scale_x, scale_y, scale_z)
			inst.rotation.y = rng.randf_range(0, TAU)

			crags_node.add_child(inst)

	set_owner_recursive(root, root)

	var new_packed = PackedScene.new()
	var pack_result = new_packed.pack(root)
	if pack_result == OK:
		ResourceSaver.save(new_packed, scene_path)
		print("==========================================")
		print("✨ SHARP-FACETED 3D SWISS ALP MOUNTAIN CHAIN SAVED!")
		print("==========================================")

	quit(0)
