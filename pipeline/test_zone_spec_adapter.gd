@tool
extends SceneTree

func _init() -> void:
	var definition := CaledoniaMapDefinition.load_from_json("res://concept_batches/caledonia_v1/godot_map_definition.json")
	if definition.map_width != 2400.0 or definition.map_length != 1600.0:
		printerr("ZoneSpec adapter lost world bounds")
		quit(1)
		return
	if definition.landforms.size() != 2:
		printerr("ZoneSpec adapter lost landform records")
		quit(1)
		return
	for landform in definition.landforms:
		var generation: Dictionary = landform.get("generation", {})
		if landform.get("semantic") != "alpine_massif" or generation.get("profile") != "alpine_jagged_massif":
			printerr("ZoneSpec adapter flattened an alpine massif")
			quit(1)
			return
	var navigation := NavigationBuilder.build_navigation_region(definition)
	if navigation.navigation_mesh == null or navigation.navigation_mesh.vertices.is_empty():
		printerr("Navigation build failed for ZoneSpec import")
		quit(1)
		return
	print("ZoneSpec Godot adapter passed: %d landforms, %d nav vertices" % [definition.landforms.size(), navigation.navigation_mesh.vertices.size()])
	quit(0)
