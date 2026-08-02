@tool
extends SceneTree

func _init() -> void:
	var s = Terrain3DStorage.new()
	print("Terrain3DStorage created: ", s)
	for m in s.get_method_list():
		if "import" in m.name or "height" in m.name or "data" in m.name:
			print(" - ", m.name)
	quit(0)
