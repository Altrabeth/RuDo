//! The theme: one style per interface element ("slot"), overridable in the configuration.
//!
//! Widgets never hard-code a colour; they ask [`Theme::style`] for the style of a [`Slot`].
//! The default styles only use the 16 ANSI colours, so they follow the terminal's palette.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::str::FromStr;

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};
use strum::{EnumCount, IntoEnumIterator};

/// An element of the interface that has its own style, named as in the `theme` section.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    strum::EnumCount,
    strum::EnumIter,
)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Slot {
    /// Default text.
    Text,
    /// Border of a pane without focus.
    Border,
    /// Border of the focused pane.
    BorderFocused,
    /// Title of a pane without focus.
    Title,
    /// Title of the focused pane.
    TitleFocused,
    /// Selected row in the focused pane, the help screen and the picker.
    Selection,
    /// Selected row in the pane without focus.
    SelectionUnfocused,
    /// Rows selected in visual mode.
    VisualSelection,
    /// Folder in the lists pane.
    Folder,
    /// List in the lists pane.
    List,
    /// `done/total` counter of a list.
    ListCount,
    /// Hidden folder or list, whose name starts with `.`, while hidden entries are shown.
    HiddenEntry,
    /// The row of the bin at the end of the lists pane.
    Bin,
    /// Indentation guides and fold markers.
    TreeGuide,
    /// Text of a done task.
    TaskDone,
    /// Text of a task in progress.
    TaskInProgress,
    /// Text of a cancelled task.
    TaskCancelled,
    /// Marker of the highest priority.
    PriorityHighest,
    /// Marker of the high priority.
    PriorityHigh,
    /// Marker of the medium priority.
    PriorityMedium,
    /// Marker of the low priority.
    PriorityLow,
    /// Marker of the lowest priority.
    PriorityLowest,
    /// Due date in the future.
    Due,
    /// Due date in the next few days.
    DueSoon,
    /// Due date today.
    DueToday,
    /// Due date in the past.
    DueOverdue,
    /// Tag.
    Tag,
    /// Recurrence rule.
    Recurrence,
    /// Bottom bar.
    StatusBar,
    /// Key in a key hint of the bottom bar.
    KeyHint,
    /// Description in a key hint of the bottom bar.
    KeyHintLabel,
    /// Keys typed so far of an unfinished key sequence.
    PendingKeys,
    /// Informational message in the bottom bar.
    MessageInfo,
    /// Error message in the bottom bar.
    MessageError,
    /// Border of a dialog.
    PopupBorder,
    /// Title of a dialog.
    PopupTitle,
    /// Error below a text input.
    InputError,
    /// Section heading of the help screen.
    HelpHeading,
    /// Key on the help screen.
    HelpKey,
    /// Action description on the help screen.
    HelpDescription,
    /// Search match, and the matched characters in the picker.
    SearchMatch,
    /// Parent kept visible as context while filtering.
    FilterContext,
}

impl Slot {
    /// Returns the built-in style of this slot.
    const fn default_style(self) -> StyleConfig {
        const NONE: Modifier = Modifier::empty();
        match self {
            Self::Text | Self::List | Self::StatusBar | Self::HelpDescription => StyleConfig::EMPTY,
            Self::Border
            | Self::ListCount
            | Self::TreeGuide
            | Self::FilterContext
            | Self::TaskDone
            | Self::PriorityLowest => StyleConfig::fg(Color::DarkGray, NONE),
            Self::TaskCancelled => StyleConfig::fg(Color::DarkGray, Modifier::CROSSED_OUT),
            Self::BorderFocused | Self::PopupBorder | Self::HelpKey | Self::Recurrence => {
                StyleConfig::fg(Color::Cyan, NONE)
            }
            Self::Title | Self::KeyHintLabel => StyleConfig::fg(Color::Gray, NONE),
            Self::TitleFocused | Self::PopupTitle | Self::KeyHint => {
                StyleConfig::fg(Color::Cyan, Modifier::BOLD)
            }
            Self::Selection | Self::VisualSelection => StyleConfig::modifiers(Modifier::REVERSED),
            Self::HiddenEntry => StyleConfig::modifiers(Modifier::DIM),
            Self::SelectionUnfocused => {
                StyleConfig::modifiers(Modifier::REVERSED.union(Modifier::DIM))
            }
            Self::Folder | Self::Due => StyleConfig::fg(Color::Blue, NONE),
            Self::TaskInProgress | Self::PriorityMedium | Self::DueSoon => {
                StyleConfig::fg(Color::Yellow, NONE)
            }
            Self::PriorityHighest | Self::DueOverdue | Self::MessageError => {
                StyleConfig::fg(Color::LightRed, Modifier::BOLD)
            }
            Self::PriorityHigh | Self::InputError => StyleConfig::fg(Color::LightRed, NONE),
            Self::PriorityLow | Self::MessageInfo => StyleConfig::fg(Color::Green, NONE),
            Self::DueToday | Self::PendingKeys | Self::HelpHeading => {
                StyleConfig::fg(Color::Yellow, Modifier::BOLD)
            }
            Self::Tag | Self::Bin => StyleConfig::fg(Color::Magenta, NONE),
            Self::SearchMatch => StyleConfig::fg(Color::Yellow, Modifier::REVERSED),
        }
    }
}

