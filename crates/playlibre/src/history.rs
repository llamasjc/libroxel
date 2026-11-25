//! Undo and redo, by snapshotting the whole tree.
//!
//! Trees are small, so a snapshot per edit is cheap and can't get out of
//! sync with the edit. Rapid edits (dragging a slider, typing a name) are
//! grouped: the snapshot is taken when the burst starts and committed once
//! things have been quiet for a moment.

use playlibre_instance::InstanceTree;

const LIMIT: usize = 200;
/// Seconds of quiet that end a burst of edits.
const SETTLE: f64 = 0.4;

#[derive(Default)]
pub struct History {
    undo: Vec<InstanceTree>,
    redo: Vec<InstanceTree>,
    /// The tree as it was when the current burst of edits started.
    pending: Option<InstanceTree>,
    last_edit: f64,
}

impl History {
    /// Call before changing the tree.
    pub fn begin_edit(&mut self, tree: &InstanceTree, now: f64) {
        if self.pending.is_none() {
            self.pending = Some(tree.clone());
        }
        self.last_edit = now;
    }

    /// Call every frame; commits a finished burst. `busy` holds it open
    /// (for example while a drag is still in progress).
    pub fn tick(&mut self, now: f64, busy: bool) {
        if !busy && self.pending.is_some() && now - self.last_edit > SETTLE {
            self.commit();
        }
    }

    fn commit(&mut self) {
        if let Some(before) = self.pending.take() {
            self.undo.push(before);
            if self.undo.len() > LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    pub fn can_undo(&self) -> bool {
        self.pending.is_some() || !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        self.pending.is_none() && !self.redo.is_empty()
    }

    pub fn undo(&mut self, tree: &mut InstanceTree) -> bool {
        self.commit();
        let Some(previous) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(tree, previous));
        true
    }

    pub fn redo(&mut self, tree: &mut InstanceTree) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(tree, next));
        true
    }

    /// Forget everything (after opening a different file).
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use playlibre_instance::ClassKind;

    fn count(tree: &InstanceTree) -> usize {
        tree.children(tree.workspace()).unwrap().len()
    }

    fn add(tree: &mut InstanceTree) {
        let p = tree.create(ClassKind::Part, "P").unwrap();
        tree.set_parent(p, Some(tree.workspace())).unwrap();
    }

    #[test]
    fn bursts_group_and_undo_redo_round_trip() {
        let mut tree = InstanceTree::new();
        let mut history = History::default();

        history.begin_edit(&tree, 0.0);
        add(&mut tree);
        history.begin_edit(&tree, 0.1);
        add(&mut tree); // same burst
        history.tick(1.0, false);
        history.begin_edit(&tree, 2.0);
        add(&mut tree);
        assert_eq!(count(&tree), 3);

        assert!(history.undo(&mut tree));
        assert_eq!(count(&tree), 2);
        assert!(history.undo(&mut tree));
        assert_eq!(count(&tree), 0, "the first two adds were one burst");
        assert!(!history.undo(&mut tree));
        assert!(history.redo(&mut tree));
        assert!(history.redo(&mut tree));
        assert_eq!(count(&tree), 3);
        assert!(!history.can_redo());
    }
}
