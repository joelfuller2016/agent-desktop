use super::SnapshotResult;
use crate::AppError;
use crate::refs::refs_store::RefStore;

impl SnapshotResult {
    pub(crate) fn save_refmap(&self, store: &RefStore) -> Result<String, AppError> {
        if self.complete {
            store.save_new_snapshot(&self.refmap)
        } else {
            store.save_new_snapshot_detached(&self.refmap)
        }
    }

    pub(crate) fn bind_snapshot_id(&mut self, snapshot_id: String) {
        crate::refs::ref_token::qualify_tree_refs(&mut self.tree, &snapshot_id);
        self.snapshot_id = Some(snapshot_id);
    }
}