/// A style as written in the `theme` section: optional colours and a set of modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StyleConfig {
    #[serde(deserialize_with = "deserialize_colour")]
    fg: Option<Color>,
    #[serde(deserialize_with = "deserialize_colour")]
    bg: Option<Color>,
    #[serde(deserialize_with = "deserialize_modifiers")]
    modifiers: Modifier,
}

impl StyleConfig {
    const EMPTY: Self = Self::modifiers(Modifier::empty());

    const fn fg(fg: Color, modifiers: Modifier) -> Self {
        Self {
            fg: Some(fg),
            bg: None,
            modifiers,
        }
    }

    const fn modifiers(modifiers: Modifier) -> Self {
        Self {
            fg: None,
            bg: None,
            modifiers,
        }
    }

    fn to_style(self, colour_mode: ColourMode) -> Style {
        let style = Style::new().add_modifier(self.modifiers);
        match colour_mode {
            ColourMode::Colour => Style {
                fg: self.fg,
                bg: self.bg,
                ..style
            },
            ColourMode::NoColour => style,
        }
    }
}

/// Text modifiers as written in the `modifiers` list of a style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, strum::EnumIter)]
#[serde(rename_all = "kebab-case")]
enum ModifierName {
    Bold,
    Dim,
    Italic,
    Underlined,
    Reversed,
    CrossedOut,
}

impl From<ModifierName> for Modifier {
    fn from(name: ModifierName) -> Self {
        match name {
            ModifierName::Bold => Self::BOLD,
            ModifierName::Dim => Self::DIM,
            ModifierName::Italic => Self::ITALIC,
            ModifierName::Underlined => Self::UNDERLINED,
            ModifierName::Reversed => Self::REVERSED,
            ModifierName::CrossedOut => Self::CROSSED_OUT,
        }
    }
}

fn deserialize_modifiers<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Modifier, D::Error> {
    let names = Vec::<ModifierName>::deserialize(deserializer)?;
    Ok(names.into_iter().map(Modifier::from).collect())
}

fn deserialize_colour<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Color>, D::Error> {
    deserializer.deserialize_any(ColourVisitor).map(Some)
}

struct ColourVisitor;

impl Visitor<'_> for ColourVisitor {
    type Value = Color;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(EXPECTED_COLOUR)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Color::from_str(value).map_err(|_| {
            E::custom(format!(
                "invalid colour {value:?}, expected {EXPECTED_COLOUR}"
            ))
        })
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        u8::try_from(value).map(Color::Indexed).map_err(|_| {
            E::custom(format!(
                "invalid colour {value}, expected {EXPECTED_COLOUR}"
            ))
        })
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        u64::try_from(value)
            .map_err(|_| {
                E::custom(format!(
                    "invalid colour {value}, expected {EXPECTED_COLOUR}"
                ))
            })
            .and_then(|value| self.visit_u64(value))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Err(E::custom(MISSING_COLOUR))
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Err(E::custom(MISSING_COLOUR))
    }
}

const EXPECTED_COLOUR: &str = "a colour name, \"#rrggbb\" or a 256-colour index";

const MISSING_COLOUR: &str =
    "missing colour; quote hex colours (\"#rrggbb\") because `#` starts a YAML comment";

/// The `theme` section of the configuration: the slots whose default style is replaced.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(transparent)]
pub struct ThemeConfig(BTreeMap<Slot, StyleConfig>);

/// Whether colours are shown, decided by the `NO_COLOR` convention (<https://no-color.org>).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, strum::EnumIter)]
pub enum ColourMode {
    /// Colours and modifiers are shown.
    #[default]
    Colour,
    /// Only modifiers are shown; every colour is the terminal default.
    NoColour,
}

