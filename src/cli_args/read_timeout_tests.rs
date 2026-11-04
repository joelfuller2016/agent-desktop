use super::{GetArgs, IsArgs};
use crate::{batch, cli::Commands};
use agent_desktop_core::commands::batch::BatchCommand;
use clap::Parser;
use serde_json::json;

#[test]
fn omitted_read_timeouts_preserve_the_existing_defaults() {
    let get = GetArgs::try_parse_from(["get", "@snapshot:e1"]).unwrap();
    let is = IsArgs::try_parse_from(["is", "@snapshot:e1"]).unwrap();
    assert_eq!(get.timeout_ms, None);
    assert_eq!(is.timeout_ms, None);
    assert_eq!(get.property, "text");
    assert_eq!(is.property, "visible");
}

#[test]
fn explicit_read_timeouts_preserve_the_property_and_requested_budget() {
    let get = GetArgs::try_parse_from([
        "get",
        "@snapshot:e1",
        "--property",
        "bounds",
        "--timeout-ms",
        "15000",
    ])
    .unwrap();
    let is = IsArgs::try_parse_from([
        "is",
        "@snapshot:e1",
        "--property",
        "enabled",
        "--timeout-ms",
        "15000",
    ])
    .unwrap();
    assert_eq!(get.timeout_ms, Some(15000));
    assert_eq!(is.timeout_ms, Some(15000));
    assert_eq!(get.property, "bounds");
    assert_eq!(is.property, "enabled");
}

#[test]
fn cli_read_timeouts_reject_negative_and_non_numeric_values() {
    for timeout in ["-1", "longer"] {
        assert!(GetArgs::try_parse_from(["get", "@snapshot:e1", "--timeout-ms", timeout]).is_err());
        assert!(IsArgs::try_parse_from(["is", "@snapshot:e1", "--timeout-ms", timeout]).is_err());
    }
}

#[test]
fn zero_read_timeouts_are_forwarded_without_becoming_an_omitted_default() {
    assert_eq!(
        GetArgs::try_parse_from(["get", "@snapshot:e1", "--timeout-ms", "0"])
            .unwrap()
            .timeout_ms,
        Some(0)
    );
    assert_eq!(
        IsArgs::try_parse_from(["is", "@snapshot:e1", "--timeout-ms", "0"])
            .unwrap()
            .timeout_ms,
        Some(0)
    );
}

fn parsed_read(
    command: &str,
    args: serde_json::Value,
) -> Result<Commands, agent_desktop_core::AppError> {
    batch::parse_command(BatchCommand {
        command: command.into(),
        session: None,
        args,
    })
}

#[test]
fn batch_read_timeouts_accept_legacy_args_and_preserve_explicit_budgets() {
    for command in ["get", "is"] {
        for timeout in [None, Some(0), Some(15000), Some(86_400_001)] {
            let mut args = json!({"ref_id": "@snapshot:e1"});
            if let Some(timeout) = timeout {
                args["timeout_ms"] = json!(timeout);
            }
            match parsed_read(command, args).unwrap() {
                Commands::Get(args) => {
                    assert_eq!(args.timeout_ms, timeout);
                    assert_eq!(args.property, "text");
                }
                Commands::Is(args) => {
                    assert_eq!(args.timeout_ms, timeout);
                    assert_eq!(args.property, "visible");
                }
                _ => panic!("expected a read command"),
            }
        }
    }
}

#[test]
fn batch_read_timeouts_reject_invalid_types_and_unknown_fields() {
    for command in ["get", "is"] {
        for args in [
            json!({"ref_id": "@snapshot:e1", "timeout_ms": -1}),
            json!({"ref_id": "@snapshot:e1", "timeout_ms": "15000"}),
            json!({"ref_id": "@snapshot:e1", "timeout_mss": 15000}),
        ] {
            assert!(parsed_read(command, args).is_err());
        }
    }
}
