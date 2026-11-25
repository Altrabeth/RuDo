//! Date expressions, shared by the due date prompt and the query terms of the search, and the
//! display of due dates relative to today.
//!
//! [`parse_due`] turns what the user types into a [`Date`], [`due_keywords`] lists the words it
//! understands, [`DueClass`] buckets a due date by urgency and [`RelativeDue`] displays it
//! relative to today. Today is always a parameter, so every function here is pure.

use std::fmt;

use jiff::Span;
use jiff::civil::{Date, Weekday};

/// The largest number of days until a due date that still counts as [`DueClass::Soon`].
const SOON_DAYS: i32 = 7;

/// The keyword that clears the due date.
const NONE: &str = "none";

/// Keywords, their abbreviation and the number of days they lie after today.
const DAY_KEYWORDS: [(&str, Option<&str>, i8); 3] = [
    ("today", Some("tod"), 0),
    ("tomorrow", Some("tom"), 1),
    ("yesterday", None, -1),
];

/// Abbreviated and full weekday names.
const WEEKDAY_NAMES: [(&str, &str, Weekday); 7] = [
    ("mon", "monday", Weekday::Monday),
    ("tue", "tuesday", Weekday::Tuesday),
    ("wed", "wednesday", Weekday::Wednesday),
    ("thu", "thursday", Weekday::Thursday),
    ("fri", "friday", Weekday::Friday),
    ("sat", "saturday", Weekday::Saturday),
    ("sun", "sunday", Weekday::Sunday),
];

/// Errors that occur while parsing a due date expression.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum DateExprError {
    /// The input is none of the supported expressions.
    #[error("unknown date expression `{input}`")]
    Unrecognized {
        /// The trimmed input.
        input: String,
    },
    /// The input has the form `YYYY-MM-DD` but names a day that does not exist.
    #[error("invalid date `{input}`")]
    InvalidDate {
        /// The trimmed input.
        input: String,
        /// Why the date does not exist.
        #[source]
        source: jiff::Error,
    },
    /// The resulting date lies outside the supported range of years -9999 to 9999.
    #[error("date is out of range")]
    OutOfRange,
}

/// Parses a due date expression typed by the user, relative to `today`.
///
/// The input is trimmed and matched case-insensitively. Returns `Ok(None)` for an empty input
/// or `none`, which clear the due date, and otherwise the date the expression names:
///
/// - `YYYY-MM-DD`: that date,
/// - `today` or `tod`, `tomorrow` or `tom`, `yesterday`,
/// - `+Nd`, `+Nw`, `+Nm`, `+Ny`: `N` days, weeks, months or years after today; the `+` is
///   optional. Adding months or years clamps to the end of the month, so one month after
///   January 31 is the last day of February,
/// - `-Nd`, `-Nw`, `-Nm`, `-Ny`: `N` days, weeks, months or years before today, clamped in
///   the same way,
/// - a weekday (`mon` or `monday`, ...): the next such day strictly after today, so `fri` on a
///   Friday means the Friday one week later.
///
/// # Errors
///
/// Returns [`DateExprError::Unrecognized`] if the input is none of the expressions above,
/// [`DateExprError::InvalidDate`] for a `YYYY-MM-DD` date that does not exist, such as
/// `2026-02-30`, and [`DateExprError::OutOfRange`] if the date lies beyond the supported
/// range, such as `+99999y`.
///
/// # Examples
///
/// ```
/// use jiff::civil::date;
/// use rudo::domain::date::{DateExprError, parse_due};
///
/// let today = date(2026, 11, 24);
/// assert_eq!(parse_due("+3d", today)?, Some(date(2026, 11, 27)));
/// assert_eq!(parse_due("-1w", today)?, Some(date(2026, 11, 17)));
/// assert_eq!(parse_due(" Fri ", today)?, Some(date(2026, 11, 27)));
/// assert_eq!(parse_due("none", today)?, None);
/// # Ok::<(), DateExprError>(())
/// ```
pub fn parse_due(input: &str, today: Date) -> Result<Option<Date>, DateExprError> {
    let expression = input.trim();
    if expression.is_empty() || expression.eq_ignore_ascii_case(NONE) {
        return Ok(None);
    }
    resolve(expression, today).map(Some)
}

