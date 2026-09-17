use std::collections::HashMap;

use crossterm::style::{Attribute, Color, ContentStyle};
use kdl::{KdlDocument, KdlNode, KdlValue};
use thiserror::Error;

/// The style applied to one themable element.
/// Any field left unset falls back to the terminal's own default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Style {
    pub fn to_content_style(self) -> ContentStyle {
        let mut attributes = crossterm::style::Attributes::default();
        if self.bold {
            attributes.set(Attribute::Bold);
        }
        if self.italic {
            attributes.set(Attribute::Italic);
        }
        if self.underline {
            attributes.set(Attribute::Underlined);
        }
        ContentStyle {
            foreground_color: self.fg,
            background_color: self.bg,
            underline_color: None,
            attributes,
        }
    }
}

/// A theme. Every themable element is its own field, grouped into nested
/// structs that mirror the `.`-separated sections of the theme's KDL
/// document (e.g. `ui.status_bar` becomes `theme.ui.status_bar`). Adding a
/// new themable element means adding a field here, not a new string key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Theme {
    pub ui: UiTheme,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UiTheme {
    pub background: Style,
    pub status_bar: Style,
    pub popup: Style,
}

impl Theme {
    /// Looks up a theme built into the binary by name — the file's stem
    /// under the repo's `default_config/theme/` directory, e.g. `"monokai"`
    /// for `default_config/theme/monokai.kdl`. Returns `None` when no
    /// built-in theme has that name, so the caller can fall back to treating
    /// it as a path.
    pub fn built_in(name: &str) -> Option<Theme> {
        let file = crate::DEFAULT_CONFIG_DIR.get_file(format!("theme/{name}.kdl"))?;
        let source = file.contents_utf8().expect("built-in themes must be UTF-8");
        let document: KdlDocument = source.parse().expect("built-in theme must be valid kdl");
        Some(Theme::parse(&document).expect("built-in theme must parse cleanly"))
    }

