use std::{collections::HashMap, str::FromStr};

use action::{did_you_mean, Action, ActionParseError, Mode};
use kdl::{KdlDocument, KdlNode};
use strum::VariantNames;
use thiserror::Error;

use crate::location::Location;

/// The number of keys per [`Row`]s.
pub const ROW_LEN: usize = 12;

/// The name of the rows, todo: we might add "number" one day, not sure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Top,
    Home,
    Bottom,
}

/// Which physical modifier, if any, was held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    /// No modifier held.
    Base,
    Shifted,
    Alted,
    Ctrled,
}

/// One key inside a [`Row`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RowEntry {
    /// `#null`
    #[default]
    Unbound,
    /// `_`
    Fallback,
    /// A parsed [`Action`].
    Bound(Action),
}

/// All the keys of a row
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row(pub [RowEntry; ROW_LEN]);

/// The three physical rows of teh keyboard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowSet {
    pub top: Row,
    pub home: Row,
    pub bottom: Row,
}

impl RowSet {
    fn row(&self, kind: RowKind) -> &Row {
        match kind {
            RowKind::Top => &self.top,
            RowKind::Home => &self.home,
            RowKind::Bottom => &self.bottom,
        }
    }
}

/// The four modifier grids bound for one [`Mode`]: `base` (no modifier
/// held), plus `shifted`/`alted`/`ctrled`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModifierKeymap {
    pub base: RowSet,
    pub shifted: RowSet,
    pub alted: RowSet,
    pub ctrled: RowSet,
}

impl ModifierKeymap {
    fn grid(&self, modifier: Modifier) -> &RowSet {
        match modifier {
            Modifier::Base => &self.base,
            Modifier::Shifted => &self.shifted,
            Modifier::Alted => &self.alted,
            Modifier::Ctrled => &self.ctrled,
        }
    }

    /// Resolves the action bound at `(modifier, row, col)`, following a `_`
    /// fallback to `base` when the modifier grid doesn't override this
    /// position.
    pub fn resolve(&self, modifier: Modifier, row: RowKind, col: usize) -> Option<&Action> {
        assert!(
            col < ROW_LEN,
            "resolve called with a col larger than the row len"
        );
        match &self.grid(modifier).row(row).0[col] {
            RowEntry::Bound(action) => Some(action),
            RowEntry::Unbound => None,
            RowEntry::Fallback => {
                debug_assert_ne!(
                    modifier,
                    Modifier::Base,
                    "The base keymap cannot use fallback"
                );
                self.resolve(Modifier::Base, row, col)
            }
        }
    }
}

/// One keymap layer
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keymap {
    pub modes: HashMap<Mode, ModifierKeymap>,
    pub layer: Option<String>,
}

impl Keymap {
    /// Resolves the action bound for `mode` at `(modifier, row, col)`.
    // TODO: Here the `mode` should not be a parameter.
    //       If we're moving to another mode, we should create a new keymap
    pub fn resolve(
        &self,
        mode: Mode,
        modifier: Modifier,
        row: RowKind,
        col: usize,
    ) -> Option<&Action> {
        self.modes.get(&mode)?.resolve(modifier, row, col)
    }

    /// Parses one keymap's body — the children of a `keymap "name" { ... }`
    /// node. A missing/empty body parses to [`Keymap::default`].
    pub(crate) fn parse(
        document: &KdlDocument,
        location: Location<'_>,
    ) -> Result<Keymap, KeymapParseError> {
        let mut keymap = Keymap::default();

        if let Some(value) = document.get_arg("layer") {
            keymap.layer = value.as_string().map(str::to_string);
        }

        for child in document.nodes() {
            let name = child.name().value();
            if name == "layer" {
                continue;
            }

            let child_location = location.key(name);
            let mode = Mode::from_str(name).map_err(|_| KeymapParseError::UnknownMode {
                location: child_location.to_string(),
                found: name.to_string(),
                valid: Mode::VARIANTS.join(", "),
                did_you_mean: did_you_mean(name, Mode::VARIANTS),
            })?;

            let modifier_keymap = match child.children() {
                Some(modes_doc) => parse_modifier_keymap(modes_doc, child_location)?,
                None => ModifierKeymap::default(),
            };
            keymap.modes.insert(mode, modifier_keymap);
        }

        Ok(keymap)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeymapParseError {
    #[error(
        "unknown mode `{found}` in `{location}`, expected one of: {valid}{suggestion}",
        suggestion = did_you_mean
            .as_deref()
            .map(|candidate| format!(", did you mean `{candidate}`?"))
            .unwrap_or_default()
    )]
    UnknownMode {
        location: String,
        found: String,
        valid: String,
        did_you_mean: Option<String>,
    },

