//! The unnamed register: the block of tasks the last yank or delete in the tasks pane took.
//!
//! The register holds a detached [`TaskBlock`], not a reference into a list, so renaming or
//! deleting the list it came from does not affect it.

use crate::markdown::document::TaskBlock;

/// The unnamed register, empty until the first yank or delete.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Register {
    block: Option<TaskBlock>,
}

impl Register {
    /// Replaces the content of the register with `block`.
    pub(super) fn fill(&mut self, block: TaskBlock) {
        self.block = Some(block);
    }

    /// Returns the block in the register, or `None` if nothing was yanked yet.
    pub(super) fn block(&self) -> Option<&TaskBlock> {
        self.block.as_ref()
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

        assert_eq!(system_under_test.block(), None);
    }

    #[test]
    fn fill_replaces_the_previous_block() {
        let first = block_of_first_task("- [ ] a\n");
        let second = block_of_first_task("- [ ] b\n\t- [ ] c\n");
        let expected = second.clone();
        let mut register = Register::default();
        register.fill(first);
        register.fill(second);

        let system_under_test = register.block();

        assert_eq!(system_under_test, Some(&expected));
    }
}
