@tool
extends Node

## Auto-captures viewport frames for the radredeye pipeline.

@export var enabled: bool = true
@export var save_to_disk: bool = true
@export var capture_interval_seconds: float = 1.0
@export var output_directory: String = "user://screenshots"

## If true, POST each captured PNG to `bridge_url` via an HTTPRequest child.
@export var emit_to_bridge: bool = false
@export var bridge_url: String = "http://127.0.0.1:8765/capture"

var _timer: Timer
var _capture_count: int = 0
var _http: HTTPRequest

func _ready() -> void:
	if not enabled:
		return
	var dir = DirAccess.open("user://")
	if dir:
		dir.make_dir_recursive("screenshots")

	_timer = Timer.new()
	_timer.wait_time = capture_interval_seconds
	_timer.autostart = true
	_timer.timeout.connect(_on_timer_timeout)
	add_child(_timer)

	if emit_to_bridge:
		_http = HTTPRequest.new()
		add_child(_http)

	print("[RadredeyeCapture] Auto-capture enabled every ", capture_interval_seconds, "s")

func _on_timer_timeout() -> void:
	_capture_screenshot()

func capture_now() -> String:
	return _capture_screenshot()

func _capture_screenshot() -> String:
	var vp = get_viewport()
	var tex = vp.get_texture()
	var img = tex.get_image()
	if not img:
		push_error("[RadredeyeCapture] Failed to get viewport image")
		return ""

	var saved_path := ""
	if save_to_disk:
		var timestamp = Time.get_datetime_string_from_system().replace(":", "-")
		_capture_count += 1
		var filename = "screenshot_%s_%04d.png" % [timestamp, _capture_count]
		saved_path = output_directory.path_join(filename)
		var err = img.save_png(saved_path)
		if err != OK:
			push_error("[RadredeyeCapture] Failed to save screenshot: " + str(err))
			saved_path = ""
		else:
			print("[RadredeyeCapture] Saved: ", saved_path)

	if emit_to_bridge and _http:
		var png = img.save_png_to_buffer()
		var headers = ["Content-Type: image/png"]
		var err = _http.request(bridge_url, headers, HTTPClient.METHOD_POST, png)
		if err != OK:
			push_error("[RadredeyeCapture] Failed to POST frame: " + str(err))

	return saved_path
