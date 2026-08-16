//! Stateless MCP (Model Context Protocol) server for radredeye.
//!
//! radredeye exposes its capture pipeline to AI agents over the MCP Streamable
//! HTTP transport in **stateless** mode: each request is independent, no
//! `Mcp-Session-Id` is issued or required, and `initialize` is answered but not
//! enforced. This is the "new mcp stateless protocol" that replaces the old
//! bespoke `/capture` `/frame` HTTP bridge as the agent-facing interface.
//!
//! The server wraps a shared [`CapturePipeline`] plus a global [`ReplayBuffer`]
//! (the canonical frame store) and answers JSON-RPC 2.0 requests at the `/mcp`
//! endpoint. Tools let an agent list streams/sinks, pull the latest captured
//! frame as a PNG image, push a frame in from any app framework, and check
//! liveness.
//!
//! # Tools
//!
//! - `list_streams` — enumerate capture streams + their sink counts.
//! - `list_sinks`   — enumerate the sink kinds attached to a stream.
//! - `get_frame`    — return the Nth-newest captured frame as a PNG image
//!   (`index = 0` = latest).
//! - `submit_frame` — push a base64 PNG frame into a stream. This is the
//!   universal capture path; *any* app framework (not just a game engine) can
//!   submit frames here.
//! - `health`       — liveness + pipeline stats.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use radredeye_core::{CapturePipeline, CapturedFrame, PixelFormat, ReplayBuffer};

use crate::{decode_png, encode_frame_png};

/// MCP protocol version this server speaks (2025-06-18 = current stateless
/// Streamable HTTP revision).
const PROTOCOL_VERSION: &str = "2025-06-18";

/// Outcome of handling one inbound MCP request body.
pub enum McpOutcome {
    /// The request was a notification (no `id`): the server must not return a
    /// JSON-RPC response. The HTTP layer answers with `202` + empty body.
    Notification,
    /// A serialized JSON-RPC response to return with `200` + `application/json`.
    Json(String),
}

/// Stateless MCP server.
///
/// Holds the shared capture pipeline and the global frame store. Both are
/// `Arc`-shared with the legacy HTTP routes (`/capture`, `/frame`, `/health`)
/// so an agent and an app framework observe the exact same frames.
pub struct McpServer {
    pipeline: Arc<Mutex<CapturePipeline>>,
    frame_store: Arc<ReplayBuffer>,
}

impl McpServer {
    /// Build a server around the shared pipeline + frame store.
    pub fn new(pipeline: Arc<Mutex<CapturePipeline>>, frame_store: Arc<ReplayBuffer>) -> Self {
        Self { pipeline, frame_store }
    }

    /// Parse and dispatch a single JSON-RPC request body. Stateless: no session
    /// id is tracked. Requests without an `id` are treated as notifications and
    /// yield [`McpOutcome::Notification`].
    pub fn handle_jsonrpc(&self, body: &[u8]) -> McpOutcome {
        let req: JsonRpcRequest = match serde_json::from_slice(body) {
            Ok(r) => r,
            Err(e) => {
                return McpOutcome::Json(error_response(
                    None,
                    -32700,
                    &format!("parse error: {e}"),
                ))
            }
        };

        // Notifications carry no id and must not receive a JSON-RPC response.
        let id = match req.id {
            Some(id) => id,
            None => return McpOutcome::Notification,
        };

        let method = match req.method {
            Some(ref m) => m.clone(),
            None => return McpOutcome::Json(error_response(Some(id), -32600, "missing method")),
        };

        let result: Result<Value, (i64, String)> = match method.as_str() {
            "initialize" => Ok(initialize_result()),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(self.tools_list()),
            "tools/call" => self.tools_call(req.params),
            "resources/list" => Ok(json!({ "resources": [] })),
            "prompts/list" => Ok(json!({ "prompts": [] })),
            _ => Err((-32601, format!("method not found: {method}"))),
        };

        match result {
            Ok(r) => McpOutcome::Json(ok_response(id, r)),
            Err((code, msg)) => McpOutcome::Json(error_response(Some(id), code, &msg)),
        }
    }

    // ---- tool dispatch ----