impl ColourMode {
    /// Returns the mode for the value of the `NO_COLOR` environment variable: colours are
    /// disabled if it is set and not empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::ffi::OsStr;
    ///
    /// use rudo::config::theme::ColourMode;
    ///
    /// assert_eq!(ColourMode::from_no_color(None), ColourMode::Colour);
    /// assert_eq!(ColourMode::from_no_color(Some(OsStr::new(""))), ColourMode::Colour);
    /// assert_eq!(ColourMode::from_no_color(Some(OsStr::new("1"))), ColourMode::NoColour);
    /// ```
    #[must_use]
    pub fn from_no_color(no_color: Option<&OsStr>) -> Self {
        match no_color {
            Some(value) if !value.is_empty() => Self::NoColour,
            _ => Self::Colour,
        }
    }
}

/// The effective style of every [`Slot`]: configured styles over the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Theme {
    styles: [Style; Slot::COUNT],
}

impl Theme {
    /// Builds the theme from the configured slots in `config`, using the default style for
    /// every other slot. With [`ColourMode::NoColour`] all colours are dropped and only the
    /// modifiers remain.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    ///
    /// use ratatui::style::{Color, Modifier, Style};
    /// use rudo::config::Config;
    /// use rudo::config::theme::{ColourMode, Slot, Theme};
    ///
    /// let yaml = "theme:\n  tag: { fg: red, modifiers: [bold] }\n";
    /// let config = Config::from_yaml(yaml, Path::new("config.yaml"))?;
    ///
    /// let theme = Theme::new(config.theme(), ColourMode::Colour);
    /// assert_eq!(theme.style(Slot::Tag), Style::new().fg(Color::Red).add_modifier(Modifier::BOLD));
    ///
    /// let theme = Theme::new(config.theme(), ColourMode::NoColour);
    /// assert_eq!(theme.style(Slot::Tag), Style::new().add_modifier(Modifier::BOLD));
    /// # Ok::<(), rudo::config::ConfigError>(())
    /// ```
    #[must_use]
    pub fn new(config: &ThemeConfig, colour_mode: ColourMode) -> Self {
        let mut styles = [Style::new(); Slot::COUNT];
        for slot in Slot::iter() {
            let style_config = config.0.get(&slot).copied();
            styles[slot as usize] = style_config
                .unwrap_or_else(|| slot.default_style())
                .to_style(colour_mode);
        }
        Self { styles }
    }

