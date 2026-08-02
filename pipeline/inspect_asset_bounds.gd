@tool
extends SceneTree

const PATHS := [
	"res://assets/models/kenney_nature/tree_pineDefaultA.glb",
	"res://assets/models/kenney_nature/tree_pineRoundA.glb",
	"res://assets/generated/codeweald_alpine/alpine_cliff_ridge_a.glb",
	"res://assets/models/kenney_nature/cliff_blockCave_rock.glb",
]

func _bounds(node: Node3D, inherited: Transform3D) -> AABB:
	var transform := inherited * node.transform
	var result := AABB()
	var has_bounds := false
	if node is MeshInstance3D and (node as MeshInstance3D).mesh != null:
		var local := (node as MeshInstance3D).mesh.get_aabb()
		var scale := transform.basis.get_scale().abs()
		result = AABB(transform * local.get_center() - local.size * scale * 0.5, local.size * scale)
		has_bounds = true
	for child in node.get_children():
		if child is Node3D:
			var child_bounds := _bounds(child as Node3D, transform)
			if child_bounds.size.length_squared() > 0.0:
				result = child_bounds if not has_bounds else result.merge(child_bounds)
				has_bounds = true
	return result

func _init() -> void:
	for path in PATHS:
		var packed := load(path) as PackedScene
		if packed == null:
			printerr("Cannot load " + path)
			continue
		var instance := packed.instantiate() as Node3D
		var bounds := _bounds(instance, Transform3D.IDENTITY)
		print("ASSET_BOUNDS %s size=%s position=%s" % [path, bounds.size, bounds.position])
	quit(0)
