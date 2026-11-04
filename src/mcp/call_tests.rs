use super::*;
use agent_desktop_core::{
    AccessibilityNode, NodeIdentity, NodePresentation, ObservationRequest, Rect,
    response::output::Response,
};

fn snapshot_envelope(depth: u8) -> Value {
    let mut node = AccessibilityNode {
        ref_id: Some("@sfixture:e1".into()),
        role: "button".into(),
        identity: NodeIdentity {
            name: Some("Deep control".into()),
            ..Default::default()
        },
        presentation: NodePresentation {
            bounds: Some(Rect {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
            }),
            ..Default::default()
        },
        children_count: None,
        subtree_truncated: false,
        children: vec![],
    };
    for _ in 0..depth {
        node = AccessibilityNode {
            ref_id: None,
            role: "group".into(),
            identity: Default::default(),
            presentation: Default::default(),
            children_count: None,
            subtree_truncated: false,
            children: vec![node],
        };
    }
    serde_json::to_value(Response::ok(
        "snapshot",
        json!({ "app": "fixture", "complete": true, "tree": node }),
    ))
    .unwrap()
}

fn call_with_stdout(stdout: &[u8]) -> Value {
    let tool = super::super::catalog::tools()
        .into_iter()
        .find(|tool| tool.name == "desktop_snapshot")
        .unwrap();
    call_output(&tool, stdout, b"fixture stderr", "exit code: 0")
}

#[test]
fn deep_snapshot_call_output_preserves_the_full_envelope_on_a_worker_thread() {
    std::thread::spawn(|| {
        for depth in [80, ObservationRequest::MAX_RAW_DEPTH] {
            let envelope = snapshot_envelope(depth);
            let stdout = serde_json::to_vec(&envelope).unwrap();
            let result = call_with_stdout(&stdout);
            assert_eq!(result["isError"], false, "depth {depth}");
            assert_eq!(result["structuredContent"], envelope);
            assert_eq!(result["content"][0]["text"], envelope.to_string());
            assert!(result.to_string().contains("Deep control"));
        }
    })
    .join()
    .unwrap();
}

#[test]
fn batch_call_output_preserves_a_snapshot_at_the_raw_depth_limit() {
    std::thread::spawn(|| {
        let mut snapshot = snapshot_envelope(ObservationRequest::MAX_RAW_DEPTH);
        snapshot["index"] = json!(0);
        snapshot["execution"] = json!("completed");
        let envelope =
            serde_json::to_value(Response::ok("batch", json!({ "results": [snapshot] }))).unwrap();
        let result = call_with_stdout(&serde_json::to_vec(&envelope).unwrap());
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"], envelope);
        assert!(result.to_string().contains("Deep control"));
    })
    .join()
    .unwrap();
}

#[test]
fn batch_commands_cannot_add_another_envelope_layer() {
    let command = agent_desktop_core::commands::batch::BatchCommand {
        command: "batch".into(),
        session: None,
        args: json!({ "commands": [] }),
    };
    let error = crate::batch::parse_command(command).unwrap_err();
    assert_eq!(error.to_string(), "Batch commands cannot be nested");
}

#[test]
fn delimiters_and_escaped_quotes_in_strings_do_not_count_as_nesting() {
    let name = format!(
        "{} \"quoted\" \\\\ \\\" {}",
        "[{".repeat(MAX_OUTPUT_JSON_DEPTH + 1),
        "]}".repeat(MAX_OUTPUT_JSON_DEPTH + 1)
    );
    let envelope = json!({ "ok": true, "data": { "name": name } });
    let result = call_with_stdout(&serde_json::to_vec(&envelope).unwrap());
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"], envelope);
}

#[test]
fn malformed_or_trailing_json_never_becomes_structured_content() {
    for stdout in [
        b"".as_slice(),
        b"{",
        b"{\"ok\":true,}",
        b"{\"ok\":true,\"value\":\"\\q\"}",
        b"{\"ok\":true]",
        b"{\"ok\":true} {\"ok\":false}",
        b"{\"ok\":true} trailing",
    ] {
        let result = call_with_stdout(stdout);
        assert_eq!(result["isError"], true, "{stdout:?}");
        assert!(result.get("structuredContent").is_none());
        assert!(
            result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("no JSON envelope: fixture stderr")
        );
    }
}

#[test]
fn call_output_refuses_json_beyond_the_finite_depth_limit() {
    let stdout = format!(
        "{{\"ok\":true,\"data\":{}0{}}}",
        "[".repeat(MAX_OUTPUT_JSON_DEPTH - 1),
        "]".repeat(MAX_OUTPUT_JSON_DEPTH - 1)
    );
    assert!(json_depth_fits(stdout.as_bytes()));
    assert_eq!(call_with_stdout(stdout.as_bytes())["isError"], false);
    let stdout = format!(
        "{{\"ok\":true,\"data\":{}0{}}}",
        "[".repeat(MAX_OUTPUT_JSON_DEPTH),
        "]".repeat(MAX_OUTPUT_JSON_DEPTH)
    );
    let result = call_with_stdout(stdout.as_bytes());
    assert_eq!(result["isError"], true);
    assert!(result.get("structuredContent").is_none());
}

#[test]
fn trailing_whitespace_and_error_envelopes_remain_supported() {
    let envelope = json!({ "ok": false, "error": { "code": "TIMEOUT" } });
    let stdout = format!("{envelope}\n \t\r");
    let result = call_with_stdout(stdout.as_bytes());
    assert_eq!(result["isError"], true);
    assert_eq!(result["structuredContent"], envelope);
}