/// Returns the words of the due date expressions that [`parse_due`] understands, in full:
/// `today`, `tomorrow`, `yesterday`, `none` and the weekdays from `monday` to `sunday`.
///
/// Abbreviations such as `tod` and `fri` are left out, since completing a word to its
/// abbreviation would not help. Every word is an expression on its own.
///
/// # Examples
///
/// ```
/// use rudo::domain::date::due_keywords;
///
/// let words: Vec<&str> = due_keywords().filter(|word| word.starts_with('t')).collect();
/// assert_eq!(words, ["today", "tomorrow", "tuesday", "thursday"]);
/// ```
pub fn due_keywords() -> impl Iterator<Item = &'static str> {
    DAY_KEYWORDS
        .iter()
        .map(|&(keyword, _, _)| keyword)
        .chain([NONE])
        .chain(WEEKDAY_NAMES.iter().map(|&(_, full_name, _)| full_name))
}

/// Resolves a trimmed, non-empty expression that does not clear the date.
fn resolve(expression: &str, today: Date) -> Result<Date, DateExprError> {
    if let Some(days) = day_keyword(expression) {
        return add_offset(today, days, OffsetUnit::Day);
    }
    if let Some(weekday) = weekday_named(expression) {
        return today
            .nth_weekday(1, weekday)
            .map_err(|_range_error| DateExprError::OutOfRange);
    }
    if has_iso_date_shape(expression) {
        return expression
            .parse()
            .map_err(|source| DateExprError::InvalidDate {
                input: expression.to_owned(),
                source,
            });
    }
    if let Some((signed_count, unit)) = split_offset(expression) {
        // The count is validated, so parsing fails only when it overflows.
        let count: i64 = signed_count
            .parse()
            .map_err(|_overflow| DateExprError::OutOfRange)?;
        return add_offset(today, count, unit);
    }
    Err(DateExprError::Unrecognized {
        input: expression.to_owned(),
    })
}

/// Returns the day offset of a keyword such as `tomorrow`.
fn day_keyword(expression: &str) -> Option<i8> {
    DAY_KEYWORDS
        .iter()
        .find(|(keyword, abbreviation, _)| {
            expression.eq_ignore_ascii_case(keyword)
                || abbreviation
                    .is_some_and(|abbreviation| expression.eq_ignore_ascii_case(abbreviation))
        })
        .map(|&(_, _, days)| days)
}

/// Returns the weekday an abbreviated or full weekday name refers to.
fn weekday_named(expression: &str) -> Option<Weekday> {
    WEEKDAY_NAMES
        .iter()
        .find(|(abbreviation, full_name, _)| {
            expression.eq_ignore_ascii_case(abbreviation)
                || expression.eq_ignore_ascii_case(full_name)
        })
        .map(|&(_, _, weekday)| weekday)
}

/// Returns whether the expression has exactly the form `YYYY-MM-DD`.
///
/// Checked before parsing because jiff also accepts other forms, such as `20261224` or a date
/// with a time of day.
fn has_iso_date_shape(expression: &str) -> bool {
    let bytes = expression.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, &byte)| match index {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// Splits an offset expression such as `+3d` or `-1m` into its count with an optional sign,
/// which parses as an integer, and its unit.
fn split_offset(expression: &str) -> Option<(&str, OffsetUnit)> {
    let (unit_index, unit_suffix) = expression.char_indices().next_back()?;
    let unit = OffsetUnit::from_suffix(unit_suffix)?;
    let signed_count = &expression[..unit_index];
    let digits = signed_count
        .strip_prefix(['+', '-'])
        .unwrap_or(signed_count);
    let is_count = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
    is_count.then_some((signed_count, unit))
}

/// Adds `count` units to `today`, failing if the result is out of range.
fn add_offset(today: Date, count: impl Into<i64>, unit: OffsetUnit) -> Result<Date, DateExprError> {
    // Both jiff errors can only mean that a span or a date is out of range.
    unit.span(count.into())
        .and_then(|span| today.checked_add(span))
        .map_err(|_range_error| DateExprError::OutOfRange)
}

/// The unit of a relative offset such as `+3d`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OffsetUnit {
    Day,
    Week,
    Month,
    Year,
}

