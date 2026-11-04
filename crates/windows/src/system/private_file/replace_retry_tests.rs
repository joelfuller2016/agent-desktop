use std::io::{Error, ErrorKind};
use std::time::Duration;

use super::{REPLACE_ATTEMPTS, REPLACE_RETRY_DELAY, replace_file_with_retry};
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_SHARING_VIOLATION,
    ERROR_UNABLE_TO_MOVE_REPLACEMENT, ERROR_UNABLE_TO_MOVE_REPLACEMENT_2,
    ERROR_UNABLE_TO_REMOVE_REPLACED,
};

#[test]
fn a_successful_replace_never_pauses_or_repeats() {
    let mut calls = 0;
    let mut pauses = Vec::new();

    replace_file_with_retry(
        || {
            calls += 1;
            Ok(())
        },
        |duration| pauses.push(duration),
    )
    .unwrap();

    assert_eq!(calls, 1);
    assert!(pauses.is_empty());
}

#[test]
fn unable_to_remove_replaced_retries_and_returns_the_native_success() {
    let mut calls = 0;
    let mut pauses = Vec::new();

    replace_file_with_retry(
        || {
            calls += 1;
            if calls == 1 {
                Err(Error::from_raw_os_error(
                    ERROR_UNABLE_TO_REMOVE_REPLACED as i32,
                ))
            } else {
                Ok(())
            }
        },
        |duration| pauses.push(duration),
    )
    .unwrap();

    assert_eq!(calls, 2);
    assert_eq!(pauses, [Duration::from_millis(10)]);
}

#[test]
fn the_last_allowed_replace_attempt_can_succeed() {
    let mut calls = 0;
    let mut pauses = Vec::new();

    replace_file_with_retry(
        || {
            calls += 1;
            if calls < REPLACE_ATTEMPTS {
                Err(Error::from_raw_os_error(
                    ERROR_UNABLE_TO_REMOVE_REPLACED as i32,
                ))
            } else {
                Ok(())
            }
        },
        |duration| pauses.push(duration),
    )
    .unwrap();

    assert_eq!(calls, 5);
    assert_eq!(pauses, [REPLACE_RETRY_DELAY; 4]);
}

#[test]
fn persistent_unable_to_remove_replaced_exhausts_the_bound_and_preserves_the_error() {
    let mut calls = 0;
    let mut pauses = Vec::new();
    let expected = Error::from_raw_os_error(ERROR_UNABLE_TO_REMOVE_REPLACED as i32);

    let error = replace_file_with_retry(
        || {
            calls += 1;
            Err(Error::from_raw_os_error(
                ERROR_UNABLE_TO_REMOVE_REPLACED as i32,
            ))
        },
        |duration| pauses.push(duration),
    )
    .unwrap_err();

    assert_eq!(calls, 5);
    assert_eq!(pauses, [Duration::from_millis(10); 4]);
    assert_eq!(error.raw_os_error(), expected.raw_os_error());
    assert_eq!(error.kind(), expected.kind());
    assert_eq!(error.to_string(), expected.to_string());
}

#[test]
fn other_os_errors_are_returned_without_a_pause_or_retry() {
    for code in [
        ERROR_ACCESS_DENIED,
        ERROR_SHARING_VIOLATION,
        ERROR_UNABLE_TO_MOVE_REPLACEMENT,
        ERROR_UNABLE_TO_MOVE_REPLACEMENT_2,
        ERROR_FILE_NOT_FOUND,
    ] {
        let mut calls = 0;
        let mut pauses = Vec::new();

        let error = replace_file_with_retry(
            || {
                calls += 1;
                Err(Error::from_raw_os_error(code as i32))
            },
            |duration| pauses.push(duration),
        )
        .unwrap_err();

        assert_eq!(calls, 1, "OS error {code} must not be retried");
        assert!(pauses.is_empty(), "OS error {code} must not pause");
        assert_eq!(error.raw_os_error(), Some(code as i32));
    }
}

#[test]
fn policy_refusals_without_an_os_code_are_returned_without_retry() {
    for kind in [ErrorKind::PermissionDenied, ErrorKind::InvalidData] {
        let mut calls = 0;
        let mut pauses = Vec::new();

        let error = replace_file_with_retry(
            || {
                calls += 1;
                Err(Error::new(kind, "private-file validation refused"))
            },
            |duration| pauses.push(duration),
        )
        .unwrap_err();

        assert_eq!(calls, 1);
        assert!(pauses.is_empty());
        assert_eq!(error.raw_os_error(), None);
        assert_eq!(error.kind(), kind);
        assert_eq!(error.to_string(), "private-file validation refused");
    }
}

#[test]
fn a_different_error_after_a_retry_is_returned_immediately() {
    let mut calls = 0;
    let mut pauses = Vec::new();

    let error = replace_file_with_retry(
        || {
            calls += 1;
            let code = if calls == 1 {
                ERROR_UNABLE_TO_REMOVE_REPLACED
            } else {
                ERROR_ACCESS_DENIED
            };
            Err(Error::from_raw_os_error(code as i32))
        },
        |duration| pauses.push(duration),
    )
    .unwrap_err();

    assert_eq!(calls, 2);
    assert_eq!(pauses, [Duration::from_millis(10)]);
    assert_eq!(error.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32));
}

#[test]
fn an_absent_destination_preserves_the_native_rename_fallback() {
    let scratch = crate::system::private_file::tests::Scratch::new("replace-missing-destination");
    let destination = scratch.path().join("destination.bin");
    let temporary = scratch.path().join("replacement.bin");
    std::fs::write(&temporary, b"new bytes").unwrap();

    super::promote_temp_to_destination(&destination, &temporary).unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"new bytes");
    assert!(!temporary.exists());
}