    #[error(
        "unknown modifier `{found}` in `{location}`, expected one of: base, shifted, alted, ctrled"
    )]
    UnknownModifier { location: String, found: String },

    #[error("invalid action at `{location}`: {source}")]
    InvalidAction {
        location: String,
        #[source]
        source: ActionParseError,
    },

    #[error(
        "`{location}` has {count} entries, but a row must have {} entries",
        ROW_LEN
    )]
    BadSizedRow { location: String, count: usize },

    #[error(
        "`{location}` entry {index} must be a string action, `_`, or `#null`, found `{found}`"
    )]
    InvalidRowEntry {
        location: String,
        index: usize,
        found: String,
    },

    #[error("`{location}` entry {index} uses `_`, but `base` has nothing further to fall back to")]
    FallbackInBaseGrid { location: String, index: usize },
}

/// Parses the `base`/`shifted`/`alted`/`ctrled` children of one `Mode`
/// block. Any other child name is a hard error.
fn parse_modifier_keymap(
    document: &KdlDocument,
    location: Location<'_>,
) -> Result<ModifierKeymap, KeymapParseError> {
    let mut modifier_keymap = ModifierKeymap::default();

    for child in document.nodes() {
        let name = child.name().value();
        let is_base = name == "base";
        if !matches!(name, "base" | "shifted" | "alted" | "ctrled") {
            return Err(KeymapParseError::UnknownModifier {
                location: location.to_string(),
                found: name.to_string(),
            });
        }

        let child_location = location.key(name);
        let row_set = match child.children() {
            Some(rows) => parse_row_set(rows, child_location, is_base)?,
            None => RowSet::default(),
        };

        match name {
            "base" => modifier_keymap.base = row_set,
            "shifted" => modifier_keymap.shifted = row_set,
            "alted" => modifier_keymap.alted = row_set,
            "ctrled" => modifier_keymap.ctrled = row_set,
            _ => unreachable!(),
        }
    }

    Ok(modifier_keymap)
}

/// Parses the `top`/`home`/`bottom` rows of one modifier grid. Any other
/// child name is silently ignored (see [`RowKind`]'s doc comment).
// TODO: Phase 2, we should stop silently ignoring stuff and instead starts returning errors with clear messages saying the field doesn't exists
fn parse_row_set(
    document: &KdlDocument,
    location: Location<'_>,
    is_base: bool,
) -> Result<RowSet, KeymapParseError> {
    let mut row_set = RowSet::default();

    if let Some(row) = document.get("top") {
        row_set.top = parse_row(row, location.key("top"), is_base)?;
    }
    if let Some(row) = document.get("home") {
        row_set.home = parse_row(row, location.key("home"), is_base)?;
    }
    if let Some(row) = document.get("bottom") {
        row_set.bottom = parse_row(row, location.key("bottom"), is_base)?;
    }

    Ok(row_set)
}

/// Parses one row node's positional entries (`top #null "Delete(Left)" _ ...`)
/// into a [`Row`]. `is_base` controls whether `_` is legal here (it never is
/// inside `base`, since there's nothing further to fall back to).
fn parse_row(
    node: &KdlNode,
    location: Location<'_>,
    is_base: bool,
) -> Result<Row, KeymapParseError> {
    let positional: Vec<_> = node
        .entries()
        .iter()
        .filter(|e| e.name().is_none())
        .collect();
    if positional.len() != ROW_LEN {
        return Err(KeymapParseError::BadSizedRow {
            location: location.to_string(),
            count: positional.len(),
        });
    }

    let mut row = Row::default();
    for (index, entry) in positional.into_iter().enumerate() {
        let value = entry.value();
        let row_entry = if value.is_null() {
            RowEntry::Unbound
        } else if let Some(s) = value.as_string() {
            if s == "_" {
                if is_base {
                    return Err(KeymapParseError::FallbackInBaseGrid {
                        location: location.to_string(),
                        index,
                    });
                }
                RowEntry::Fallback
            } else {
                let action =
                    Action::from_str(s).map_err(|source| KeymapParseError::InvalidAction {
                        location: location.index(index).to_string(),
                        source,
                    })?;
                RowEntry::Bound(action)
            }
        } else {
            return Err(KeymapParseError::InvalidRowEntry {
                location: location.to_string(),
                index,
                found: value.to_string(),
            });
        };
        row.0[index] = row_entry;
    }

    Ok(row)
}

#[cfg(test)]
mod test {
    use insta::assert_snapshot;

    use super::*;

    fn parse(input: &str) -> Keymap {
        let document: KdlDocument = input.parse().unwrap();
        Keymap::parse(&document, Location::Root.key("keymap \"test\"")).unwrap()
    }

    fn err(input: &str) -> KeymapParseError {
        let document: KdlDocument = input.parse().unwrap();
        Keymap::parse(&document, Location::Root.key("keymap \"test\"")).unwrap_err()
    }

    #[test]
    fn empty_body_defaults_to_an_empty_keymap() {
        assert_eq!(parse(""), Keymap::default());
    }

