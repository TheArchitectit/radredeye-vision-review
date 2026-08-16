# Engine support matrix

| Engine  | Adapter | Status      | Notes |
|---------|---------|-------------|-------|
| Bevy    | Rust    | ✅ functional | `crates/radredeye-bevy`; observer-based screenshot |
| Godot   | GDScript| ✅ addon    | `engines/godot/addons/radredeye_capture` |
| Unity   | C#      | ✅ stubbed  | Phase 4 — `crates/radredeye-unity` re-exports core types; C# side POSTs to daemon bridge |
| Unreal  | C++     | ✅ stubbed  | Phase 4 — `crates/radredeye-unreal` re-exports core types; C++ side POSTs to daemon bridge |
| WebXR   | JS/WASM | ✅ stubbed  | Phase 4 — `crates/radredeye-webxr` canvas capture via `wasm-bindgen` (excluded from default build) |

## Sinks

| Sink | Feature Flag | Status | Notes |
|------|-------------|--------|-------|
| StdoutSink | *(always available)* | ✅ EPIPE-safe | Prints metadata to stdout, ignores BrokenPipe |
| FileSink | `file-sink` (default) | ✅ functional | Writes PNG files to `out/` directory |
| HttpSink | `http-sink` (default) | ✅ functional | POSTs PNG to `RADREDEYE_HTTP_SINK_URL` |
| WebSocketSink | `websocket-sink` | ✅ functional | Connects via `tungstenite`, auto-reconnect |
| GrpcSink | `grpc-sink` | ✅ functional | Client-streaming RPC via `tonic` |

## Bridges

| Component | Status | Notes |
|-----------|--------|-------|
| radredeye-mcp | ✅ implemented | Receives Godot PNGs via HTTP and runs core sinks |
| Proto schema | ✅ defined | `proto/radredeye.proto` — `FrameStreaming` service |