    /// Returns the style of `slot`.
    #[must_use]
    pub const fn style(&self, slot: Slot) -> Style {
        self.styles[slot as usize]
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(&ThemeConfig::default(), ColourMode::default())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::Path;

    use rstest::rstest;

    use super::*;
    use crate::config::{Config, ConfigError, DEFAULT_CONFIG_YAML};
    use crate::user_docs;

    fn parse_theme(yaml: &str) -> Result<ThemeConfig, ConfigError> {
        Config::from_yaml(yaml, Path::new("config.yaml")).map(|config| config.theme().clone())
    }

    fn parse_style(style_yaml: &str) -> Result<StyleConfig, ConfigError> {
        let yaml = format!("theme:\n  tag: {style_yaml}\n");
        parse_theme(&yaml).map(|theme| theme.0[&Slot::Tag])
    }

    fn report(result: Result<StyleConfig, ConfigError>) -> String {
        match result.unwrap_err() {
            ConfigError::Invalid { report, .. } => report,
            other => panic!("expected an invalid configuration, got {other:?}"),
        }
    }

    /// The entries of the commented `theme` section of the default configuration file, with
    /// the comment markers removed.
    fn documented_theme_yaml() -> String {
        DEFAULT_CONFIG_YAML
            .lines()
            .skip_while(|line| *line != "theme:")
            .skip(1)
            .take_while(|line| line.starts_with("  "))
            .filter_map(|line| line.strip_prefix("  # "))
            .filter(|entry| entry.contains(": {"))
            .flat_map(|entry| ["  ", entry, "\n"])
            .collect()
    }

    #[rstest]
    #[case::black("black", Color::Black)]
    #[case::red("red", Color::Red)]
    #[case::green("green", Color::Green)]
    #[case::yellow("yellow", Color::Yellow)]
    #[case::blue("blue", Color::Blue)]
    #[case::magenta("magenta", Color::Magenta)]
    #[case::cyan("cyan", Color::Cyan)]
    #[case::gray("gray", Color::Gray)]
    #[case::dark_gray("dark-gray", Color::DarkGray)]
    #[case::light_red("light-red", Color::LightRed)]
    #[case::light_green("light-green", Color::LightGreen)]
    #[case::light_yellow("light-yellow", Color::LightYellow)]
    #[case::light_blue("light-blue", Color::LightBlue)]
    #[case::light_magenta("light-magenta", Color::LightMagenta)]
    #[case::light_cyan("light-cyan", Color::LightCyan)]
    #[case::white("white", Color::White)]
    #[case::reset("reset", Color::Reset)]
    #[case::hex("\"#ff5500\"", Color::Rgb(0xff, 0x55, 0x00))]
    #[case::hex_upper_case("\"#FF5500\"", Color::Rgb(0xff, 0x55, 0x00))]
    #[case::quoted_index("\"238\"", Color::Indexed(238))]
    #[case::unquoted_index("238", Color::Indexed(238))]
    #[case::index_zero("0", Color::Indexed(0))]
    #[case::largest_index("255", Color::Indexed(255))]
    fn parses_every_documented_colour(#[case] colour: &str, #[case] expected: Color) {
        let style_yaml = format!("{{ fg: {colour}, bg: {colour} }}");

        let system_under_test = parse_style(&style_yaml).unwrap();

        assert_eq!(system_under_test.fg, Some(expected));
        assert_eq!(system_under_test.bg, Some(expected));
    }

    #[rstest]
    #[case::unknown_name("purple", "invalid colour \"purple\"")]
    #[case::short_hex("\"#fff\"", "invalid colour \"#fff\"")]
    #[case::bad_hex_digit("\"#gg0000\"", "invalid colour \"#gg0000\"")]
    #[case::quoted_index_too_large("\"256\"", "invalid colour \"256\"")]
    #[case::index_too_large("256", "invalid colour 256")]
    #[case::negative_index("-1", "invalid colour -1")]
    #[case::boolean("true", "a colour name, \"#rrggbb\" or a 256-colour index")]
    fn rejects_invalid_colour(#[case] colour: &str, #[case] expected_message: &str) {
        let style_yaml = format!("\n    fg: {colour}");

        let system_under_test = report(parse_style(&style_yaml));

        assert!(
            system_under_test.contains(expected_message),
            "{system_under_test}"
        );
        assert!(
            system_under_test.contains("config.yaml:3:"),
            "{system_under_test}"
        );
    }

    #[test]
    fn unquoted_hex_colour_hints_at_quoting() {
        let style_yaml = "\n    fg: #ff0000\n";
        let expected_hint = "quote hex colours";

        let system_under_test = report(parse_style(style_yaml));

        assert!(
            system_under_test.contains(expected_hint),
            "{system_under_test}"
        );
    }

    #[test]
    fn parses_every_modifier() {
        let style_yaml = "{ modifiers: [bold, dim, italic, underlined, reversed, crossed-out] }";
        let expected = Modifier::BOLD
            | Modifier::DIM
            | Modifier::ITALIC
            | Modifier::UNDERLINED
            | Modifier::REVERSED
            | Modifier::CROSSED_OUT;

        let system_under_test = parse_style(style_yaml).unwrap();

        assert_eq!(system_under_test.modifiers, expected);
    }

    #[test]
    fn modifier_names_map_to_distinct_modifiers() {
        let expected_count = ModifierName::iter().count();

        let system_under_test: HashSet<Modifier> =
            ModifierName::iter().map(Modifier::from).collect();

        assert_eq!(system_under_test.len(), expected_count);
        assert!(!system_under_test.contains(&Modifier::empty()));
    }

    #[rstest]
    #[case::unknown_modifier("{ modifiers: [blink] }", "unknown variant `blink`")]
    #[case::unknown_field("{ colour: red }", "unknown field `colour`")]
    fn rejects_unknown_style_settings(#[case] style_yaml: &str, #[case] expected_message: &str) {
        let system_under_test = report(parse_style(style_yaml));

        assert!(
            system_under_test.contains(expected_message),
            "{system_under_test}"
        );
    }

    #[test]
    fn unknown_slot_is_reported_with_location() {
        let yaml = "theme:\n  tag: {}\n  selected: { fg: red }\n";
        let expected_message = "unknown variant `selected`";
        let expected_location = "config.yaml:3:3";

        let system_under_test = report(parse_theme(yaml).map(|_| StyleConfig::EMPTY));

        assert!(
            system_under_test.contains(expected_message),
            "{system_under_test}"
        );
        assert!(
            system_under_test.contains(expected_location),
            "{system_under_test}"
        );
    }

    #[test]
    fn configured_slot_replaces_the_default_completely() {
        let config = parse_theme("theme:\n  selection: { fg: red }\n").unwrap();
        let expected = Style::new().fg(Color::Red);

        let system_under_test = Theme::new(&config, ColourMode::Colour);

        assert_eq!(system_under_test.style(Slot::Selection), expected);
    }

    #[test]
    fn unconfigured_slots_keep_their_default() {
        let config = parse_theme("theme:\n  selection: { fg: red }\n").unwrap();
        let defaults = Theme::default();

        let system_under_test = Theme::new(&config, ColourMode::Colour);

        for slot in Slot::iter().filter(|slot| *slot != Slot::Selection) {
            assert_eq!(
                system_under_test.style(slot),
                defaults.style(slot),
                "{slot:?}"
            );
        }
    }

    #[test]
    fn empty_style_is_plain_text() {
        let config = parse_theme("theme:\n  due_overdue: {}\n").unwrap();
        let expected = Style::new();

        let system_under_test = Theme::new(&config, ColourMode::Colour);

        assert_eq!(system_under_test.style(Slot::DueOverdue), expected);
    }

    #[test]
    fn default_styles_only_use_the_sixteen_ansi_colours() {
        let is_ansi =
            |colour: Option<Color>| !matches!(colour, Some(Color::Rgb(..) | Color::Indexed(_)));

        let system_under_test = Theme::default();

        for slot in Slot::iter() {
            let style = system_under_test.style(slot);
            assert!(
                is_ansi(style.fg) && is_ansi(style.bg),
                "{slot:?}: {style:?}"
            );
        }
    }

    #[test]
    fn default_selection_is_reversed() {
        let expected = Modifier::REVERSED;

        let system_under_test = Theme::default().style(Slot::Selection);

        assert!(system_under_test.add_modifier.contains(expected));
    }

    #[test]
    fn no_colour_keeps_modifiers_of_every_slot() {
        let config =
            parse_theme("theme:\n  tag: { fg: red, bg: \"#000000\", modifiers: [bold] }\n")
                .unwrap();
        let coloured = Theme::new(&config, ColourMode::Colour);

        let system_under_test = Theme::new(&config, ColourMode::NoColour);

        for slot in Slot::iter() {
            let style = system_under_test.style(slot);
            assert_eq!((style.fg, style.bg), (None, None), "{slot:?}");
            assert_eq!(
                style.add_modifier,
                coloured.style(slot).add_modifier,
                "{slot:?}"
            );
        }
    }

    #[rstest]
    #[case::unset(None, ColourMode::Colour)]
    #[case::empty(Some(""), ColourMode::Colour)]
    #[case::one(Some("1"), ColourMode::NoColour)]
    #[case::any_text(Some("false"), ColourMode::NoColour)]
    fn no_color_variable_disables_colours_when_set_and_not_empty(
        #[case] no_color: Option<&str>,
        #[case] expected: ColourMode,
    ) {
        let system_under_test = ColourMode::from_no_color(no_color.map(OsStr::new));

        assert_eq!(system_under_test, expected);
    }

    /// Returns the code spans in the first column of the theme table in `docs/configuration.md`
    /// with the header `first_header`.
    fn documented_theme_column(first_header: &str) -> Vec<&'static str> {
        user_docs::table_rows(&user_docs::CONFIGURATION.section("## Theme"), first_header)
            .into_iter()
            .flat_map(|cells| user_docs::code_spans(cells[0]))
            .collect()
    }