    #[test]
    fn parses_a_base_row_with_mixed_bound_and_unbound_slots() {
        let keymap = parse(
            r#"
            normal {
                base {
                    home "Quit" #null "ChangeMode(Insert)" #null #null #null #null #null #null #null #null #null
                }
            }
            "#,
        );
        let normal = keymap.modes.get(&Mode::Normal).unwrap();
        assert_eq!(
            normal.resolve(Modifier::Base, RowKind::Home, 0),
            Some(&Action::Quit)
        );
        assert_eq!(normal.resolve(Modifier::Base, RowKind::Home, 1), None);
        assert_eq!(
            normal.resolve(Modifier::Base, RowKind::Home, 2),
            Some(&Action::ChangeMode(Mode::Insert))
        );
        // Trailing columns are `#null`: explicitly unbound.
        assert_eq!(normal.resolve(Modifier::Base, RowKind::Home, 11), None);
    }

    #[test]
    fn shifted_fallback_reads_through_to_base() {
        let keymap = parse(
            r#"
            normal {
                base { home "Quit" "OpenPopup" #null #null #null #null #null #null #null #null #null #null }
                shifted { home _ "Delete(Left)" #null #null #null #null #null #null #null #null #null #null }
            }
            "#,
        );
        let normal = keymap.modes.get(&Mode::Normal).unwrap();
        // Column 0 falls back to base's `Quit`.
        assert_eq!(
            normal.resolve(Modifier::Shifted, RowKind::Home, 0),
            Some(&Action::Quit)
        );
        // Column 1 overrides base's `OpenPopup`.
        assert_eq!(
            normal.resolve(Modifier::Shifted, RowKind::Home, 1),
            Some(&Action::Delete(action::DeleteDirection::Left))
        );
    }

    #[test]
    fn fallback_in_base_is_a_hard_error() {
        let error = err(r#"
            normal {
                base { home _ #null #null #null #null #null #null #null #null #null #null #null }
            }
            "#);
        assert_snapshot!(error, @r#"`keymap "test" > normal > base > home` entry 0 uses `_`, but `base` has nothing further to fall back to"#);
    }

    #[test]
    fn unknown_mode_name_reports_a_suggestion() {
        let error = err(r#"
            nromal {
                base { home "Quit" }
            }
            "#);
        assert_snapshot!(error, @r#"unknown mode `nromal` in `keymap "test" > nromal`, expected one of: Normal, Insert, did you mean `Normal`?"#);
    }

    #[test]
    fn unknown_modifier_name_is_an_error() {
        let error = err(r#"
            normal {
                pressed { home "Quit" }
            }
            "#);
        assert_snapshot!(error, @r#"unknown modifier `pressed` in `keymap "test" > normal`, expected one of: base, shifted, alted, ctrled"#);
    }

    #[test]
    fn unknown_row_name_is_silently_ignored() {
        let keymap = parse(
            r#"
            normal {
                base { thumb "Quit" }
            }
            "#,
        );
        // Parses without error; `thumb` just isn't a recognized row (yet).
        assert_eq!(
            keymap.modes.get(&Mode::Normal).unwrap(),
            &ModifierKeymap::default()
        );
    }

    #[test]
    fn row_over_the_limit_is_an_error() {
        let thirteen = vec!["#null"; 13].join(" ");
        let input = format!(
            r#"
            normal {{
                base {{ home {thirteen} }}
            }}
            "#
        );
        let error = err(&input);
        assert_snapshot!(error, @r#"`keymap "test" > normal > base > home` has 13 entries, but a row must have 12 entries"#);
    }

    #[test]
    fn invalid_row_entry_type_is_an_error() {
        let error = err(r#"
            normal {
                base { home 42 #null #null #null #null #null #null #null #null #null #null #null }
            }
            "#);
        assert_snapshot!(error, @r#"`keymap "test" > normal > base > home` entry 0 must be a string action, `_`, or `#null`, found `42`"#);
    }

    #[test]
    fn invalid_action_string_wraps_the_underlying_error() {
        let error = err(r#"
            normal {
                base { home "MoveAnchor(Tail)" #null #null #null #null #null #null #null #null #null #null #null }
            }
            "#);
        assert_snapshot!(error, @r#"invalid action at `keymap "test" > normal > base > home[0]`: expected `,`, found `)` (at 15..15)"#);
    }

    #[test]
    fn layer_field_is_parsed_but_otherwise_inert() {
        let keymap = parse(
            r#"
            layer "default"
            normal {
                base { home "Quit" #null #null #null #null #null #null #null #null #null #null #null }
            }
            "#,
        );
        assert_eq!(keymap.layer, Some("default".to_string()));
    }

    #[test]
    fn two_sibling_modes_both_parse() {
        let keymap = parse(
            r#"
            normal {
                base { home "Quit" #null #null #null #null #null #null #null #null #null #null #null }
            }
            insert {
                base { home "ChangeMode(Normal)" #null #null #null #null #null #null #null #null #null #null #null }
            }
            "#,
        );
        assert_eq!(keymap.modes.len(), 2);
        assert!(keymap.modes.contains_key(&Mode::Normal));
        assert!(keymap.modes.contains_key(&Mode::Insert));
    }
}
