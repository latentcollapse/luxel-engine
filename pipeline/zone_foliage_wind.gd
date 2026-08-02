extends Node3D

## Runtime component applied by the generated scene to individually placed
## foliage roots. It keeps authored import transforms intact and sways around
## each object's local base rather than rotating a whole forest as one block.

@export var sway_degrees := 3.0
@export var gust_period_s := 7.0
@export var wind_phase := 0.0

var _base_rotation := Vector3.ZERO

func _ready() -> void:
	_base_rotation = rotation

func _process(_delta: float) -> void:
	if Engine.is_editor_hint():
		return
	var frequency := TAU / maxf(gust_period_s, 0.1)
	var wave := sin(Time.get_ticks_msec() * 0.001 * frequency + wind_phase)
	rotation = _base_rotation
	rotation.x += deg_to_rad(sway_degrees) * wave
	rotation.z += deg_to_rad(sway_degrees * 0.55) * cos(Time.get_ticks_msec() * 0.001 * frequency * 1.17 + wind_phase)
