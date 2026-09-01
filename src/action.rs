use std::{fmt, str::FromStr};

use strum::VariantNames;

use crate::Mode;

#[derive(Debug, Clone, PartialEq, Eq, strum::VariantNames)]
pub enum Action {
    Quit,
    FocusGained,
    FocusLost,
    Redraw,
    PasteRawString(String),
    Paste(PasteSource),
    ChangeMode(Mode),
    MoveAnchor(Anchor, Direction),
    Insert(char),
    Delete(DeleteDirection),
    OpenPopup,
}

/// Where a [`Action::Paste`] should read its content from.
#[derive(Debug, Copy, Clone, PartialEq, Eq, strum::VariantNames, strum::EnumString)]
pub enum PasteSource {
    Internal,
    System,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, strum::VariantNames, strum::EnumString)]
pub enum DeleteDirection {
    Left,
    Right,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, strum::VariantNames, strum::EnumString)]
pub enum Direction {
    Up,
    Down,
    Right,
    Left,

    StartOfLine,
    EndOfLine,
    StartOfFile,
    EndOfFile,

    PageUp,
    PageDown,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, strum::VariantNames, strum::EnumString)]
pub enum Anchor {
    Tail,
    Head,
}

/// A byte-offset range into a string.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub len: usize,
}

impl Span {
    fn new(start: usize, len: usize) -> Self {
        Span { start, len }
    }

    fn at(pos: usize) -> Self {
        Span { start: pos, len: 0 }
    }

    /// The exclusive end of this span (`start + len`).
    pub fn end(&self) -> usize {
        self.start + self.len
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind} (at {span})")]
pub struct ActionParseError {
    pub span: Span,
    pub kind: ActionParseErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActionParseErrorKind {
    #[error("unexpected end of input, expected {expected}")]
    UnexpectedEof { expected: &'static str },

    #[error("unknown action `{found}`, expected one of: {valid}")]
    UnknownAction { found: String, valid: String },

    #[error(
        "unknown {type_name} value `{found}`, expected one of: {valid}{suggestion}",
        suggestion = did_you_mean
            .as_deref()
            .map(|candidate| format!(", did you mean `{candidate}`?"))
            .unwrap_or_default()
    )]
    UnknownEnumValue {
        type_name: &'static str,
        found: String,
        valid: String,
        did_you_mean: Option<String>,
    },

    #[error("expected `{expected}`, found `{found}`")]
    UnexpectedChar { expected: char, found: char },

    #[error("expected `{expected}`, found end of input")]
    ExpectedCharGotEof { expected: char },

    #[error("unterminated char literal")]
    UnterminatedCharLiteral,

    #[error("unterminated string literal")]
    UnterminatedStringLiteral,

    #[error("invalid escape sequence `\\{found}`")]
    InvalidEscape { found: char },

    #[error("trailing input after complete action")]
    TrailingInput,
}

impl ActionParseErrorKind {
    fn unknown_enum_value<T: VariantNames>(found: &str) -> Self {
        let type_name = std::any::type_name::<T>().rsplit("::").next().unwrap();
        ActionParseErrorKind::UnknownEnumValue {
            type_name,
            found: found.to_string(),
            valid: T::VARIANTS.join(", "),
            did_you_mean: did_you_mean(found, T::VARIANTS),
        }
    }
}

fn did_you_mean(found: &str, candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .map(|candidate| (*candidate, strsim::jaro_winkler(found, candidate)))
        .filter(|(_, score)| *score > 0.7)
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(candidate, _)| candidate.to_string())
}

impl ActionParseError {
    /// Renders a diagnostic for the tests
    #[cfg(test)]
    pub fn render(&self, input: &str) -> String {
        let leading = input[..self.span.start].chars().count();
        let width = input[self.span.start..self.span.end()]
            .chars()
            .count()
            .max(1);
        format!(
            "{input}\n{}{} {}",
            " ".repeat(leading),
            "^".repeat(width),
            self.kind
        )
    }
}

fn escape(c: char) -> Option<char> {
    match c {
        '\\' => Some('\\'),
        '\'' => Some('\''),
        '"' => Some('"'),
        'n' => Some('\n'),
        't' => Some('\t'),
        'r' => Some('\r'),
        '0' => Some('\0'),
        _ => None,
    }
}

