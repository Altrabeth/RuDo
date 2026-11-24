//! Recurrence rules (`[repeat:: every 2 weeks]`) and the date of a task's next occurrence.
//!
//! RuDo supports the subset of the [Obsidian Tasks](https://publish.obsidian.md/tasks/)
//! recurrence syntax that repeats in fixed calendar steps: `every [N] day(s)`, `week(s)`,
//! `month(s)` or `year(s)`. Rules are parsed case-insensitively with any whitespace between
//! the words, and are written back in the canonical form produced by [`Recurrence`]'s
//! `Display`.
//!
//! Adding months or years clamps the day to the end of the target month, so January 31 plus
//! one month is February 28 (or 29). Because every occurrence is computed from the previous
//! one, a monthly rule that starts on the 31st drifts: January 31, February 28, March 28.
//! Obsidian Tasks behaves the same.

use std::fmt;
use std::num::{IntErrorKind, NonZeroU32, ParseIntError};
use std::str::FromStr;

use jiff::Span;
use jiff::civil::Date;
use strum::IntoEnumIterator;

/// The word every recurrence rule starts with.
const EVERY: &str = "every";

/// Errors that occur while parsing a recurrence rule or computing the next occurrence.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RecurrenceError {
    /// The rule does not start with the word `every`.
    #[error("recurrence rule must start with `every`")]
    MissingEvery,
    /// The rule repeats every zero units, as in `every 0 days`.
    #[error("recurrence interval must be at least 1")]
    ZeroInterval,
    /// The interval does not fit into a 32 bit unsigned integer.
    #[error("recurrence interval is too large")]
    IntervalOutOfRange,
    /// The rule is not one of the supported forms. This includes valid Obsidian Tasks rules
    /// such as `every weekday`, `every week on Monday` and `every day when done`.
    #[error("unsupported recurrence rule")]
    Unsupported,
    /// The next occurrence lies outside the range of supported dates.
    #[error("next occurrence is out of the supported date range")]
    DateOutOfRange(#[source] jiff::Error),
}

/// The calendar unit a [`Recurrence`] counts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum RecurrenceUnit {
    /// Calendar days.
    Day,
    /// Weeks of seven days.
    Week,
    /// Calendar months; days past the end of the target month are clamped.
    Month,
    /// Calendar years; February 29 is clamped to February 28 in common years.
    Year,
}

impl RecurrenceUnit {
    /// Returns the singular English name; the plural appends an `s`.
    const fn singular(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }

    /// Parses the singular or plural name of a unit, ignoring ASCII case.
    fn from_word(word: &str) -> Option<Self> {
        let singular = word.strip_suffix(['s', 'S']).unwrap_or(word);
        Self::iter().find(|unit| unit.singular().eq_ignore_ascii_case(singular))
    }

    /// Returns a span of `count` of this unit.
    fn span(self, count: NonZeroU32) -> Result<Span, jiff::Error> {
        let count = i64::from(count.get());
        let span = Span::new();
        match self {
            Self::Day => span.try_days(count),
            Self::Week => span.try_weeks(count),
            Self::Month => span.try_months(count),
            Self::Year => span.try_years(count),
        }
    }
}

/// A recurrence rule such as `every 2 weeks`.
///
/// # Examples
///
/// ```
/// use jiff::civil::date;
/// use rudo::domain::recurrence::Recurrence;
///
/// let rule: Recurrence = "Every 1 Month".parse()?;
/// assert_eq!(rule.to_string(), "every month");
/// assert_eq!(rule.next_after(date(2026, 1, 31))?, date(2026, 2, 28));
/// # Ok::<(), rudo::domain::recurrence::RecurrenceError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Recurrence {
    interval: NonZeroU32,
    unit: RecurrenceUnit,
}

impl Recurrence {
    /// Creates a rule that repeats every `interval` `unit`s.
    #[must_use]
    pub const fn new(interval: NonZeroU32, unit: RecurrenceUnit) -> Self {
        Self { interval, unit }
    }

    /// Returns the date of the occurrence that follows the one on `base`.
    ///
    /// Months and years are added in calendar terms; a day that does not exist in the target
    /// month is clamped to its last day (see the [module documentation](self)).
    ///
    /// # Errors
    ///
    /// Returns [`RecurrenceError::DateOutOfRange`] if the result lies outside the range of
    /// [`Date`].
    pub fn next_after(self, base: Date) -> Result<Date, RecurrenceError> {
        self.unit
            .span(self.interval)
            .and_then(|span| base.checked_add(span))
            .map_err(RecurrenceError::DateOutOfRange)
    }
}

impl fmt::Display for Recurrence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let unit = self.unit.singular();
        match self.interval.get() {
            1 => write!(formatter, "{EVERY} {unit}"),
            interval => write!(formatter, "{EVERY} {interval} {unit}s"),
        }
    }
}

impl FromStr for Recurrence {
    type Err = RecurrenceError;

