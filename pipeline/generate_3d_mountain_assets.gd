@tool
extends SceneTree

# Generates 4 sharp, razor-edged, faceted 3D Alpine Mountain Massif Assets (Matterhorn / Kaer Morhen quality)
func _init() -> void:
	print("==========================================")
	print("⛰️ GENERATING SHARP-FACETED 3D ALPINE MOUNTAIN MASSIF ASSETS...")
	print("==========================================")
	
	DirAccess.make_dir_recursive_absolute("res://assets/generated/mountains")

	# Sharp, razor-edged Swiss Alp massifs with flat granite cliff facets
	_create_sharp_alpine_asset("res://assets/generated/mountains/massif_broad.res", 110.0, 52.0, 110.0, 1)
	_create_sharp_alpine_asset("res://assets/generated/mountains/massif_jagged.res", 120.0, 68.0, 100.0, 2)
	_create_sharp_alpine_asset("res://assets/generated/mountains/massif_peak.res", 95.0, 85.0, 95.0, 3)
	_create_sharp_alpine_asset("res://assets/generated/mountains/massif_foothill.res", 105.0, 32.0, 105.0, 4)

	print("==========================================")
	print("✨ ALL 4 SHARP-FACETED 3D MOUNTAIN ASSETS GENERATED!")
	print("==========================================")
	quit(0)

func _create_sharp_alpine_asset(out_path: String, width: float, height: float, depth: float, preset_type: int) -> void:
	var st = SurfaceTool.new()
	st.begin(Mesh.PRIMITIVE_TRIANGLES)

	var seg_x = 28
	var seg_z = 28

	# Compute height grid with sharp Voronoi ridges & stratified granite cliff terraces
	var grid_y: Array = []
	for z in range(seg_z + 1):
		var row: Array = []
		var f_z = float(z) / float(seg_z)
		var p_z = (f_z - 0.5) * depth

		for x in range(seg_x + 1):
			var f_x = float(x) / float(seg_x)
			var p_x = (f_x - 0.5) * width

			# Pyramidal massif base falloff
			var nx = (f_x - 0.5) * 2.0
			var nz = (f_z - 0.5) * 2.0
			var r_dist = sqrt(nx*nx + nz*nz)
			var base_falloff = clamp(1.0 - r_dist, 0.0, 1.0)
			base_falloff = sin(base_falloff * PI * 0.5)

			# Sharp Voronoi Crease Ridges (1.0 - |sin|) -> Creates razor-sharp mountain crests!
			var r1 = 1.0 - abs(sin(p_x * 0.09) * cos(p_z * 0.09))
			var r2 = 1.0 - abs(cos(p_x * 0.22 - p_z * 0.18))
			var r3 = abs(sin(p_x * 0.45 + p_z * 0.45)) * 0.25

			var peak_h = (base_falloff ** 1.3) * (0.3 + r1 * 0.5 + r2 * 0.35 + r3)
			
			# Stratified Granite Cliff Terracing (sharp horizontal rock ledges)
			var terrace_h = floor(peak_h * 5.0) / 5.0
			var final_factor = lerpf(peak_h, terrace_h, 0.4)
			
			var p_y = final_factor * height
			row.append(p_y)
		grid_y.append(row)

	# Build triangles with UN-SHARED vertices to force crisp, razor-sharp flat face facets!
	for z in range(seg_z):
		var f_z0 = float(z) / float(seg_z)
		var f_z1 = float(z + 1) / float(seg_z)
		var p_z0 = (f_z0 - 0.5) * depth
		var p_z1 = (f_z1 - 0.5) * depth

		for x in range(seg_x):
			var f_x0 = float(x) / float(seg_x)
			var f_x1 = float(x + 1) / float(seg_x)
			var p_x0 = (f_x0 - 0.5) * width
			var p_x1 = (f_x1 - 0.5) * width

			var y00 = grid_y[z][x]
			var y10 = grid_y[z][x + 1]
			var y01 = grid_y[z + 1][x]
			var y11 = grid_y[z + 1][x + 1]

			var v00 = Vector3(p_x0, y00, p_z0)
			var v10 = Vector3(p_x1, y10, p_z0)
			var v01 = Vector3(p_x0, y01, p_z1)
			var v11 = Vector3(p_x1, y11, p_z1)

			# Triangle 1 (v00, v10, v01) - Counter-Clockwise
			var n1 = (v10 - v00).cross(v01 - v00).normalized()
			st.set_normal(n1)
			st.set_uv(Vector2(f_x0, f_z0)); st.add_vertex(v00)
			st.set_normal(n1)
			st.set_uv(Vector2(f_x1, f_z0)); st.add_vertex(v10)
			st.set_normal(n1)
			st.set_uv(Vector2(f_x0, f_z1)); st.add_vertex(v01)

			# Triangle 2 (v10, v11, v01) - Counter-Clockwise
			var n2 = (v11 - v10).cross(v01 - v10).normalized()
			st.set_normal(n2)
			st.set_uv(Vector2(f_x1, f_z0)); st.add_vertex(v10)
			st.set_normal(n2)
			st.set_uv(Vector2(f_x1, f_z1)); st.add_vertex(v11)
			st.set_normal(n2)
			st.set_uv(Vector2(f_x0, f_z1)); st.add_vertex(v01)

	st.generate_tangents()

	var arr_mesh = st.commit()
	ResourceSaver.save(arr_mesh, out_path)
	print("  -> Saved Sharp-Faceted 3D Mountain Asset: ", out_path)