struct Cursor<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(input: &'a str) -> Self {
        Cursor { input, pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.advance();
        }
    }

    fn span_since(&self, start: usize) -> Span {
        Span::new(start, self.pos - start)
    }

    fn parse_ident(&mut self, expected: &'static str) -> Result<(&'a str, Span), ActionParseError> {
        let start = self.pos;
        if self.peek().is_none() {
            return Err(ActionParseError {
                span: Span::at(start),
                kind: ActionParseErrorKind::UnexpectedEof { expected },
            });
        }
        while matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_') {
            self.advance();
        }
        Ok((&self.input[start..self.pos], self.span_since(start)))
    }

    /// Consumes a `'c'` char literal. Does not skip leading whitespace —
    /// whitespace before a literal is not permitted.
    fn parse_char_literal(&mut self) -> Result<(char, Span), ActionParseError> {
        let start = self.pos;
        self.advance_match('\'')?;

        let value = match self.peek() {
            Some('\\') => {
                let esc_start = self.pos;
                self.advance();
                match self.advance() {
                    Some(c) => match escape(c) {
                        Some(value) => value,
                        None => {
                            return Err(ActionParseError {
                                span: self.span_since(esc_start),
                                kind: ActionParseErrorKind::InvalidEscape { found: c },
                            });
                        }
                    },
                    None => {
                        return Err(ActionParseError {
                            span: self.span_since(start),
                            kind: ActionParseErrorKind::UnterminatedCharLiteral,
                        });
                    }
                }
            }
            Some(c) => {
                self.advance();
                c
            }
            None => {
                return Err(ActionParseError {
                    span: self.span_since(start),
                    kind: ActionParseErrorKind::UnterminatedCharLiteral,
                });
            }
        };

        self.advance_match('\'')?;

        Ok((value, self.span_since(start)))
    }

    /// Consumes a `"..."` string literal. Does not skip leading whitespace —
    /// whitespace before a literal is not permitted.
    fn parse_string_literal(&mut self) -> Result<(String, Span), ActionParseError> {
        let start = self.pos;
        self.advance_match('"')?;

        let mut value = String::new();
        loop {
            match self.peek() {
                Some('"') => {
                    self.advance();
                    break;
                }
                Some('\\') => {
                    let esc_start = self.pos;
                    self.advance();
                    match self.advance() {
                        Some(c) => match escape(c) {
                            Some(actual) => value.push(actual),
                            None => {
                                return Err(ActionParseError {
                                    span: self.span_since(esc_start),
                                    kind: ActionParseErrorKind::InvalidEscape { found: c },
                                });
                            }
                        },
                        None => {
                            return Err(ActionParseError {
                                span: self.span_since(start),
                                kind: ActionParseErrorKind::UnterminatedStringLiteral,
                            });
                        }
                    }
                }
                Some(c) => {
                    value.push(c);
                    self.advance();
                }
                None => {
                    return Err(ActionParseError {
                        span: self.span_since(start),
                        kind: ActionParseErrorKind::UnterminatedStringLiteral,
                    });
                }
            }
        }

        Ok((value, self.span_since(start)))
    }

    fn advance_match(&mut self, c: char) -> Result<Span, ActionParseError> {
        let start = self.pos;
        match self.peek() {
            Some(found) if found == c => {
                self.advance();
                Ok(self.span_since(start))
            }
            Some(found) => Err(ActionParseError {
                span: Span::at(start),
                kind: ActionParseErrorKind::UnexpectedChar { expected: c, found },
            }),
            None => Err(ActionParseError {
                span: Span::at(start),
                kind: ActionParseErrorKind::ExpectedCharGotEof { expected: c },
            }),
        }
    }

    /// Skips whitespace, then expects `c` at the cursor.
    fn expect_char(&mut self, c: char) -> Result<Span, ActionParseError> {
        self.skip_whitespace();
        self.advance_match(c)
    }
}

/// Parses `ident`. This only works with unit variant.
fn parse_enum_ident<T>(ident: &str, span: Span) -> Result<T, ActionParseError>
where
    T: FromStr + VariantNames,
{
    ident.parse::<T>().map_err(|_| ActionParseError {
        span,
        kind: ActionParseErrorKind::unknown_enum_value::<T>(ident),
    })
}

