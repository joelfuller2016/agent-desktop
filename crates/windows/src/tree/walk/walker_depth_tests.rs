use super::DEFAULT_MAX_RAW_DEPTH;
use crate::tree::properties::property_ids::TreeProperty;
use crate::tree::properties::{PropertyOutcome, PropertyValue};
use crate::tree::walk::walker_fake::{FakeTree, budget, walk};

#[test]
fn a_control_below_fifty_raw_levels_is_observed_completely() {
    let chain: Vec<i32> = (1..=81).collect();
    let mut fake = FakeTree::default().with_chain(&chain).reading(
        81,
        &[(
            TreeProperty::Name,
            PropertyOutcome::Known(PropertyValue::Text("Deep control".into())),
        )],
    );
    for node in 1..81 {
        fake = fake.wrapping(node);
    }

    let outcome = walk(&fake, budget(10));

    assert!(outcome.tree.is_complete());
    assert_eq!(outcome.stats.traversal.limits.depth_hits, 0);
    assert_eq!(outcome.stats.traversal.max_raw_depth, 80);
    assert_eq!(outcome.stats.traversal.nodes_visited, 81);
    let tree = outcome.tree.into_accessibility_tree().unwrap();
    let mut leaf = &tree;
    while let Some(child) = leaf.children.first() {
        leaf = child;
    }
    assert_eq!(leaf.identity.name.as_deref(), Some("Deep control"));
}

#[test]
fn exceeding_the_native_depth_bound_remains_incomplete() {
    let chain: Vec<i32> = (1..=i32::from(DEFAULT_MAX_RAW_DEPTH) + 2).collect();
    let fake = FakeTree::default().with_chain(&chain);

    let outcome = walk(&fake, budget(DEFAULT_MAX_RAW_DEPTH));

    assert!(!outcome.tree.is_complete());
    assert_eq!(outcome.stats.traversal.max_raw_depth, DEFAULT_MAX_RAW_DEPTH);
    assert!(outcome.stats.traversal.limits.depth_hits > 0);
    assert_eq!(
        outcome
            .tree
            .clone()
            .into_accessibility_tree()
            .unwrap_err()
            .code,
        agent_desktop_core::ErrorCode::Timeout
    );
    let (tree, complete, _) = outcome.tree.into_accessibility_tree_partial().unwrap();
    assert!(!complete);
    let mut boundary = &tree;
    while let Some(child) = boundary.children.first() {
        boundary = child;
    }
    assert!(boundary.subtree_truncated);
}