    fn tools_call(&self, params: Option<Value>) -> Result<Value, (i64, String)> {
        let params = params.unwrap_or(Value::Null);
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or((-32602, "missing tool name".to_string()))?;
        let args = match params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()))
        {
            Value::Object(m) => m,
            _ => Map::new(),
        };

        let content = match name {
            "list_streams" => self.list_streams()?,
            "list_sinks" => self.list_sinks(&str_arg(&args, "stream", "default"))?,
            "get_frame" => {
                let stream = str_arg(&args, "stream", "default");
                let index = args.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                self.get_frame(&stream, index)?
            }
            "submit_frame" => {
                let png = str_arg_required(&args, "png_base64")?;
                let stream = str_arg(&args, "stream", "default");
                self.submit_frame(&png, &stream)?
            }
            "health" => self.health()?,
            other => return Err((-32601, format!("unknown tool: {other}"))),
        };

        Ok(json!({ "content": content, "isError": false }))
    }

    fn list_streams(&self) -> Result<Value, (i64, String)> {
        let p = self
            .pipeline
            .lock()
            .map_err(|e| (-32603, format!("pipeline lock poisoned: {e}")))?;
        let streams: Vec<Value> = p
            .stream_ids()
            .iter()
            .map(|id| {
                json!({
                    "id": id,
                    "sink_count": p.stream_sink_count(id),
                    "has_replay": p.stream_has_replay(id),
                })
            })
            .collect();
        drop(p);
        Ok(json!([text_content(
            &serde_json::to_string_pretty(&streams).unwrap_or_default()
        )]))
    }

    fn list_sinks(&self, stream: &str) -> Result<Value, (i64, String)> {
        let p = self
            .pipeline
            .lock()
            .map_err(|e| (-32603, format!("pipeline lock poisoned: {e}")))?;
        let kinds: Vec<&'static str> = if stream == "default" {
            p.sink_kinds()
        } else {
            p.stream(stream).map(|s| s.sink_kinds()).unwrap_or_default()
        };
        drop(p);
        Ok(json!([text_content(
            &serde_json::to_string_pretty(&kinds).unwrap_or_default()
        )]))
    }

    fn get_frame(&self, stream: &str, index: usize) -> Result<Value, (i64, String)> {
        // Resolve the frame under the pipeline lock, then encode outside it.
        let frame = {
            let p = self
                .pipeline
                .lock()
                .map_err(|e| (-32603, format!("pipeline lock poisoned: {e}")))?;
            match p.stream(stream) {
                Some(s) => s.replay().and_then(|buf| buf.get(index)),
                None => None,
            }
        };
        let frame = frame.or_else(|| self.frame_store.get(index));
        match frame {
            Some(f) => {
                let png = encode_frame_png(&f).map_err(|e| (-32603, format!("png encode: {e}")))?;
                let b64 = B64.encode(&png);
                Ok(json!([{ "type": "image", "data": b64, "mimeType": "image/png" }]))
            }
            None => Err((-32000, format!("no frame at index {index} on stream '{stream}'"))),
        }
    }

    fn submit_frame(&self, png_base64: &str, stream: &str) -> Result<Value, (i64, String)> {
        let bytes = B64
            .decode(png_base64)
            .map_err(|e| (-32602, format!("invalid base64 png: {e}")))?;
        let (w, h, data) = decode_png(&bytes).map_err(|e| (-32602, format!("invalid png: {e}")))?;
        let frame = CapturedFrame {
            width: w,
            height: h,
            format: PixelFormat::Rgba8,
            data,
            timestamp: Instant::now(),
        };
        {
            let p = self
                .pipeline
                .lock()
                .map_err(|e| (-32603, format!("pipeline lock poisoned: {e}")))?;
            p.submit_to(stream, &frame);
        }
        self.frame_store.push(frame);
        Ok(json!([text_content(&format!(
            "submitted frame to stream '{stream}'"
        ))]))
    }

    fn health(&self) -> Result<Value, (i64, String)> {
        let p = self
            .pipeline
            .lock()
            .map_err(|e| (-32603, format!("pipeline lock poisoned: {e}")))?;
        let streams = p.stream_ids();
        let default_sinks = p.sink_count();
        let frame_store_len = self.frame_store.len();
        drop(p);
        let payload = json!({
            "status": "ok",
            "protocol": "mcp-stateless",
            "protocol_version": PROTOCOL_VERSION,
            "streams": streams,
            "default_sinks": default_sinks,
            "frame_store_len": frame_store_len,
        });
        Ok(json!([text_content(&payload.to_string())]))
    }

    fn tools_list(&self) -> Value {
        json!({
            "tools": [
                {
                    "name": "list_streams",
                    "description": "List capture streams and their sink counts.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "list_sinks",
                    "description": "List the sink kinds attached to a stream (default: 'default').",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "stream": { "type": "string", "description": "stream id" }
                        }
                    }
                },
                {
                    "name": "get_frame",
                    "description": "Return the Nth-newest captured frame as a PNG image. index 0 = latest.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "stream": { "type": "string", "description": "stream id" },
                            "index": { "type": "integer", "description": "0 = newest" }
                        }
                    }
                },
                {
                    "name": "submit_frame",
                    "description": "Submit a base64-encoded PNG frame into a stream. Universal capture path for any app framework.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "png_base64": { "type": "string", "description": "base64 PNG" },
                            "stream": { "type": "string", "description": "stream id" }
                        },
                        "required": ["png_base64"]
                    }
                },
                {
                    "name": "health",
                    "description": "Liveness + pipeline stats.",
                    "inputSchema": { "type": "object", "properties": {} }
                }
            ]
        })
    }
}