impl OffsetUnit {
    /// Returns the unit a suffix letter stands for, ignoring case.
    fn from_suffix(suffix: char) -> Option<Self> {
        match suffix.to_ascii_lowercase() {
            'd' => Some(Self::Day),
            'w' => Some(Self::Week),
            'm' => Some(Self::Month),
            'y' => Some(Self::Year),
            _ => None,
        }
    }

    /// Returns a span of `count` of this unit.
    fn span(self, count: i64) -> Result<Span, jiff::Error> {
        let span = Span::new();
        match self {
            Self::Day => span.try_days(count),
            Self::Week => span.try_weeks(count),
            Self::Month => span.try_months(count),
            Self::Year => span.try_years(count),
        }
    }
}

/// How urgent a due date is relative to today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DueClass {
    /// Due the given number of days ago, at least one.
    Overdue(u32),
    /// Due today.
    Today,
    /// Due tomorrow.
    Tomorrow,
    /// Due in the given number of days, from two to seven.
    Soon(u32),
    /// Due in more than seven days.
    Later,
}

impl DueClass {
    /// Classifies the date `due` relative to `today`.
    ///
    /// # Examples
    ///
    /// ```
    /// use jiff::civil::date;
    /// use rudo::domain::date::DueClass;
    ///
    /// let today = date(2026, 11, 24);
    /// assert_eq!(DueClass::classify(date(2026, 11, 22), today), DueClass::Overdue(2));
    /// assert_eq!(DueClass::classify(date(2026, 11, 27), today), DueClass::Soon(3));
    /// ```
    #[must_use]
    pub fn classify(due: Date, today: Date) -> Self {
        // Subtracting two dates yields a span in days and never panics.
        let days_until_due = (due - today).get_days();
        match days_until_due {
            ..0 => Self::Overdue(days_until_due.unsigned_abs()),
            0 => Self::Today,
            1 => Self::Tomorrow,
            2..=SOON_DAYS => Self::Soon(days_until_due.unsigned_abs()),
            _ => Self::Later,
        }
    }
}

/// A due date displayed relative to today, as shown in the tasks pane.
///
/// Displays `today`, `tomorrow`, `in 3d` for up to seven days ahead and `2d overdue` for any
/// past date. Dates further ahead are shown as `Dec 24` within the current year and as
/// `2027-01-05` otherwise.
///
/// # Examples
///
/// ```
/// use jiff::civil::date;
/// use rudo::domain::date::RelativeDue;
///
/// let today = date(2026, 11, 24);
/// assert_eq!(RelativeDue::new(date(2026, 11, 27), today).to_string(), "in 3d");
/// assert_eq!(RelativeDue::new(date(2026, 12, 24), today).to_string(), "Dec 24");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RelativeDue {
    due: Date,
    today: Date,
}

impl RelativeDue {
    /// Creates the display of the date `due` relative to `today`.
    #[must_use]
    pub const fn new(due: Date, today: Date) -> Self {
        Self { due, today }
    }
}

impl fmt::Display for RelativeDue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match DueClass::classify(self.due, self.today) {
            DueClass::Overdue(days) => write!(formatter, "{days}d overdue"),
            DueClass::Today => formatter.write_str("today"),
            DueClass::Tomorrow => formatter.write_str("tomorrow"),
            DueClass::Soon(days) => write!(formatter, "in {days}d"),
            DueClass::Later if self.due.year() == self.today.year() => {
                write!(formatter, "{}", self.due.strftime("%b %-d"))
            }
            DueClass::Later => write!(formatter, "{}", self.due),
        }
    }
}

#[cfg(test)]
mod tests {
    use jiff::ToSpan;
    use jiff::civil::date;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    /// A Tuesday.
    const TODAY: Date = date(2026, 11, 24);

    #[rstest]
    #[case::empty("")]
    #[case::whitespace("   ")]
    #[case::none("none")]
    #[case::none_upper_case(" NONE ")]
    fn parse_due_clears_the_date(#[case] input: &str) {
        let system_under_test = parse_due(input, TODAY);

        assert_eq!(system_under_test.unwrap(), None);
    }

