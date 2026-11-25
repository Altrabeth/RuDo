//! The unnamed register: the blocks of tasks the last yank or delete in the tasks pane took.
//!
//! The register holds detached [`TaskBlock`]s, not references into a list, so renaming or
//! deleting the list they came from does not affect them. A yank or delete of a single task
//! fills it with one block, one of a visual selection with a block per selected task.

use crate::markdown::document::TaskBlock;

/// The unnamed register, empty until the first yank or delete.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Register {
    blocks: Vec<TaskBlock>,
}

impl Register {
    /// Replaces the content of the register with `blocks`, in document order.
    pub(super) fn fill(&mut self, blocks: Vec<TaskBlock>) {
        self.blocks = blocks;
    }

    /// Returns the blocks in the register in document order, none if nothing was yanked yet.
    pub(super) fn blocks(&self) -> &[TaskBlock] {
        &self.blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::document::TodoDocument;

    fn block_of_first_task(text: &str) -> TaskBlock {
        let document = TodoDocument::parse(text);
        let first = document.tasks().next().unwrap().id();
        document.copy_block(first).unwrap()
    }

    #[test]
    fn new_register_is_empty() {
        let system_under_test = Register::default();

        assert_eq!(system_under_test.blocks(), []);
    }

    #[test]
    fn fill_replaces_the_previous_blocks() {
        let first = block_of_first_task("- [ ] a\n");
        let second = block_of_first_task("- [ ] b\n\t- [ ] c\n");
        let third = block_of_first_task("- [ ] d\n");
        let expected = [second.clone(), third.clone()];
        let mut register = Register::default();
        register.fill(vec![first]);
        register.fill(vec![second, third]);

        let system_under_test = register.blocks();

        assert_eq!(system_under_test, expected);
    }
}
