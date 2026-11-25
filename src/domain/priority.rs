//! Task priorities (`[priority:: high]`) as Obsidian Tasks writes them in the Dataview format.
//!
//! A task without a priority field has normal priority, which lies between [`Priority::Medium`]
//! and [`Priority::Low`]. Normal is therefore not a [`Priority`] but its absence, and [`step`]
//! works on `Option<Priority>`.

use std::fmt;
use std::str::FromStr;

use strum::IntoEnumIterator;

/// Every priority level from lowest to highest, with `None` for normal priority.
const LEVELS: [Option<Priority>; 6] = [
    Some(Priority::Lowest),
    Some(Priority::Low),
    None,
    Some(Priority::Medium),
    Some(Priority::High),
    Some(Priority::Highest),
];

/// The priority of a task that has a priority field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum Priority {
    /// `lowest`
    Lowest,
    /// `low`
    Low,
    /// `medium`, one level above normal priority.
    Medium,
    /// `high`
    High,
    /// `highest`
    Highest,
}

impl Priority {
    /// Returns the value of the priority field, such as `high`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lowest => "lowest",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Highest => "highest",
        }
    }
}

/// The error returned when a priority field holds none of the five values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[error("unknown priority")]
pub struct UnknownPriorityError;

impl FromStr for Priority {
    type Err = UnknownPriorityError;

    /// Parses the value of a priority field, such as `high`. Values are case sensitive, as in
    /// Obsidian Tasks.
    ///
    /// # Errors
    ///
    /// Returns [`UnknownPriorityError`] if `value` is none of `lowest`, `low`, `medium`,
    /// `high` and `highest`.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::iter()
            .find(|priority| priority.as_str() == value)
            .ok_or(UnknownPriorityError)
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The direction in which [`step`] changes a priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    /// Towards [`Priority::Highest`].
    Up,
    /// Towards [`Priority::Lowest`].
    Down,
}

/// Returns the priority one level above or below `priority`, where `None` is normal priority.
///
/// The levels are lowest, low, normal, medium, high and highest. Stepping stops at both ends
/// instead of wrapping around.
///
/// # Examples
///
/// ```
/// use rudo::domain::priority::{Priority, Step, step};
///
/// assert_eq!(step(None, Step::Up), Some(Priority::Medium));
/// assert_eq!(step(Some(Priority::Low), Step::Up), None);
/// assert_eq!(step(Some(Priority::Highest), Step::Up), Some(Priority::Highest));
/// ```
#[must_use]
pub fn step(priority: Option<Priority>, direction: Step) -> Option<Priority> {
    let index = level(priority);
    let stepped = match direction {
        Step::Up => index.checked_add(1),
        Step::Down => index.checked_sub(1),
    };
    stepped
        .and_then(|stepped| LEVELS.get(stepped))
        .map_or(priority, |&level| level)
}

/// Returns the level of `priority` for comparing priorities: 0 for lowest up to 5 for highest,
/// where `None` is normal priority between low and medium.
///
/// # Examples
///
/// ```
/// use rudo::domain::priority::{Priority, level};
///
/// assert!(level(Some(Priority::Low)) < level(None));
/// assert!(level(None) < level(Some(Priority::Medium)));
/// ```
#[must_use]
pub fn level(priority: Option<Priority>) -> usize {
    // Every priority and normal priority are levels, so the position is always found.
    LEVELS
        .iter()
        .position(|&level| level == priority)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::lowest("lowest", Priority::Lowest)]
    #[case::low("low", Priority::Low)]
    #[case::medium("medium", Priority::Medium)]
    #[case::high("high", Priority::High)]
    #[case::highest("highest", Priority::Highest)]
    fn priority_parses_its_field_value(#[case] value: &str, #[case] expected: Priority) {
        let system_under_test = value.parse::<Priority>();

        assert_eq!(system_under_test, Ok(expected));
    }

    #[rstest]
    #[case::unknown_word("urgent")]
    #[case::normal("normal")]
    #[case::empty("")]
    #[case::upper_case("High")]
    #[case::padded(" high")]
    fn priority_rejects_other_values(#[case] value: &str) {
        let system_under_test = value.parse::<Priority>();

        assert_eq!(system_under_test, Err(UnknownPriorityError));
    }

    #[test]
    fn every_priority_round_trips_through_display() {
        let expected: Vec<Result<Priority, UnknownPriorityError>> =
            Priority::iter().map(Ok).collect();

        let system_under_test: Vec<Result<Priority, UnknownPriorityError>> = Priority::iter()
            .map(|priority| priority.to_string().parse())
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::lowest_up(Some(Priority::Lowest), Step::Up, Some(Priority::Low))]
    #[case::low_up(Some(Priority::Low), Step::Up, None)]
    #[case::normal_up(None, Step::Up, Some(Priority::Medium))]
    #[case::medium_up(Some(Priority::Medium), Step::Up, Some(Priority::High))]
    #[case::high_up(Some(Priority::High), Step::Up, Some(Priority::Highest))]
    #[case::highest_up_stops(Some(Priority::Highest), Step::Up, Some(Priority::Highest))]
    #[case::highest_down(Some(Priority::Highest), Step::Down, Some(Priority::High))]
    #[case::high_down(Some(Priority::High), Step::Down, Some(Priority::Medium))]
    #[case::medium_down(Some(Priority::Medium), Step::Down, None)]
    #[case::normal_down(None, Step::Down, Some(Priority::Low))]
    #[case::low_down(Some(Priority::Low), Step::Down, Some(Priority::Lowest))]
    #[case::lowest_down_stops(Some(Priority::Lowest), Step::Down, Some(Priority::Lowest))]
    fn step_moves_one_level_and_stops_at_the_ends(
        #[case] priority: Option<Priority>,
        #[case] direction: Step,
        #[case] expected: Option<Priority>,
    ) {
        let system_under_test = step(priority, direction);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn levels_rise_from_lowest_through_normal_to_highest() {
        let priorities = [
            Some(Priority::Lowest),
            Some(Priority::Low),
            None,
            Some(Priority::Medium),
            Some(Priority::High),
            Some(Priority::Highest),
        ];
        let expected: Vec<usize> = (0..priorities.len()).collect();

        let system_under_test: Vec<usize> = priorities.into_iter().map(level).collect();

        assert_eq!(system_under_test, expected);
    }
}
