use std::sync::Mutex;
use std::time::Duration;

use crate::adapter::{ActionOps, InputOps, ObservationOps, SystemOps};
use crate::commands::{get, is_check};
use crate::locator::live_locator::{ObservationRequest, ObservationRoot, ObservedTree};
use crate::refs::{RefMap, refs_store::RefStore, refs_test_support::HomeGuard};
use crate::wait::deadline::{VirtualClock, enter_scope, sleep};
use crate::{
    AccessibilityNode, AdapterError, AppError, CommandContext, Deadline, ElementState,
    NativeHandle, Rect, RefEntry,
};
use serde_json::Value;

struct BudgetAdapter {
    resolution_ms: u64,
    state_ms: u64,
    bounds_ms: u64,
    tree: AccessibilityNode,
    received: Mutex<Vec<(&'static str, Deadline)>>,
}

impl BudgetAdapter {
    fn new(resolution_ms: u64, read_ms: u64, state_check: bool) -> Self {
        let mut tree = crate::adapter::minimal_accessibility_node("group", "Window");
        tree.children
            .push(crate::adapter::minimal_accessibility_node(
                "switch", "Target",
            ));
        Self {
            resolution_ms,
            state_ms: if state_check { read_ms / 2 } else { 0 },
            bounds_ms: if state_check {
                read_ms - read_ms / 2
            } else {
                read_ms
            },
            tree,
            received: Mutex::new(Vec::new()),
        }
    }

    fn record(&self, stage: &'static str, deadline: Deadline, elapsed_ms: u64) {
        self.received.lock().unwrap().push((stage, deadline));
        sleep(Duration::from_millis(elapsed_ms));
    }

    fn live_reads(&self) -> usize {
        self.received
            .lock()
            .unwrap()
            .iter()
            .filter(|(stage, _)| matches!(*stage, "state" | "bounds"))
            .count()
    }
}

impl ObservationOps for BudgetAdapter {
    crate::adapter::saved_ref_scope!(@inventory);

    fn observe_tree(
        &self,
        root: ObservationRoot<'_>,
        request: &ObservationRequest,
    ) -> Result<ObservedTree, AdapterError> {
        match root {
            ObservationRoot::Window(_) => {
                self.record("window", request.deadline, self.resolution_ms);
                crate::adapter::observed_tree(&root, self.tree.clone())
            }
            ObservationRoot::Element { .. } => {
                self.record("confirm", request.deadline, 0);
                crate::adapter::saved_ref_element_tree(root)
            }
        }
    }

    fn resolve_locator_anchor(
        &self,
        _entry: &RefEntry,
        deadline: Deadline,
    ) -> Result<NativeHandle, AdapterError> {
        self.record("anchor", deadline, 0);
        Ok(NativeHandle::null())
    }

    fn get_live_state(
        &self,
        _handle: &NativeHandle,
        deadline: Deadline,
    ) -> Result<Option<ElementState>, AdapterError> {
        self.record("state", deadline, self.state_ms);
        Ok(Some(ElementState {
            role: "switch".into(),
            states: Vec::new(),
            value: None,
            enabled: Some(true),
            hidden: Some(false),
            offscreen: Some(false),
        }))
    }

