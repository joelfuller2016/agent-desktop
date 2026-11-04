//! Runs one tool call as a child process of this same binary and maps its JSON envelope to an MCP result.
//! Each tree level adds a node object and child array. Five batch-envelope containers, the root node and its properties add seven.

use agent_desktop_core::ObservationRequest;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::io::Read;
use std::process::{Command, Stdio};

use super::calls::Calls;
use super::catalog::Tool;

const CANCELLED: &str = "Cancelled";
const MAX_OUTPUT_JSON_DEPTH: usize = 2 * ObservationRequest::MAX_RAW_DEPTH as usize + 7;

/// Runs `tool` with `arguments` for request `id`, prefixed by the server's own global flags, and returns a
/// `CallToolResult`. A cancelled call returns early; the caller sends no response for it.
pub(crate) fn call(
    tool: &Tool,
    arguments: &Map<String, Value>,
    globals: &[String],
    calls: &Calls,
    id: &Value,
) -> Value {
    let argv = match tool.argv(arguments) {
        Ok(argv) => argv,
        Err(message) => return error_result(&message),
    };
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => return error_result(&format!("Cannot locate agent-desktop: {error}")),
    };
    let mut command = Command::new(exe);
    command
        .args(globals)
        .args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (stdout, stderr) = match calls.start(id, &mut command) {
        None => return error_result(CANCELLED),
        Some(Err(error)) => return error_result(&format!("Cannot run agent-desktop: {error}")),
        Some(Ok(pipes)) => pipes,
    };
    let (stdout, stderr) = std::thread::scope(|scope| {
        let stderr = scope.spawn(|| read_all(stderr));
        (read_all(stdout), stderr.join().unwrap_or_default())
    });
    let Some(mut child) = calls.take(id) else {
        return error_result(CANCELLED);
    };
    let status = child
        .wait()
        .map_or_else(|error| error.to_string(), |status| status.to_string());
    call_output(tool, &stdout, &stderr, &status)
}

fn call_output(tool: &Tool, stdout: &[u8], stderr: &[u8], status: &str) -> Value {
    match decode_envelope(stdout) {
        Some(envelope) => envelope_result(envelope),
        None => error_result(&format!(
            "agent-desktop {} exited with {status} and no JSON envelope: {}",
            tool.path.join(" "),
            String::from_utf8_lossy(stderr).trim()
        )),
    }
}

fn decode_envelope(stdout: &[u8]) -> Option<Value> {
    if !json_depth_fits(stdout) {
        return None;
    }
    let mut deserializer = serde_json::Deserializer::from_slice(stdout);
    deserializer.disable_recursion_limit();
    let envelope = Value::deserialize(&mut deserializer).ok()?;
    deserializer.end().ok()?;
    Some(envelope)
}

fn json_depth_fits(bytes: &[u8]) -> bool {
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for &byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else {
                match byte {
                    b'\\' => escaped = true,
                    b'"' => in_string = false,
                    _ => {}
                }
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_OUTPUT_JSON_DEPTH {
                    return false;
                }
            }
            b'}' | b']' => {
                let Some(parent_depth) = depth.checked_sub(1) else {
                    return false;
                };
                depth = parent_depth;
            }
            _ => {}
        }
    }
    depth == 0 && !in_string
}

fn read_all(pipe: Option<impl Read>) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut bytes);
    }
    bytes
}

/// Maps an envelope to a result: the envelope as structured content and text, an image block for inline screenshots.
pub(crate) fn envelope_result(mut envelope: Value) -> Value {
    let is_error = envelope["ok"] != Value::Bool(true);
    let image = take_inline_image(&mut envelope);
    let mut content = vec![json!({ "type": "text", "text": envelope.to_string() })];
    content.extend(image);
    json!({ "content": content, "structuredContent": envelope, "isError": is_error })
}

fn take_inline_image(envelope: &mut Value) -> Option<Value> {
    let data = envelope.get_mut("data")?.as_object_mut()?;
    let mime_type = match data.get("format")?.as_str()? {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        _ => return None,
    };
    if !data.get("data").is_some_and(Value::is_string) {
        return None;
    }
    let Value::String(encoded) = data.remove("data")? else {
        return None;
    };
    Some(json!({ "type": "image", "data": encoded, "mimeType": mime_type }))
}

fn error_result(message: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

#[cfg(test)]
#[path = "call_tests.rs"]
mod tests;