fn parse_action(cursor: &mut Cursor) -> Result<Action, ActionParseError> {
    cursor.skip_whitespace();
    let (ident, ident_span) = cursor.parse_ident("an action name")?;
    match ident {
        "Quit" => Ok(Action::Quit),
        "FocusGained" => Ok(Action::FocusGained),
        "FocusLost" => Ok(Action::FocusLost),
        "Redraw" => Ok(Action::Redraw),
        "OpenPopup" => Ok(Action::OpenPopup),

        "PasteRawString" => {
            cursor.expect_char('(')?;
            let (s, _) = cursor.parse_string_literal()?;
            cursor.expect_char(')')?;
            Ok(Action::PasteRawString(s))
        }
        "Paste" => {
            cursor.expect_char('(')?;
            cursor.skip_whitespace();
            let (arg, arg_span) = cursor.parse_ident("a PasteSource value")?;
            let source = parse_enum_ident::<PasteSource>(arg, arg_span)?;
            cursor.expect_char(')')?;
            Ok(Action::Paste(source))
        }
        "ChangeMode" => {
            cursor.expect_char('(')?;
            cursor.skip_whitespace();
            let (arg, arg_span) = cursor.parse_ident("a Mode value")?;
            let mode = parse_enum_ident::<Mode>(arg, arg_span)?;
            cursor.expect_char(')')?;
            Ok(Action::ChangeMode(mode))
        }
        "Insert" => {
            cursor.expect_char('(')?;
            let (c, _) = cursor.parse_char_literal()?;
            cursor.expect_char(')')?;
            Ok(Action::Insert(c))
        }
        "Delete" => {
            cursor.expect_char('(')?;
            cursor.skip_whitespace();
            let (arg, arg_span) = cursor.parse_ident("a DeleteDirection value")?;
            let dir = parse_enum_ident::<DeleteDirection>(arg, arg_span)?;
            cursor.expect_char(')')?;
            Ok(Action::Delete(dir))
        }
        "MoveAnchor" => {
            cursor.expect_char('(')?;
            cursor.skip_whitespace();
            let (a, a_span) = cursor.parse_ident("an Anchor value")?;
            let anchor = parse_enum_ident::<Anchor>(a, a_span)?;
            cursor.expect_char(',')?;
            cursor.skip_whitespace();
            let (d, d_span) = cursor.parse_ident("a Direction value")?;
            let direction = parse_enum_ident::<Direction>(d, d_span)?;
            cursor.expect_char(')')?;
            Ok(Action::MoveAnchor(anchor, direction))
        }
        _ => Err(ActionParseError {
            span: ident_span,
            kind: ActionParseErrorKind::UnknownAction {
                found: ident.to_string(),
                valid: Action::VARIANTS.join(", "),
            },
        }),
    }
}

impl FromStr for Action {
    type Err = ActionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut cursor = Cursor::new(s);
        let action = parse_action(&mut cursor)?;
        cursor.skip_whitespace();
        if cursor.pos != s.len() {
            return Err(ActionParseError {
                span: Span::new(cursor.pos, s.len() - cursor.pos),
                kind: ActionParseErrorKind::TrailingInput,
            });
        }
        Ok(action)
    }
}

#[cfg(test)]
mod test {
    use insta::assert_snapshot;

    use super::*;

    #[test]
    fn parses_unit_variants() {
        assert_eq!("Quit".parse(), Ok(Action::Quit));
        assert_eq!("FocusGained".parse(), Ok(Action::FocusGained));
        assert_eq!("FocusLost".parse(), Ok(Action::FocusLost));
        assert_eq!("Redraw".parse(), Ok(Action::Redraw));
        assert_eq!("OpenPopup".parse(), Ok(Action::OpenPopup));
    }

    #[test]
    fn parses_paste_raw_string() {
        assert_eq!(
            "PasteRawString(\"hello world\")".parse(),
            Ok(Action::PasteRawString("hello world".to_string()))
        );
        assert_eq!(
            "PasteRawString(\"say \\\"hi\\\"\")".parse(),
            Ok(Action::PasteRawString("say \"hi\"".to_string()))
        );
    }

    #[test]
    fn parses_paste() {
        assert_eq!(
            "Paste(Internal)".parse(),
            Ok(Action::Paste(PasteSource::Internal))
        );
        assert_eq!(
            "Paste(System)".parse(),
            Ok(Action::Paste(PasteSource::System))
        );
    }

    #[test]
    fn parses_change_mode() {
        assert_eq!(
            "ChangeMode(Insert)".parse(),
            Ok(Action::ChangeMode(Mode::Insert))
        );
        assert_eq!(
            "ChangeMode(Normal)".parse(),
            Ok(Action::ChangeMode(Mode::Normal))
        );
    }

    #[test]
    fn parses_insert() {
        assert_eq!("Insert('a')".parse(), Ok(Action::Insert('a')));
        assert_eq!("Insert('\\n')".parse(), Ok(Action::Insert('\n')));
        assert_eq!("Insert('(')".parse(), Ok(Action::Insert('(')));
        assert_eq!("Insert(')')".parse(), Ok(Action::Insert(')')));
    }

    #[test]
    fn parses_delete() {
        assert_eq!(
            "Delete(Left)".parse(),
            Ok(Action::Delete(DeleteDirection::Left))
        );
        assert_eq!(
            "Delete(Right)".parse(),
            Ok(Action::Delete(DeleteDirection::Right))
        );
    }

