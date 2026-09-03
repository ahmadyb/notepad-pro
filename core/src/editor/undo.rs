//! A bounded undo/redo stack over whole-document snapshots.
//!
//! Semantics: `states[0]` is the pristine document, `states[i]` is the
//! document *after* edit `i`, and `index` always points at the current state.
//! Pushing after the change (rather than snapshotting before it) removes a
//! whole class of "undo restores the wrong frame" bugs, which is what the
//! original implementation suffered from.

use crate::types::line::EditorLine;

/// Maximum number of retained states.
pub const MAX_STATES: usize = 200;

#[derive(Debug, Clone)]
pub struct UndoStack {
    states: Vec<Vec<EditorLine>>,
    index: usize,
}

impl UndoStack {
    /// Create a stack whose current state is `initial`.
    pub fn new(initial: Vec<EditorLine>) -> Self {
        Self {
            states: vec![initial],
            index: 0,
        }
    }

    /// Record a new state produced by an edit.
    ///
    /// Redo history above the cursor is discarded. A no-op edit (state equal
    /// to the current one) is ignored so the stack is not polluted.
    pub fn push(&mut self, next: Vec<EditorLine>) {
        if self.states.get(self.index).map(|s| *s == next).unwrap_or(false) {
            return;
        }
        self.states.truncate(self.index + 1);
        self.states.push(next);
        if self.states.len() > MAX_STATES {
            self.states.remove(0);
        }
        self.index = self.states.len() - 1;
    }

    /// Overwrite the current state in place.
    ///
    /// Used to coalesce a burst of keystrokes in the same line into a single
    /// undo step, so Ctrl+Z rewinds the word rather than the character.
    pub fn replace_top(&mut self, state: Vec<EditorLine>) {
        if let Some(slot) = self.states.get_mut(self.index) {
            *slot = state;
        }
    }

    pub fn can_replace_top(&self) -> bool {
        self.index < self.states.len()
    }

    pub fn undo(&mut self) -> Option<Vec<EditorLine>> {
        if self.index == 0 {
            return None;
        }
        self.index -= 1;
        Some(self.states[self.index].clone())
    }

    pub fn redo(&mut self) -> Option<Vec<EditorLine>> {
        if self.index + 1 >= self.states.len() {
            return None;
        }
        self.index += 1;
        Some(self.states[self.index].clone())
    }

    pub fn can_undo(&self) -> bool {
        self.index > 0
    }

    pub fn can_redo(&self) -> bool {
        self.index + 1 < self.states.len()
    }

    /// Discard all history, keeping `state` as the new baseline.
    pub fn reset(&mut self, state: Vec<EditorLine>) {
        self.states = vec![state];
        self.index = 0;
    }

    /// Number of retained states (including the baseline).
    pub fn len(&self) -> usize {
        self.states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Depth of the undo history from the current position.
    pub fn undo_depth(&self) -> usize {
        self.index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::line::EditorLine;

    fn doc(texts: &[&str]) -> Vec<EditorLine> {
        texts.iter().map(|t| EditorLine::new(*t)).collect()
    }

    #[test]
    fn fresh_stack_cannot_undo_or_redo() {
        let stack = UndoStack::new(doc(&["a"]));
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.undo_depth(), 0);
    }

    #[test]
    fn push_then_undo_restores_previous_state() {
        let mut stack = UndoStack::new(doc(&["a"]));
        stack.push(doc(&["a", "b"]));
        assert!(stack.can_undo());
        let prev = stack.undo().expect("one undo step available");
        assert_eq!(prev.len(), 1);
        assert_eq!(prev[0].text, "a");
    }

    #[test]
    fn redo_walks_forward_again() {
        let mut stack = UndoStack::new(doc(&["a"]));
        stack.push(doc(&["a", "b"]));
        stack.undo();
        assert!(stack.can_redo());
        let next = stack.redo().expect("redo available");
        assert_eq!(next.len(), 2);
        assert!(!stack.can_redo());
    }

    #[test]
    fn pushing_after_undo_discards_redo_branch() {
        let mut stack = UndoStack::new(doc(&["a"]));
        stack.push(doc(&["a", "b"]));
        stack.push(doc(&["a", "b", "c"]));
        stack.undo();
        stack.push(doc(&["a", "z"]));
        assert!(!stack.can_redo(), "the discarded branch must be gone");
        assert_eq!(stack.len(), 3);
    }

    #[test]
    fn identical_states_are_not_pushed() {
        let mut stack = UndoStack::new(doc(&["a"]));
        stack.push(doc(&["a"]));
        assert!(!stack.can_undo());
        assert_eq!(stack.len(), 1);
    }

    #[test]
    fn stack_is_bounded_at_max_states() {
        let mut stack = UndoStack::new(doc(&["0"]));
        for i in 1..=(MAX_STATES + 50) {
            stack.push(doc(&[&i.to_string()]));
        }
        assert_eq!(stack.len(), MAX_STATES);
        assert!(stack.can_undo());
    }

    #[test]
    fn bounding_keeps_the_cursor_inside_the_buffer() {
        let mut stack = UndoStack::new(doc(&["0"]));
        for i in 1..=(MAX_STATES + 5) {
            stack.push(doc(&[&i.to_string()]));
        }
        // Undo all the way back; must terminate on the oldest retained state.
        let mut steps = 0;
        while stack.undo().is_some() {
            steps += 1;
            assert!(steps < MAX_STATES + 10, "undo loop did not terminate");
        }
        assert!(!stack.can_undo());
        assert_eq!(steps, MAX_STATES - 1);
    }

    #[test]
    fn replace_top_does_not_grow_the_stack() {
        let mut stack = UndoStack::new(doc(&["a"]));
        stack.push(doc(&["a", "b"]));
        let before = stack.len();
        stack.replace_top(doc(&["a", "b", "c"]));
        assert_eq!(stack.len(), before);
        let state = stack.undo().unwrap();
        assert_eq!(state.len(), 1);
        let state = stack.redo().unwrap();
        assert_eq!(state.len(), 3);
    }

    #[test]
    fn reset_clears_history() {
        let mut stack = UndoStack::new(doc(&["a"]));
        stack.push(doc(&["a", "b"]));
        stack.reset(doc(&["x"]));
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
        assert_eq!(stack.len(), 1);
    }

    #[test]
    fn undo_at_the_bottom_is_a_noop() {
        let mut stack = UndoStack::new(doc(&["a"]));
        assert!(stack.undo().is_none());
        assert!(stack.redo().is_none());
    }
}