    #[rstest]
    #[case::iso("2026-12-24", date(2026, 12, 24))]
    #[case::iso_in_the_past("2025-01-05", date(2025, 1, 5))]
    #[case::iso_leap_day("2028-02-29", date(2028, 2, 29))]
    #[case::today("today", TODAY)]
    #[case::tod("tod", TODAY)]
    #[case::tomorrow("tomorrow", date(2026, 11, 25))]
    #[case::tom("tom", date(2026, 11, 25))]
    #[case::yesterday("yesterday", date(2026, 11, 23))]
    #[case::days("+3d", date(2026, 11, 27))]
    #[case::days_without_plus("3d", date(2026, 11, 27))]
    #[case::zero_days("+0d", TODAY)]
    #[case::weeks("+2w", date(2026, 12, 8))]
    #[case::months("+1m", date(2026, 12, 24))]
    #[case::years("+1y", date(2027, 11, 24))]
    #[case::multi_digit("+100d", date(2027, 3, 4))]
    #[case::trimmed_and_upper_case("  ToMorrow\t", date(2026, 11, 25))]
    #[case::upper_case_unit("+3D", date(2026, 11, 27))]
    #[case::days_back("-3d", date(2026, 11, 21))]
    #[case::weeks_back("-1w", date(2026, 11, 17))]
    #[case::months_back("-1m", date(2026, 10, 24))]
    #[case::years_back("-2y", date(2024, 11, 24))]
    #[case::zero_days_back("-0d", TODAY)]
    fn parse_due_accepts_expressions(#[case] input: &str, #[case] expected: Date) {
        let system_under_test = parse_due(input, TODAY);

        assert_eq!(system_under_test.unwrap(), Some(expected));
    }

    #[rstest]
    #[case::wed("wed", date(2026, 11, 25))]
    #[case::wednesday("wednesday", date(2026, 11, 25))]
    #[case::thu("thu", date(2026, 11, 26))]
    #[case::thursday("thursday", date(2026, 11, 26))]
    #[case::fri("fri", date(2026, 11, 27))]
    #[case::friday("Friday", date(2026, 11, 27))]
    #[case::sat("sat", date(2026, 11, 28))]
    #[case::saturday("saturday", date(2026, 11, 28))]
    #[case::sun("sun", date(2026, 11, 29))]
    #[case::sunday("sunday", date(2026, 11, 29))]
    #[case::mon("mon", date(2026, 11, 30))]
    #[case::monday("MONDAY", date(2026, 11, 30))]
    #[case::same_weekday_means_next_week("tue", date(2026, 12, 1))]
    #[case::same_weekday_full_name("tuesday", date(2026, 12, 1))]
    fn parse_due_resolves_weekdays_strictly_after_today(
        #[case] input: &str,
        #[case] expected: Date,
    ) {
        let system_under_test = parse_due(input, TODAY);

        assert_eq!(system_under_test.unwrap(), Some(expected));
    }

    #[rstest]
    #[case::jan_31_to_feb_28(date(2026, 1, 31), "+1m", date(2026, 2, 28))]
    #[case::jan_31_to_leap_feb_29(date(2028, 1, 31), "+1m", date(2028, 2, 29))]
    #[case::leap_day_plus_a_year(date(2028, 2, 29), "+1y", date(2029, 2, 28))]
    #[case::mar_31_back_to_feb_28(date(2026, 3, 31), "-1m", date(2026, 2, 28))]
    fn parse_due_clamps_to_the_end_of_the_month(
        #[case] today: Date,
        #[case] input: &str,
        #[case] expected: Date,
    ) {
        let system_under_test = parse_due(input, today);

        assert_eq!(system_under_test.unwrap(), Some(expected));
    }

    #[rstest]
    #[case::word("someday")]
    #[case::unknown_unit("+3x")]
    #[case::missing_count("+d")]
    #[case::double_plus("++3d")]
    #[case::double_minus("--3d")]
    #[case::inner_sign("+-3d")]
    #[case::inner_plus("-+3d")]
    #[case::minus_only("-")]
    #[case::missing_count_back("-d")]
    #[case::space_in_offset("+3 d")]
    #[case::plus_only("+")]
    #[case::partial_iso("2026-12")]
    #[case::iso_with_time("2026-12-24T10:00")]
    #[case::compact_iso("20261224")]
    #[case::weekday_prefix("fr")]
    fn parse_due_rejects_unknown_expressions(#[case] input: &str) {
        let expected_input = input.trim();

        let system_under_test = parse_due(input, TODAY);

        assert!(matches!(
            system_under_test,
            Err(DateExprError::Unrecognized { input }) if input == expected_input
        ));
    }