    #[test]
    fn parses_move_anchor() {
        assert_eq!(
            "MoveAnchor(Tail, Up)".parse(),
            Ok(Action::MoveAnchor(Anchor::Tail, Direction::Up))
        );
    }

    #[test]
    fn tolerates_whitespace() {
        assert_eq!(
            "  ChangeMode ( Insert ) ".parse(),
            Ok(Action::ChangeMode(Mode::Insert))
        );
        assert_eq!(
            "MoveAnchor(Tail,Up)".parse(),
            Ok(Action::MoveAnchor(Anchor::Tail, Direction::Up))
        );
        assert_eq!(
            "MoveAnchor( Tail , Up )".parse(),
            Ok(Action::MoveAnchor(Anchor::Tail, Direction::Up))
        );
    }

    #[test]
    fn round_trips_through_debug() {
        let actions = [
            Action::Quit,
            Action::FocusGained,
            Action::FocusLost,
            Action::Redraw,
            Action::OpenPopup,
            Action::PasteRawString("hello world".to_string()),
            Action::Paste(PasteSource::Internal),
            Action::Paste(PasteSource::System),
            Action::ChangeMode(Mode::Insert),
            Action::ChangeMode(Mode::Normal),
            Action::MoveAnchor(Anchor::Tail, Direction::Up),
            Action::MoveAnchor(Anchor::Head, Direction::EndOfLine),
            Action::Insert('a'),
            Action::Insert('\n'),
            Action::Delete(DeleteDirection::Left),
            Action::Delete(DeleteDirection::Right),
        ];

        for action in actions {
            let debug = format!("{action:?}");
            assert_eq!(
                debug.parse::<Action>().as_ref(),
                Ok(&action),
                "round-trip of {debug:?}"
            );
        }
    }

    #[test]
    fn empty_input() {
        let input = "";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @"

        ^ unexpected end of input, expected an action name
        ");
    }

    #[test]
    fn unknown_action() {
        let input = "Cuit";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @"
        Cuit
        ^^^^ unknown action `Cuit`, expected one of: Quit, FocusGained, FocusLost, Redraw, PasteRawString, Paste, ChangeMode, MoveAnchor, Insert, Delete, OpenPopup
        ");
    }

    #[test]
    fn unknown_mode_value() {
        let input = "ChangeMode(Insrt)";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @"
        ChangeMode(Insrt)
                   ^^^^^ unknown Mode value `Insrt`, expected one of: Normal, Insert, did you mean `Insert`?
        ");
    }

    #[test]
    fn unknown_mode_value_without_a_close_match() {
        let input = "ChangeMode(Xyz123)";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @"
        ChangeMode(Xyz123)
                   ^^^^^^ unknown Mode value `Xyz123`, expected one of: Normal, Insert
        ");
    }

    #[test]
    fn extra_argument() {
        let input = "MoveAnchor(Tail, Up, Down)";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @"
        MoveAnchor(Tail, Up, Down)
                           ^ expected `)`, found `,`
        ");
    }

    #[test]
    fn missing_argument() {
        let input = "MoveAnchor(Tail)";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @"
        MoveAnchor(Tail)
                       ^ expected `,`, found `)`
        ");
    }

    #[test]
    fn missing_open_paren() {
        let input = "ChangeMode Insert)";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @"
        ChangeMode Insert)
                   ^ expected `(`, found `I`
        ");
    }

    #[test]
    fn missing_close_paren() {
        let input = "ChangeMode(Insert";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @"
        ChangeMode(Insert
                         ^ expected `)`, found end of input
        ");
    }

    #[test]
    fn insert_missing_quotes() {
        let input = "Insert(a)";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @"
        Insert(a)
               ^ expected `'`, found `a`
        ");
    }

    #[test]
    fn unterminated_char_literal() {
        let input = "Insert('a";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @"
        Insert('a
                 ^ expected `'`, found end of input
        ");
    }

    #[test]
    fn unterminated_string_literal() {
        let input = "PasteRawString(\"unterminated)";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @r#"
        PasteRawString("unterminated)
                       ^^^^^^^^^^^^^^ unterminated string literal
        "#);
    }

    #[test]
    fn invalid_escape() {
        let input = "Insert('\\q')";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @r"
        Insert('\q')
                ^^ invalid escape sequence `\q`
        ");
    }

    #[test]
    fn trailing_input() {
        let input = "Quit extra";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @"
        Quit extra
             ^^^^^ trailing input after complete action
        ");
    }

    #[test]
    fn unit_variant_rejects_parens() {
        let input = "Quit()";
        let err = input.parse::<Action>().unwrap_err();
        assert_snapshot!(err.render(input), @"
        Quit()
            ^^ trailing input after complete action
        ");
    }
}
