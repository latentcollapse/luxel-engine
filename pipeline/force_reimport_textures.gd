@tool
extends SceneTree

func _init() -> void:
	print("==========================================")
	print("🔄 FORCING REIMPORT OF GENERATED TERRAIN TEXTURES...")
	print("==========================================")
	
	var files = [
		"res://assets/generated/heightmap.png",
		"res://assets/generated/splatmap.png",
		"res://assets/generated/normalmap.png",
		"res://assets/generated/road_mask.png"
	]
	
	for f_path in files:
		if FileAccess.file_exists(f_path):
			# Force reload resource without cache
			var res = ResourceLoader.load(f_path, "", ResourceLoader.CACHE_MODE_REPLACE)
			print("  -> Force reloaded: ", f_path, " -> ", res != null)
			
	print("==========================================")
	print("✨ REIMPORT COMPLETE!")
	print("==========================================")
	quit(0)