    /// Parses `every [N] <unit>`, where the unit may be singular or plural.
    fn from_str(rule: &str) -> Result<Self, Self::Err> {
        let mut words = rule.split_whitespace();
        if !words
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case(EVERY))
        {
            return Err(RecurrenceError::MissingEvery);
        }
        let mut word = words.next().ok_or(RecurrenceError::Unsupported)?;
        let interval = if word.starts_with(|first: char| first.is_ascii_digit()) {
            let interval = parse_interval(word)?;
            word = words.next().ok_or(RecurrenceError::Unsupported)?;
            interval
        } else {
            NonZeroU32::MIN
        };
        let unit = RecurrenceUnit::from_word(word).ok_or(RecurrenceError::Unsupported)?;
        if words.next().is_some() {
            return Err(RecurrenceError::Unsupported);
        }
        Ok(Self::new(interval, unit))
    }
}

/// Parses the interval of a rule from a word that starts with a digit.
fn parse_interval(word: &str) -> Result<NonZeroU32, RecurrenceError> {
    word.parse()
        .map_err(|error: ParseIntError| match error.kind() {
            IntErrorKind::Zero => RecurrenceError::ZeroInterval,
            IntErrorKind::PosOverflow => RecurrenceError::IntervalOutOfRange,
            _ => RecurrenceError::Unsupported,
        })
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use proptest::prelude::{Strategy, any, prop_assert, prop_assert_eq, proptest};
    use proptest::sample::select;
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    fn recurrence(interval: u32, unit: RecurrenceUnit) -> Recurrence {
        Recurrence::new(NonZeroU32::new(interval).unwrap(), unit)
    }

    fn any_unit() -> impl Strategy<Value = RecurrenceUnit> {
        select(RecurrenceUnit::iter().collect::<Vec<_>>())
    }

    fn any_date() -> impl Strategy<Value = Date> {
        (-9999_i16..=9999, 1_i8..=12, 1_i8..=31)
            .prop_filter_map("invalid day of month", |(year, month, day)| {
                Date::new(year, month, day).ok()
            })
    }

    #[rstest]
    #[case::day("every day", recurrence(1, RecurrenceUnit::Day))]
    #[case::week("every week", recurrence(1, RecurrenceUnit::Week))]
    #[case::two_weeks("every 2 weeks", recurrence(2, RecurrenceUnit::Week))]
    #[case::month("every month", recurrence(1, RecurrenceUnit::Month))]
    #[case::three_months("every 3 months", recurrence(3, RecurrenceUnit::Month))]
    #[case::year("every year", recurrence(1, RecurrenceUnit::Year))]
    #[case::upper_case("EVERY Day", recurrence(1, RecurrenceUnit::Day))]
    #[case::mixed_case("Every 2 WEEKS", recurrence(2, RecurrenceUnit::Week))]
    #[case::any_whitespace("  every\t2 \n weeks  ", recurrence(2, RecurrenceUnit::Week))]
    #[case::plural_without_number("every days", recurrence(1, RecurrenceUnit::Day))]
    #[case::singular_with_number("every 2 week", recurrence(2, RecurrenceUnit::Week))]
    #[case::explicit_one("every 1 year", recurrence(1, RecurrenceUnit::Year))]
    #[case::leading_zero("every 07 days", recurrence(7, RecurrenceUnit::Day))]
    #[case::largest_interval("every 4294967295 days", recurrence(u32::MAX, RecurrenceUnit::Day))]
    fn from_str_parses_supported_rules(#[case] rule: &str, #[case] expected: Recurrence) {
        let system_under_test = rule.parse::<Recurrence>();

        assert_eq!(system_under_test.unwrap(), expected);
    }

    #[test]
    fn from_str_accepts_singular_and_plural_of_every_unit() {
        let rules: Vec<(String, Recurrence)> = RecurrenceUnit::iter()
            .flat_map(|unit| {
                let singular = format!("{unit:?}");
                [
                    (format!("every {singular}"), recurrence(1, unit)),
                    (format!("every 4 {singular}s"), recurrence(4, unit)),
                ]
            })
            .collect();

        let system_under_test: Vec<Recurrence> = rules
            .iter()
            .map(|(rule, _)| rule.parse::<Recurrence>().unwrap())
            .collect();

        assert_eq!(
            system_under_test,
            rules
                .iter()
                .map(|(_, expected)| *expected)
                .collect::<Vec<_>>()
        );
    }

    #[rstest]
    #[case::empty("")]
    #[case::blank("   ")]
    #[case::other_word("daily")]
    #[case::number_first("2 weeks")]
    fn from_str_rejects_rules_without_every(#[case] rule: &str) {
        let system_under_test = rule.parse::<Recurrence>();

        assert!(matches!(
            system_under_test,
            Err(RecurrenceError::MissingEvery)
        ));
    }

    #[rstest]
    #[case::weekday("every weekday")]
    #[case::week_on_monday("every week on Monday")]
    #[case::when_done("every day when done")]
    #[case::day_of_week("every Monday")]
    #[case::month_on_day("every month on the 15th")]
    #[case::missing_unit("every")]
    #[case::number_without_unit("every 2")]
    #[case::unknown_unit("every 2 fortnights")]
    #[case::hour("every hour")]
    #[case::malformed_number("every 2x days")]
    #[case::negative_number("every -1 days")]
    #[case::plural_twice("every dayss")]
    fn from_str_rejects_unsupported_rules(#[case] rule: &str) {
        let system_under_test = rule.parse::<Recurrence>();

        assert!(matches!(
            system_under_test,
            Err(RecurrenceError::Unsupported)
        ));
    }

    #[rstest]
    #[case::zero("every 0 days")]
    #[case::zeros("every 000 weeks")]
    fn from_str_rejects_zero_interval(#[case] rule: &str) {
        let system_under_test = rule.parse::<Recurrence>();

        assert!(matches!(
            system_under_test,
            Err(RecurrenceError::ZeroInterval)
        ));
    }

    #[test]
    fn from_str_rejects_interval_beyond_u32() {
        let rule = "every 4294967296 days";

        let system_under_test = rule.parse::<Recurrence>();

        assert!(matches!(
            system_under_test,
            Err(RecurrenceError::IntervalOutOfRange)
        ));
    }

    #[rstest]
    #[case::day(recurrence(1, RecurrenceUnit::Day), "every day")]
    #[case::week(recurrence(1, RecurrenceUnit::Week), "every week")]
    #[case::two_weeks(recurrence(2, RecurrenceUnit::Week), "every 2 weeks")]
    #[case::month(recurrence(1, RecurrenceUnit::Month), "every month")]
    #[case::three_months(recurrence(3, RecurrenceUnit::Month), "every 3 months")]
    #[case::year(recurrence(1, RecurrenceUnit::Year), "every year")]
    #[case::ten_years(recurrence(10, RecurrenceUnit::Year), "every 10 years")]
    fn display_is_canonical(#[case] rule: Recurrence, #[case] expected: &str) {
        let system_under_test = rule.to_string();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::day(date(2026, 11, 22), "every day", date(2026, 11, 23))]
    #[case::day_into_leap_day(date(2024, 2, 28), "every day", date(2024, 2, 29))]
    #[case::day_into_new_year(date(2026, 12, 31), "every day", date(2027, 1, 1))]
    #[case::week(date(2026, 11, 22), "every week", date(2026, 11, 29))]
    #[case::two_weeks(date(2026, 11, 22), "every 2 weeks", date(2026, 12, 6))]
    #[case::month(date(2026, 11, 22), "every month", date(2026, 12, 22))]
    #[case::month_end_common_year(date(2026, 1, 31), "every month", date(2026, 2, 28))]
    #[case::month_end_leap_year(date(2024, 1, 31), "every month", date(2024, 2, 29))]
    #[case::month_end_thirty_days(date(2026, 3, 31), "every month", date(2026, 4, 30))]
    #[case::month_end_drifts(date(2026, 2, 28), "every month", date(2026, 3, 28))]
    #[case::three_months_end(date(2026, 1, 31), "every 3 months", date(2026, 4, 30))]
    #[case::month_into_new_year(date(2026, 12, 15), "every month", date(2027, 1, 15))]
    #[case::year(date(2026, 11, 22), "every year", date(2027, 11, 22))]
    #[case::leap_day_to_common_year(date(2024, 2, 29), "every year", date(2025, 2, 28))]
    #[case::leap_day_to_leap_year(date(2024, 2, 29), "every 4 years", date(2028, 2, 29))]
    #[case::leap_day_to_century(date(2096, 2, 29), "every 4 years", date(2100, 2, 28))]
    #[case::leap_day_to_400_years(date(1996, 2, 29), "every 4 years", date(2000, 2, 29))]
    fn next_after_adds_interval(#[case] base: Date, #[case] rule: &str, #[case] expected: Date) {
        let rule: Recurrence = rule.parse().unwrap();

        let system_under_test = rule.next_after(base);

        assert_eq!(system_under_test.unwrap(), expected);
    }

    #[rstest]
    #[case::last_date(date(9999, 12, 31), recurrence(1, RecurrenceUnit::Day))]
    #[case::huge_interval(date(2026, 1, 1), recurrence(u32::MAX, RecurrenceUnit::Year))]
    fn next_after_rejects_dates_out_of_range(#[case] base: Date, #[case] rule: Recurrence) {
        let system_under_test = rule.next_after(base);

        assert!(matches!(
            system_under_test,
            Err(RecurrenceError::DateOutOfRange(_))
        ));
    }

    proptest! {
        #[test]
        fn display_then_from_str_round_trips(interval in any::<NonZeroU32>(), unit in any_unit()) {
            let rule = Recurrence::new(interval, unit);

            let system_under_test = rule.to_string().parse::<Recurrence>();

            prop_assert_eq!(system_under_test.unwrap(), rule);
        }

        #[test]
        fn next_after_is_later_than_base(
            base in any_date(),
            interval in 1_u32..=1000,
            unit in any_unit(),
        ) {
            let rule = recurrence(interval, unit);

            let system_under_test = rule.next_after(base);

            prop_assert!(system_under_test.map_or(true, |next| next > base));
        }
    }
}