/// One parsed JSON-RPC 2.0 request. Unknown/extra fields are ignored by serde.
#[derive(Deserialize)]
struct JsonRpcRequest {
    pub id: Option<Value>,
    pub method: Option<String>,
    pub params: Option<Value>,
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "radredeye",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn ok_response(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_response(id: Option<Value>, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
    .to_string()
}

fn text_content(s: &str) -> Value {
    json!({ "type": "text", "text": s })
}

fn str_arg(args: &Map<String, Value>, key: &str, default: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

fn str_arg_required(args: &Map<String, Value>, key: &str) -> Result<String, (i64, String)> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or((-32602, format!("missing required argument: {key}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use radredeye_core::{sinks::StdoutSink, FRAME_STORE_CAP};

    fn server() -> McpServer {
        let pipeline = CapturePipeline::new();
        pipeline.register_sink(Arc::new(StdoutSink));
        let pipeline = Arc::new(Mutex::new(pipeline));
        let frame_store = Arc::new(ReplayBuffer::new(FRAME_STORE_CAP));
        McpServer::new(pipeline, frame_store)
    }

    /// Call the server and parse the JSON-RPC response (panics on notification).
    fn call(srv: &McpServer, body: &str) -> Value {
        match srv.handle_jsonrpc(body.as_bytes()) {
            McpOutcome::Json(s) => serde_json::from_str(&s).expect("valid JSON-RPC response"),
            McpOutcome::Notification => panic!("expected a response, got a notification"),
        }
    }

    /// A minimal valid 1×1 red PNG in memory.
    fn tiny_png() -> Vec<u8> {
        use image::codecs::png::PngEncoder;
        use image::ImageEncoder;
        let rgba = vec![255u8, 0, 0, 255];
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(&rgba, 1, 1, image::ExtendedColorType::Rgba8)
            .expect("png encode");
        buf
    }

    #[test]
    fn initialize_reports_capabilities() {
        let r = call(
            &server(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        );
        assert_eq!(r["result"]["serverInfo"]["name"].as_str(), Some("radredeye"));
        assert_eq!(r["result"]["capabilities"]["tools"], json!({}));
        assert_eq!(r["result"]["protocolVersion"].as_str(), Some(PROTOCOL_VERSION));
    }

    #[test]
    fn ping_returns_empty_result() {
        let r = call(&server(), r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#);
        assert_eq!(r["result"], json!({}));
    }

    #[test]
    fn tools_list_exposes_five_tools() {
        let r = call(&server(), r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#);
        let tools = r["result"]["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"list_streams"));
        assert!(names.contains(&"list_sinks"));
        assert!(names.contains(&"get_frame"));
        assert!(names.contains(&"submit_frame"));
        assert!(names.contains(&"health"));
    }

    #[test]
    fn notification_gets_no_response() {
        let srv = server();
        match srv.handle_jsonrpc(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#) {
            McpOutcome::Notification => {}
            McpOutcome::Json(_) => panic!("notification must not produce a response"),
        }
    }

    #[test]
    fn unknown_method_is_error() {
        let r = call(&server(), r#"{"jsonrpc":"2.0","id":4,"method":"bogus/method"}"#);
        assert_eq!(r["error"]["code"], json!(-32601));
    }

    #[test]
    fn submit_then_get_frame_roundtrip() {
        let srv = server();
        let png = tiny_png();
        let b64 = B64.encode(&png);
        let submit = format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"submit_frame","arguments":{{"png_base64":"{}"}}}}}}"#,
            b64
        );
        let r = call(&srv, &submit);
        assert_eq!(r["result"]["isError"].as_bool(), Some(false));

        let r = call(
            &srv,
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"get_frame","arguments":{"index":0}}}"#,
        );
        assert_eq!(r["result"]["isError"].as_bool(), Some(false));
        let content = &r["result"]["content"][0];
        assert_eq!(content["type"].as_str(), Some("image"));
        assert_eq!(content["mimeType"].as_str(), Some("image/png"));

        // health now reports a stored frame.
        let r = call(
            &srv,
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"health"}}"#,
        );
        assert_eq!(r["result"]["content"][0]["type"].as_str(), Some("text"));
    }
}