    /// Parses a theme document: an optional `palette` block of named colors,
    /// followed by nested sections (`ui { status_bar { fg "..." bg "..." } }`).
    /// A `fg`/`bg` value is looked up in the palette first, and falls back to
    /// being parsed as a literal color (a `#rrggbb` hex value or a color
    /// name) when it isn't a palette entry.
    pub fn parse(document: &KdlDocument) -> Result<Theme, ThemeParseError> {
        let palette = match document.get("palette") {
            Some(node) => parse_palette(node)?,
            None => HashMap::new(),
        };

        let ui_section = document.get("ui").and_then(KdlNode::children);
        let ui = UiTheme {
            background: parse_named_style(ui_section, "ui.background", "background", &palette)?,
            status_bar: parse_named_style(ui_section, "ui.status_bar", "status_bar", &palette)?,
            popup: parse_named_style(ui_section, "ui.popup", "popup", &palette)?,
        };

        Ok(Theme { ui })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ThemeParseError {
    #[error("palette entry `{key}` is missing a color value")]
    MissingPaletteValue { key: String },
    #[error("`{key}` must be a color name or a `#rrggbb` hex value, found `{found}`")]
    InvalidColor { key: String, found: String },
}

fn parse_palette(node: &KdlNode) -> Result<HashMap<String, Color>, ThemeParseError> {
    let mut palette = HashMap::new();
    let Some(children) = node.children() else {
        return Ok(palette);
    };
    for entry in children.nodes() {
        let name = entry.name().value().to_string();
        let value = entry
            .entries()
            .first()
            .map(|e| e.value())
            .and_then(KdlValue::as_string)
            .ok_or_else(|| ThemeParseError::MissingPaletteValue { key: name.clone() })?;
        let color = parse_color(value).ok_or_else(|| ThemeParseError::InvalidColor {
            key: name.clone(),
            found: value.to_string(),
        })?;
        palette.insert(name, color);
    }
    Ok(palette)
}

/// Parses the `name` child of `section` (e.g. `status_bar` under `ui`) into a
/// [`Style`], or an empty style if `section` or that child is absent.
/// `full_key` is only used to identify the field in error messages.
fn parse_named_style(
    section: Option<&KdlDocument>,
    full_key: &str,
    name: &str,
    palette: &HashMap<String, Color>,
) -> Result<Style, ThemeParseError> {
    let Some(node) = section.and_then(|section| section.get(name)) else {
        return Ok(Style::default());
    };
    parse_style(full_key, node, palette)
}

fn parse_style(
    key: &str,
    node: &KdlNode,
    palette: &HashMap<String, Color>,
) -> Result<Style, ThemeParseError> {
    let mut style = Style::default();
    let Some(children) = node.children() else {
        return Ok(style);
    };
    if let Some(value) = children.get_arg("fg") {
        style.fg = Some(resolve_color(key, "fg", value, palette)?);
    }
    if let Some(value) = children.get_arg("bg") {
        style.bg = Some(resolve_color(key, "bg", value, palette)?);
    }
    style.bold = children
        .get_arg("bold")
        .and_then(KdlValue::as_bool)
        .unwrap_or(false);
    style.italic = children
        .get_arg("italic")
        .and_then(KdlValue::as_bool)
        .unwrap_or(false);
    style.underline = children
        .get_arg("underline")
        .and_then(KdlValue::as_bool)
        .unwrap_or(false);
    Ok(style)
}

fn resolve_color(
    key: &str,
    field: &str,
    value: &KdlValue,
    palette: &HashMap<String, Color>,
) -> Result<Color, ThemeParseError> {
    let error = || ThemeParseError::InvalidColor {
        key: format!("{key}.{field}"),
        found: value.to_string(),
    };
    let raw = value.as_string().ok_or_else(error)?;
    if let Some(color) = palette.get(raw) {
        return Ok(*color);
    }
    parse_color(raw).ok_or_else(error)
}

/// Parses either a `#rrggbb` hex value or one of crossterm's named colors.
fn parse_color(raw: &str) -> Option<Color> {
    if let Some(hex) = raw.strip_prefix('#') {
        let [r, g, b] = [0..2, 2..4, 4..6].map(|range| {
            hex.get(range)
                .and_then(|part| u8::from_str_radix(part, 16).ok())
        });
        return Some(Color::Rgb {
            r: r?,
            g: g?,
            b: b?,
        });
    }

    Some(match raw {
        "black" => Color::Black,
        "dark_grey" | "dark_gray" => Color::DarkGrey,
        "red" => Color::Red,
        "dark_red" => Color::DarkRed,
        "green" => Color::Green,
        "dark_green" => Color::DarkGreen,
        "yellow" => Color::Yellow,
        "dark_yellow" => Color::DarkYellow,
        "blue" => Color::Blue,
        "dark_blue" => Color::DarkBlue,
        "magenta" => Color::Magenta,
        "dark_magenta" => Color::DarkMagenta,
        "cyan" => Color::Cyan,
        "dark_cyan" => Color::DarkCyan,
        "white" => Color::White,
        "grey" | "gray" => Color::Grey,
        _ => return None,
    })
}

#[cfg(test)]
mod test {
    use super::*;

    fn parse(input: &str) -> Theme {
        let document: KdlDocument = input.parse().unwrap();
        Theme::parse(&document).unwrap()
    }

    #[test]
    fn every_built_in_theme_parses_cleanly() {
        // Sanity check: `Theme::built_in` panics on a malformed bundled
        // theme, which would otherwise only surface the first time someone
        // actually references that theme by name.
        let themes_dir = crate::DEFAULT_CONFIG_DIR
            .get_dir("theme")
            .expect("default_config/theme must be bundled");
        for file in themes_dir.files() {
            let name = file
                .path()
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("theme file must have a UTF-8 stem");
            assert!(
                Theme::built_in(name).is_some(),
                "{name} is bundled but Theme::built_in couldn't find it"
            );
        }
    }

    #[test]
    fn unknown_built_in_theme_names_return_none() {
        assert_eq!(Theme::built_in("does_not_exist"), None);
    }

    #[test]
    fn empty_theme_falls_back_to_empty_style_for_every_field() {
        let theme = parse("");
        assert_eq!(theme.ui.status_bar.to_content_style(), ContentStyle::new());
        assert_eq!(theme.ui.popup.to_content_style(), ContentStyle::new());
        assert_eq!(theme.ui.background.to_content_style(), ContentStyle::new());
    }

    #[test]
    fn resolves_palette_names() {
        let theme = parse(
            r##"
            palette {
                bg "#282a36"
                fg "#f8f8f2"
            }
            ui {
                status_bar {
                    fg "fg"
                    bg "bg"
                }
            }
            "##,
        );
        let style = theme.ui.status_bar;
        assert_eq!(
            style.fg,
            Some(Color::Rgb {
                r: 0xf8,
                g: 0xf8,
                b: 0xf2
            })
        );
        assert_eq!(
            style.bg,
            Some(Color::Rgb {
                r: 0x28,
                g: 0x2a,
                b: 0x36
            })
        );
    }

    #[test]
    fn falls_back_to_a_literal_color_when_not_in_the_palette() {
        let theme = parse(
            r#"
            ui {
                popup {
                    fg "white"
                    bg "dark_grey"
                }
            }
            "#,
        );
        let style = theme.ui.popup;
        assert_eq!(style.fg, Some(Color::White));
        assert_eq!(style.bg, Some(Color::DarkGrey));
    }

    #[test]
    fn parses_modifiers() {
        let theme = parse(
            r#"
            ui {
                status_bar {
                    bold #true
                    italic #true
                    underline #true
                }
            }
            "#,
        );
        let style = theme.ui.status_bar.to_content_style();
        assert!(style.attributes.has(Attribute::Bold));
        assert!(style.attributes.has(Attribute::Italic));
        assert!(style.attributes.has(Attribute::Underlined));
    }

    #[test]
    fn rejects_an_unknown_color_name() {
        let document: KdlDocument = r#"
            ui {
                status_bar {
                    fg "not_a_color"
                }
            }
            "#
        .parse()
        .unwrap();
        let err = Theme::parse(&document).unwrap_err();
        assert_eq!(
            err,
            ThemeParseError::InvalidColor {
                key: "ui.status_bar.fg".to_string(),
                found: "not_a_color".to_string(),
            }
        );
    }

    #[test]
    fn undefined_fields_default_to_an_empty_style() {
        let theme = parse(
            r#"
            ui {
                status_bar {
                    fg "white"
                }
            }
            "#,
        );
        assert_eq!(theme.ui.popup.to_content_style(), ContentStyle::new());
    }
}
