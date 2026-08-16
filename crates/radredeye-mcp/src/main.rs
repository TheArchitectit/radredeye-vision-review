//! radredeye — unified capture server for AI agents.
//!
//! Listens on `0.0.0.0:8765`:
//! - POST `/mcp` — **stateless MCP** (Model Context Protocol) Streamable HTTP.
//!   This is the primary agent interface: list/get/submit capture frames as
//!   MCP tools. No session id is required.
//! - POST `/capture` — legacy PNG submission from any app framework (back-compat
//!   with the Godot addon). Frames land in the same pipeline + frame store the
//!   MCP tools read from.
//! - GET  `/health` — liveness probe.
//! - GET  `/frame[?index=N]` — latest stored frame as PNG.

use std::sync::{Arc, Mutex};

use tiny_http::{Header, Method, Response, Server};

use radredeye_core::{
    sinks::{file::FileSink, http::HttpSink, StdoutSink},
    CapturePipeline, ReplayBuffer, FRAME_STORE_CAP,
};
use radredeye_mcp::{dispatch_request, McpOutcome, McpServer};

/// Build a `200 application/json` response from a serialized JSON-RPC body.
fn json_response(body: String, status: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    let resp = Response::from_string(body).with_status_code(status);
    match Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]) {
        Ok(h) => resp.with_header(h),
        Err(_) => resp,
    }
}

fn main() {
    let pipeline = CapturePipeline::new();
    pipeline.register_sink(Arc::new(StdoutSink));
    pipeline.register_sink(Arc::new(
        FileSink::new("captures/godot").expect("could not create captures/godot"),
    ));
    if let Ok(url) = std::env::var("RADREDEYE_HTTP_SINK_URL") {
        pipeline.register_sink(Arc::new(HttpSink::new(url)));
    }

    let pipeline = Arc::new(Mutex::new(pipeline));
    let frame_store = Arc::new(ReplayBuffer::new(FRAME_STORE_CAP));
    let mcp = McpServer::new(pipeline.clone(), frame_store.clone());

    let addr = "0.0.0.0:8765";
    let server = Server::http(addr).expect("failed to start HTTP server");
    println!("[radredeye-mcp] listening on {addr} (MCP stateless @ /mcp; legacy @ /capture /frame /health)");

    let health_body = r#"{"status":"ok"}"#;

    for mut request in server.incoming_requests() {
        let method = request.method().clone();
        let url = request.url().to_string();

        let mut body = Vec::new();
        if method == Method::Post {
            if let Err(e) = request.as_reader().read_to_end(&mut body) {
                eprintln!("[radredeye-mcp] failed to read request body: {e}");
                let _ = request.respond(Response::from_string("bad request").with_status_code(400));
                continue;
            }
        }

        // Primary interface: stateless MCP JSON-RPC at /mcp.
        if url.split('?').next() == Some("/mcp") {
            if method == Method::Post {
                match mcp.handle_jsonrpc(&body) {
                    McpOutcome::Notification => {
                        let _ = request.respond(Response::from_string("").with_status_code(202));
                    }
                    McpOutcome::Json(s) => {
                        let _ = request.respond(json_response(s, 200));
                    }
                }
            } else {
                // stateless mode has no server-initiated SSE stream; only POST.
                let _ = request.respond(
                    Response::from_string("Method Not Allowed — POST JSON-RPC to /mcp")
                        .with_status_code(405),
                );
            }
            continue;
        }

        // Legacy HTTP bridge routes (app-framework frame submission + debugging).
        let response = dispatch_request(&method, &url, &body, &pipeline, &frame_store, health_body);
        let _ = request.respond(response);
    }
}