    #[test]
    fn documented_theme_table_lists_every_slot_in_order() {
        let expected: Vec<Slot> = Slot::iter().collect();

        let system_under_test: Vec<Slot> = documented_theme_column("Slot")
            .into_iter()
            .map(|name| serde_saphyr::from_str(name).unwrap())
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn documented_colour_table_lists_colours_that_parse() {
        let expected = [
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::Gray,
            Color::DarkGray,
            Color::LightRed,
            Color::LightGreen,
            Color::LightYellow,
            Color::LightBlue,
            Color::LightMagenta,
            Color::LightCyan,
            Color::White,
            Color::Reset,
            Color::Rgb(0xff, 0x55, 0x55),
            Color::Indexed(238),
        ];

        let system_under_test: Vec<Color> = documented_theme_column("Colour")
            .into_iter()
            .map(|colour| {
                let style = parse_style(&format!("{{ fg: {colour} }}")).unwrap();
                style.fg.unwrap()
            })
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn default_config_file_documents_the_default_style_of_every_slot() {
        let yaml = format!("theme:\n{}", documented_theme_yaml());
        let expected: BTreeMap<Slot, StyleConfig> = Slot::iter()
            .map(|slot| (slot, slot.default_style()))
            .collect();

        let system_under_test = parse_theme(&yaml).unwrap();

        assert_eq!(system_under_test.0, expected);
    }
}