    fn get_element_bounds(
        &self,
        _handle: &NativeHandle,
        deadline: Deadline,
    ) -> Result<Option<Rect>, AdapterError> {
        self.record("bounds", deadline, self.bounds_ms);
        Ok(Some(Rect {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        }))
    }
}

impl ActionOps for BudgetAdapter {}
impl InputOps for BudgetAdapter {}
impl SystemOps for BudgetAdapter {}

fn save_target() -> String {
    let mut refs = RefMap::new();
    let local = refs.allocate(crate::adapter::minimal_ref_entry("switch", Some("Target")));
    let snapshot = RefStore::new().unwrap().save_new_snapshot(&refs).unwrap();
    format!("@{snapshot}:{}", local.trim_start_matches('@'))
}

fn read(
    state_check: bool,
    timeout_ms: Option<u64>,
    adapter: &BudgetAdapter,
    target: &str,
) -> Result<Value, AppError> {
    let context = CommandContext::default();
    if state_check {
        let args = is_check::IsArgs {
            ref_id: target.into(),
            property: is_check::IsProperty::Visible,
        };
        match timeout_ms {
            Some(timeout) => is_check::execute_with_timeout(args, timeout, adapter, &context),
            None => is_check::execute(args, adapter, &context),
        }
    } else {
        let args = get::GetArgs {
            ref_id: target.into(),
            property: get::GetProperty::Bounds,
        };
        match timeout_ms {
            Some(timeout) => get::execute_with_timeout(args, timeout, adapter, &context),
            None => get::execute(args, adapter, &context),
        }
    }
}

#[test]
fn explicit_budget_above_five_seconds_reaches_resolution_and_live_reads_unchanged() {
    let _home = HomeGuard::new();
    let target = save_target();
    for state_check in [false, true] {
        let _clock = VirtualClock::freeze();
        let adapter = BudgetAdapter::new(6_000, 1_000, state_check);

        read(state_check, Some(15_000), &adapter, &target).unwrap();

        let received = adapter.received.lock().unwrap();
        let deadline = received[0].1;
        assert_eq!(deadline.timeout_ms(), 15_000);
        assert!(!deadline.was_capped());
        assert!(received.iter().all(|(_, actual)| *actual == deadline));
        assert_eq!(deadline.remaining_ms(), 8_000);
    }
}

#[test]
fn an_explicit_budget_is_not_reset_after_resolution() {
    let _home = HomeGuard::new();
    let target = save_target();
    for state_check in [false, true] {
        let _clock = VirtualClock::freeze();
        let adapter = BudgetAdapter::new(4_000, 2_000, state_check);

        let error = read(state_check, Some(5_000), &adapter, &target).unwrap_err();

        assert_eq!(error.code(), "TIMEOUT");
        let received = adapter.received.lock().unwrap();
        let deadline = received[0].1;
        assert!(received.iter().all(|(_, actual)| *actual == deadline));
        assert!(deadline.is_expired());
    }
}

#[test]
fn omitted_timeout_preserves_fresh_five_second_resolution_and_read_stages() {
    let _home = HomeGuard::new();
    let target = save_target();
    for state_check in [false, true] {
        let clock = VirtualClock::freeze();
        let adapter = BudgetAdapter::new(4_000, 4_000, state_check);

        read(state_check, None, &adapter, &target).unwrap();

        let received = adapter.received.lock().unwrap();
        let resolution = received[0].1;
        let live = received
            .iter()
            .find(|(stage, _)| matches!(*stage, "state" | "bounds"))
            .unwrap()
            .1;
        assert_eq!(resolution.timeout_ms(), 5_000);
        assert_eq!(live.timeout_ms(), 5_000);
        assert_ne!(resolution, live);
        assert_eq!(live.remaining_ms(), 1_000);
        assert_eq!(clock.elapsed(), Duration::from_millis(8_000));
    }
}

#[test]
fn an_inherited_deadline_caps_a_larger_explicit_budget() {
    let _home = HomeGuard::new();
    let target = save_target();
    for state_check in [false, true] {
        let _clock = VirtualClock::freeze();
        let inherited = Deadline::after(2_000).unwrap();
        let _scope = enter_scope(Some(inherited));
        let adapter = BudgetAdapter::new(1_000, 2_000, state_check);

        let error = read(state_check, Some(15_000), &adapter, &target).unwrap_err();

        assert_eq!(error.code(), "TIMEOUT");
        assert!(inherited.is_expired());
        assert!(
            adapter
                .received
                .lock()
                .unwrap()
                .iter()
                .all(|(_, deadline)| deadline.was_capped())
        );
    }
}

#[test]
fn zero_and_out_of_range_explicit_budgets_never_reach_the_adapter() {
    let _home = HomeGuard::new();
    let target = save_target();
    for state_check in [false, true] {
        for (timeout, code) in [
            (0, "TIMEOUT"),
            (crate::MAX_WAIT_TIMEOUT_MS + 1, "INVALID_ARGS"),
        ] {
            let _clock = VirtualClock::freeze();
            let adapter = BudgetAdapter::new(0, 0, state_check);

            let error = read(state_check, Some(timeout), &adapter, &target).unwrap_err();

            assert_eq!(error.code(), code);
            assert!(adapter.received.lock().unwrap().is_empty());
        }
    }
}

#[test]
fn resolution_that_finishes_after_the_explicit_deadline_never_starts_a_live_read() {
    let _home = HomeGuard::new();
    let target = save_target();
    for state_check in [false, true] {
        let _clock = VirtualClock::freeze();
        let adapter = BudgetAdapter::new(6_000, 0, state_check);

        let error = read(state_check, Some(5_000), &adapter, &target).unwrap_err();

        assert_eq!(error.code(), "TIMEOUT");
        assert_eq!(adapter.live_reads(), 0);
    }
}

#[test]
fn successful_bounds_arriving_after_the_explicit_deadline_are_not_published() {
    let _home = HomeGuard::new();
    let target = save_target();
    for state_check in [false, true] {
        let _clock = VirtualClock::freeze();
        let mut adapter = BudgetAdapter::new(0, 0, state_check);
        adapter.bounds_ms = 2_000;

        let error = read(state_check, Some(1_000), &adapter, &target).unwrap_err();

        assert_eq!(error.code(), "TIMEOUT");
        assert!(
            adapter
                .received
                .lock()
                .unwrap()
                .iter()
                .any(|(stage, _)| *stage == "bounds")
        );
    }
}

#[test]
fn an_expired_state_read_prevents_the_visible_bounds_read() {
    let _home = HomeGuard::new();
    let target = save_target();
    let _clock = VirtualClock::freeze();
    let mut adapter = BudgetAdapter::new(0, 0, true);
    adapter.state_ms = 2_000;

    let error = read(true, Some(1_000), &adapter, &target).unwrap_err();

    assert_eq!(error.code(), "TIMEOUT");
    assert_eq!(adapter.live_reads(), 1);
    assert!(
        !adapter
            .received
            .lock()
            .unwrap()
            .iter()
            .any(|(stage, _)| *stage == "bounds")
    );
}

#[test]
fn a_larger_timeout_still_refuses_a_live_identical_twin() {
    let _home = HomeGuard::new();
    let target = save_target();
    for state_check in [false, true] {
        let _clock = VirtualClock::freeze();
        let mut adapter = BudgetAdapter::new(0, 0, state_check);
        adapter.tree.children.push(adapter.tree.children[0].clone());

        let error = read(state_check, Some(15_000), &adapter, &target).unwrap_err();

        assert_eq!(error.code(), "AMBIGUOUS_TARGET");
        assert_eq!(adapter.received.lock().unwrap().len(), 1);
        assert_eq!(adapter.live_reads(), 0);
    }
}

#[test]
fn a_larger_timeout_still_refuses_incomplete_whole_window_identity_evidence() {
    let _home = HomeGuard::new();
    let target = save_target();
    for state_check in [false, true] {
        let _clock = VirtualClock::freeze();
        let mut adapter = BudgetAdapter::new(0, 0, state_check);
        adapter.tree.subtree_truncated = true;

        let error = read(state_check, Some(15_000), &adapter, &target).unwrap_err();

        assert_eq!(error.code(), "TIMEOUT");
        let AppError::Adapter(error) = error else {
            panic!("expected identity refusal")
        };
        assert_eq!(
            error.details.unwrap()["kind"],
            "resolution_identity_unknown"
        );
        assert_eq!(adapter.received.lock().unwrap().len(), 1);
        assert_eq!(adapter.live_reads(), 0);
    }
}