    #[rstest]
    #[case::february_30("2026-02-30")]
    #[case::month_13("2026-13-01")]
    #[case::no_leap_day("2027-02-29")]
    fn parse_due_rejects_impossible_dates(#[case] input: &str) {
        let system_under_test = parse_due(input, TODAY);

        assert!(matches!(
            system_under_test,
            Err(DateExprError::InvalidDate { .. })
        ));
    }

    #[rstest]
    #[case::huge_years("+99999y")]
    #[case::huge_days("+99999999d")]
    #[case::years_past_the_maximum("+9000y")]
    #[case::count_overflows_i64("+99999999999999999999999d")]
    #[case::huge_years_back("-99999y")]
    #[case::count_back_overflows_i64("-99999999999999999999999d")]
    fn parse_due_rejects_dates_out_of_range(#[case] input: &str) {
        let system_under_test = parse_due(input, TODAY);

        assert!(matches!(system_under_test, Err(DateExprError::OutOfRange)));
    }

    #[test]
    fn parse_due_rejects_weekday_after_the_last_date() {
        let today = Date::MAX;

        let system_under_test = parse_due("mon", today);

        assert!(matches!(system_under_test, Err(DateExprError::OutOfRange)));
    }

    #[rstest]
    #[case::overdue_long_ago(-400, DueClass::Overdue(400))]
    #[case::overdue_one_day(-1, DueClass::Overdue(1))]
    #[case::today(0, DueClass::Today)]
    #[case::tomorrow(1, DueClass::Tomorrow)]
    #[case::soon_two_days(2, DueClass::Soon(2))]
    #[case::soon_seven_days(7, DueClass::Soon(7))]
    #[case::later_eight_days(8, DueClass::Later)]
    fn classify_buckets_days_until_due(#[case] days_until_due: i32, #[case] expected: DueClass) {
        let due = TODAY.checked_add(days_until_due.days()).unwrap();

        let system_under_test = DueClass::classify(due, TODAY);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::today(TODAY, "today")]
    #[case::tomorrow(date(2026, 11, 25), "tomorrow")]
    #[case::soon(date(2026, 11, 27), "in 3d")]
    #[case::soon_seven_days(date(2026, 12, 1), "in 7d")]
    #[case::overdue(date(2026, 11, 22), "2d overdue")]
    #[case::overdue_long_ago(date(2025, 11, 24), "365d overdue")]
    #[case::later_same_year(date(2026, 12, 24), "Dec 24")]
    #[case::later_single_digit_day(date(2026, 12, 5), "Dec 5")]
    #[case::later_next_year(date(2027, 1, 5), "2027-01-05")]
    fn relative_due_formats_relative_to_today(#[case] due: Date, #[case] expected: &str) {
        let system_under_test = RelativeDue::new(due, TODAY);

        assert_eq!(system_under_test.to_string(), expected);
    }

    #[test]
    fn due_keywords_are_the_full_words_in_grammar_order() {
        let expected = [
            "today",
            "tomorrow",
            "yesterday",
            "none",
            "monday",
            "tuesday",
            "wednesday",
            "thursday",
            "friday",
            "saturday",
            "sunday",
        ];

        let system_under_test: Vec<&str> = due_keywords().collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn every_due_keyword_parses() {
        let keywords: Vec<&str> = due_keywords().collect();

        let system_under_test: Vec<&str> = keywords
            .iter()
            .copied()
            .filter(|keyword| parse_due(keyword, TODAY).is_err())
            .collect();

        assert_eq!(system_under_test, Vec::<&str>::new());
    }

    fn four_digit_year_date() -> impl Strategy<Value = Date> {
        let first = date(0, 1, 1);
        let span_in_days = (date(9999, 12, 31) - first).get_days();
        (0..=span_in_days).prop_map(move |offset| first.checked_add(offset.days()).unwrap())
    }

    proptest! {
        #[test]
        fn parse_due_round_trips_iso_dates(due in four_digit_year_date()) {
            let system_under_test = parse_due(&due.to_string(), TODAY);

            prop_assert_eq!(system_under_test.unwrap(), Some(due));
        }
    }
}
