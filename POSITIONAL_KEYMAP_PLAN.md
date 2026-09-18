# Positional keymap (ki-style), staged across multiple sessions

## Context

The editor currently binds keys the way vim/helix do: actions are wired to
whatever character a key produces (`'q' => Quit`, `'i' => ChangeMode(Insert)`,
hardcoded in `Editor::event_to_action`, `src/main.rs:200-247`). We want to move
to a **ki-editor-style positional keymap** instead: actions are bound to a
*physical key position* on the keyboard, chosen for ergonomics (accessible
fingers, navigation on the left hand), completely decoupled from what
character that position produces under the user's active keyboard layout
(QWERTY, AZERTY, ergo-l, Dvorak, ...).

Two independent things have to exist for that to work:

1. **A keymap**: for each editor `Mode` and physical position, which `Action`
   fires. This is layout-independent by construction — it never mentions a
   character or a layout, only rows/columns.
2. **A way to know which physical position was just pressed.** The "true"
   answer is the kitty keyboard protocol, which reports a layout-independent
   "base layout key" for exactly this purpose. But the vendored `crossterm
   0.29` parses kitty's CSI-u sequences and **discards that exact field**
   (confirmed by reading crossterm's vendored source,
   `~/.cargo/registry/.../crossterm-0.29.0/src/event/sys/unix/parse.rs:496-609`;
   crossterm's own doc comment at `event.rs:289` admits alternate/base-layout
   keys "are not yet supported"). Getting kitty support therefore means
   patching/forking crossterm or writing a standalone kitty CSI-u parser —
   real work, deliberately deferred to its own later phase (Phase 4 below).

   In the meantime we "cheat": we let the user describe their keyboard
   **layout** (which character sits at which physical position, for their
   specific layout) as its own config, fully independent of the keymap. Given
   a received character, we reverse-look-it-up in the active layout to find
   the physical position, then look up the action for that position in the
   keymap. Once kitty support lands later, the kitty resolver simply replaces
   this reverse-lookup step and the `layout` config becomes optional/unused —
   nothing else in the pipeline changes. This is the key architectural
   insight driving the whole plan: **resolving "which physical position was
   pressed" is decoupled from "which action lives at that position."**

This plan is scoped to be executed across several sessions. Each phase below
is independently buildable, testable, and committable.

---

## Decisions locked in during planning (do not re-litigate)

- **Scope of phases 0-3**: build the real thing (schema + parsing + actual
  dispatch), using the char/layout resolver. Kitty support is Phase 4, later.
- **New shared crate** `crates/action` holds `Action` + `Mode` (extracted out
  of the `editor` binary crate) so `crates/config` can depend on them.
- **No QWERTY-letter references anywhere** in the keymap schema — physical
  key *columns* are addressed purely positionally (index in an array), never
  by letter. Physical *rows* may be named by keyboard geography (`top`,
  `home`, `bottom`) since that's a position description, not a character.
- **Keymap row shape**: 3 rows (`top`/`home`/`bottom`) × up to 12 columns,
  flat arrays, no left/right nesting in the schema (hand placement is just a
  convention in which slots get bound, not a schema concept).
- **Keymap `_` fallback**: keymap action grids for `shifted`/`alted`/`ctrled`
  modifiers may use `_` per-slot to mean "fall back to the `base` (no
  modifier) grid's action at this position" — so you only override the
  positions where a modifier changes what fires, not all 36 slots per
  modifier per mode.
- **Layout has no fallback, ever**: every one of the layout's four grids
  (`base`/`shifted`/`alted`/`ctrled`) must be fully, explicitly spelled out.
  This is a deliberate choice — the user was burned by `ki`'s implicit
  shift/alt derivation before, especially around Alt, and wants zero magic
  here even though it's more verbose to author.
- **Named keymaps, QMK-style layering staged for later**: the schema is
  `keymap "name" { ... }` (repeatable, named), modeled as a
  `HashMap<String, Keymap>`, but only a keymap literally named `"default"` is
  ever resolved/used right now. Each `Keymap` also parses an optional
  `layer "other-name"` field — parsed and stored, but not resolved or acted
  on anywhere yet. This is reserved for a future QMK-style layer-switching
  scheme with (per the user) at least three distinct semantics to design
  later: switch to a keymap **permanently** (QMK `DF`/`TO`), **for the next
  keypress only** (QMK `OSL` one-shot), or **only while a key is held** (QMK
  `MO` momentary).
- **Modes are generic, not hardcoded**: a keymap's per-mode blocks
  (`normal { ... }`, `insert { ... }`) are matched by parsing the child
  node's name via `Mode::from_str`. As of Phase 1.5, `Mode`
  (`crates/action/src/mode.rs`, moved there by Phase 0) is no longer a
  closed enum at all — it's an open, arbitrary string identifier
  (`Mode::from_str` is infallible), so a brand new mode name (e.g. `visual`)
  becomes fully usable from config alone, with zero code changes, not just
  "zero parser changes". `"normal"`/`"insert"` remain the only names the
  editor's own Rust code references directly, as bootstrap/fallback entry
  points (see Phase 1.5, Part C).
- **Layout presets bundled now**: `qwerty` and `ergo-l` only (the user's two
  personal layouts). More can be added later without schema changes, the
  same way bundled themes work (`default_config/theme/monokai.kdl`).
- **Cross-layer merge semantics**: a `keymap "default"` (or a `layout`)
  defined in a more specific config layer **fully replaces** the same-named
  one from a less specific layer — no deep/per-slot merging across layers.
  Mirrors how `theme: Option<Theme>` already behaves via `InnerConfig::resolve`
  (`crates/config/src/lib.rs:251-264`).

### Naming call I'm making unilaterally (flag if you disagree)

The "no modifier held" grid is called **`base`**, not `normal` — because
`normal` is already taken by `Mode::Normal` one level up, and
`keymap.normal.normal` reads badly. `keymap "default" { normal { base { ... }
shifted { ... } } } }` reads fine. Same `base` name used in `layout` for
symmetry.

---

## Phase 0 — Extract `Action` + `Mode` into `crates/action`

**Why first**: `crates/config` (a library) currently can't depend on `Action`/
`Mode` because they live in the `editor` binary crate, and a binary can't be
depended on by the library it depends on. Every later phase needs this fixed
first. Pure mechanical refactor, no behavior change.

**Files**:
- New crate `crates/action/` (add to workspace `members` in root
  `Cargo.toml:25`, add `action = { path = "crates/action" }` to
  `[workspace.dependencies]`).
  - `crates/action/src/action.rs` — moved verbatim from `src/action.rs`
    (currently 748 lines: `Action` enum + `PasteSource`/`DeleteDirection`/
    `Direction`/`Anchor` + the hand-rolled `FromStr for Action` parser +
    `ActionParseError`/`ActionParseErrorKind` + tests). Promote the existing
    `fn did_you_mean(...)` (line 150) to `pub fn did_you_mean(...)` so the new
    keymap parser (Phase 1) can reuse the same `strsim::jaro_winkler`
    "did you mean" logic instead of duplicating it.
  - `crates/action/src/mode.rs` — moved verbatim from `src/main.rs:104-118`
    (`Mode` enum + its `impl`). **Add `Hash`** to `Mode`'s derive list (needed
    once `Mode` becomes a `HashMap` key in Phase 1) — the only non-mechanical
    change in this phase.
  - `crates/action/src/lib.rs` — re-exports both.
- `editor` binary (`Cargo.toml` + `src/`): delete `src/action.rs`, remove the
  `Mode` definition from `src/main.rs`, add `action = { workspace = true }`
  as a dependency, fix up imports in `src/main.rs`, `src/screen/mod.rs`,
  `src/screen/components/mod.rs`, `src/screen/view/buffer_view.rs` (all
  currently do `use crate::{action::{...}, ...}` / `crate::Mode` — becomes a
  plain `use action::{...};`). `strsim`/`strum`/`thiserror` can be dropped
  from `editor`'s own `[dependencies]` since nothing else in `src/` uses them
  directly (verified via `grep -rn "thiserror::\|strum::\|strsim::" src/`).
- `crates/config/Cargo.toml`: add `action = { workspace = true }`.

**Verify**: `cargo check --workspace` and `cargo test --workspace` both still
pass with zero behavior change (existing `action.rs` tests move as-is and
should pass unmodified from their new location).

---

## Phase 1 — Keymap config schema + parsing (`crates/config/src/keymap.rs`)

Mirrors the structure/testing style of the existing `crates/config/src/theme.rs`
(manual `KdlDocument`/`KdlNode` walking, no serde, dedicated `thiserror` error
enum, exhaustive `#[cfg(test)]` with `insta` snapshots).

### Shape

```rust
pub enum RowEntry {
    Unbound,       // `#null`
    Fallback,      // `_` — only legal inside shifted/alted/ctrled, never `base`
    Bound(Action), // a quoted Action::from_str string
}

pub struct Row(pub Vec<RowEntry>); // up to 12 entries; trailing entries may be omitted (implicit Unbound)

pub struct RowSet { pub top: Row, pub home: Row, pub bottom: Row }

pub struct ModifierKeymap {
    pub base: RowSet,    // no modifier held
    pub shifted: RowSet,
    pub alted: RowSet,
    pub ctrled: RowSet,
}

pub struct Keymap {
    pub modes: HashMap<Mode, ModifierKeymap>, // populated generically via Mode::from_str on child block names
    pub layer: Option<String>,                // reserved for future QMK-style layering, unresolved for now
}
```

`Keymap::resolve(&self, mode: Mode, modifier: Modifier, row: RowKind, col: usize) -> Option<&Action>`:
look up the slot in the requested modifier's grid; if it's `Bound`, return it;
if `Unbound`, return `None`; if `Fallback`, look up the same position in
`base` instead (a `Fallback` found inside `base` itself is a **parse-time
error**, not a runtime fallback loop — see error list below).

### KDL schema (worked example)

```kdl
keymap "default" {
    normal {
        base {
            //      col0   col1              col2               col3    ...   col11
            top     #null  "Delete(Left)"    "Delete(Right)"    #null   ...   #null
            home    "ChangeMode(Insert)" #null #null            #null   ...   "Quit"
            bottom  #null  #null             #null              #null   ...   #null
        }
        shifted {
            // Most slots just inherit the base action; override only where
            // holding Shift should do something different.
            top     _ _ _ _ _ _ _ _ _ _ _ _
            home    _ _ _ _ _ _ _ _ _ _ _ _
            bottom  _ _ _ _ _ _ _ _ _ _ _ _
        }
        alted  { top _ _ _ _ _ _ _ _ _ _ _ _  home _ _ _ _ _ _ _ _ _ _ _ _  bottom _ _ _ _ _ _ _ _ _ _ _ _ }
        ctrled { top _ _ _ _ _ _ _ _ _ _ _ _  home _ _ _ _ _ _ _ _ _ _ _ _  bottom _ _ _ _ _ _ _ _ _ _ _ _ }
    }

    insert {
        base   { top #null ... home #null ... bottom #null ... }
        shifted { top _ ... home _ ... bottom _ ... }
        alted   { top _ ... home _ ... bottom _ ... }
        ctrled  { top _ ... home _ ... bottom _ ... }
    }
}

// Parses into the keymaps map; `layer` is stored but nothing consumes it yet.
keymap "arrows-on-home" {
    layer "default"
    normal {
        base { home "MoveAnchor(Head, Left)" "MoveAnchor(Head, Down)" "MoveAnchor(Head, Up)" "MoveAnchor(Head, Right)" }
    }
}
```

### Error type (`KeymapParseError`, thiserror, mirrors `ThemeParseError`)

- `UnknownMode { location, found, valid, did_you_mean }` — unknown mode block
  name, "did you mean" via the promoted `action::did_you_mean`.
- `UnknownModifier { location, found }` — a child of a mode block that isn't
  `base`/`shifted`/`alted`/`ctrled` is a hard error here (unlike keymap row
  names inside a `RowSet`, where an unrecognized row name is silently
  ignored to leave room for a future 4th row, same leniency style as
  `theme.rs`'s `parse_named_style`).
- `InvalidAction { location, source: ActionParseError }`.
- `RowTooLong { location, count }` (> 12).
- `InvalidRowEntry { location, index, found }` (not `#null`, `_`, or string).
- `FallbackInBaseGrid { location, index }` — `_` used inside a `base` block.

### `crates/config/src/lib.rs` wiring

- `ConfigLayer` gains `keymaps: HashMap<String, Keymap>`. This is a
  collection, not a scalar `Option<T>`, so it's **not** compatible with the
  `#[derive(ConfigField)]` macro (which the macro's own doc comment says it
  silently skips for non-`Option<T>` fields,
  `crates/config-macros/src/lib.rs:7-14`) — parse/merge it by hand, the same
  way `theme` gets special-cased in `parse_theme_node`
  (`crates/config/src/lib.rs:159-194`).
- `ConfigLayer::parse` (`lib.rs:119-152`): unlike `status_bar`/`theme`
  (each appear at most once, fetched via `document.get(name)`), `keymap` is
  **repeated**, so scan with `document.nodes()` filtering by name, read the
  first positional string argument as the keymap's name, and insert into the
  map.
- New `Config::get_keymap(&self, name: &str) -> Option<Keymap>` — walks the
  layer chain (same shape as `InnerConfig::resolve`, `lib.rs:251-264`, but
  returning `None` instead of panicking when no layer defines that name) and
  `Config::get_default_keymap(&self) -> Keymap` (panics only if `"default"`
  is missing everywhere, which the bundled config guarantees).

### Bundled default keymap

Inlined directly in `default_config/config.kdl` (append after the existing
`theme { ... }` block) as `keymap "default" { ... }` — **not** a separate
`default_config/keymap/*.kdl` resolved by name, because nothing needs
"pick a bundled keymap variant by name" yet (unlike themes). If that's wanted
later it's an additive follow-up (`Keymap::built_in`, mirroring
`Theme::built_in`, `theme.rs:61-68`), not a redesign.

Content: a **structurally-valid placeholder**, porting today's hardcoded
`'q'`/`'i'`/`' '` bindings (`src/main.rs:224-229`) onto arbitrary `normal.base`
columns, everything else `#null`/`_`. Actual ergonomic key assignment (which
action truly belongs where) is explicitly undecided — that's a separate,
later design pass once this mechanism exists to test against.

Note: non-character `KeyCode`s already handled today (`Backspace`, `Delete`,
arrows, `Home`/`End`, `PageUp`/`PageDown`, `Enter`, `Tab`, `Esc`, the literal
`Insert` key) are distinct `KeyCode` variants, not `KeyCode::Char(_)` — they
don't participate in this positional matrix at all. Whether/how they ever get
folded in is a question for Phase 3, not this phase.

### Tests

Mirror `theme.rs`'s style: bare `keymap "default"` defaults to empty;
mixed `#null`/`_`/action rows resolve correctly including the fallback chain;
`_` inside `base` is a hard error; unknown mode/modifier name errors with
"did you mean" where applicable; row >12 entries errors; two sibling
`keymap "a" {} keymap "b" {}` both parse into the map; a more specific config
layer's `keymap "default"` fully replaces (not merges with) a less specific
layer's; `Config::default().get_default_keymap()` resolves without panicking
and contains the placeholder bindings.

---

## Phase 1.5 — Self-insert action templates + open-ended `Mode`

**Why this exists / why it's not part of Phase 1**: Phase 1 shipped the keymap schema and
`ModifierKeymap`/`RowSet` grid, but punted on "insert mode" by placeholder-binding its entire
grid to `#null` (`default_config/config.kdl`'s `insert { base { ... } }` block). That doesn't
work: typing needs the actual character produced by a keystroke, which is exactly the thing
the position/layout grid is designed to *not* care about (see the parent doc's core insight).
Encoding "type what you typed" as 144 literal `Insert('a')`/`Insert('b')`/... bindings would
duplicate the entire `Layout` config inside every keymap and break the moment a layout
changes. So "insert mode" needs a fundamentally different resolution path — a *mode-wide
fallback template*, not more grid bindings — and while doing that we noticed the mode name
itself doesn't need to be fixed either. This phase builds both, staying schema/type-level only
(same scope discipline as Phase 1): buildable and fully unit-testable in `crates/action` and
`crates/config` without Phase 3's real dispatch existing yet.

### Part A — `$`-hole action templates (`crates/action/src/action.rs`)

Add a second, parallel grammar entry point alongside `Action::from_str`/`parse_action`
(`crates/action/src/action.rs:387-461`): a **template** parse that mirrors it exactly, except
wherever a variant's argument is a `char` (today, only `Insert`'s), it may instead be a `$`
placeholder ("hole"), to be filled in later with the character an actual keystroke produced.

Why a parallel type instead of extending `Action`/`Action::from_str` themselves: `Action`
needs to stay a plain, fully-resolved value (every existing call site — `Keymap::resolve`,
`process_action`, tests — assumes a concrete `Action`, no half-filled state). Ordinary grid
bindings (`RowEntry::Bound(Action)`, parsed via `Action::from_str`) must keep rejecting `$`
outright — a `#null`/action grid slot needs a literal, always. Only the new `self_insert`
directive (Part B) is a template.

```rust
/// One character not yet known: filled in at keystroke-resolution time.
enum CharOrHole {
    Char(char),
    Hole,
}

/// An `Action` constructor with exactly one `char` argument left as a `$`
/// hole, e.g. `Insert($)`. Mirrors `Action`, but only lists the variants
/// that have a char-typed field to hold the hole (today: just `Insert`).
/// A future variant needing hole-support gets its own arm here, exactly the
/// same way `Action`'s own per-variant hand-rolled parser already works —
/// no generic reflection, consistent with this file's existing style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionTemplate {
    Insert(CharOrHole),
}

impl ActionTemplate {
    /// Fills the hole (if any) with `value` and produces a concrete `Action`.
    /// If the template had a literal char instead of a hole, `value` is
    /// ignored and that literal is used (this is what lets `self_insert`
    /// reject a holeless template explicitly at config-parse time instead
    /// of silently ignoring every keystroke — see Part B).
    pub fn fill(&self, value: char) -> Action {
        match self {
            ActionTemplate::Insert(CharOrHole::Hole) => Action::Insert(value),
            ActionTemplate::Insert(CharOrHole::Char(c)) => Action::Insert(*c),
        }
    }

    pub fn has_hole(&self) -> bool {
        matches!(self, ActionTemplate::Insert(CharOrHole::Hole))
    }
}

impl FromStr for ActionTemplate {
    type Err = ActionParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> { /* parse_action_template(&mut Cursor::new(s)) + trailing-input check, mirrors `impl FromStr for Action` at action.rs:463-477 */ }
}
```

Parsing (`fn parse_action_template(cursor: &mut Cursor) -> Result<ActionTemplate, ActionParseError>`,
placed right after `parse_action`): parse the ident exactly like `parse_action` does
(`cursor.parse_ident("an action name")`), then:
- `"Insert"` → `expect_char('(')`, then a new `Cursor::parse_char_literal_or_hole()` (peek for
  `'$'`: if present, `advance()` past it and return `CharOrHole::Hole`; else delegate to the
  existing `parse_char_literal()` and wrap `CharOrHole::Char`), then `expect_char(')')` →
  `ActionTemplate::Insert(...)`.
- Any other known ident (`Quit`, `ChangeMode`, `MoveAnchor`, `Delete`, `Resize`, `Paste`,
  `PasteRawString`, `OpenPopup`, `FocusGained`, `FocusLost`, `Redraw`) → new error variant
  `ActionParseErrorKind::NotTemplatable { found: String }` (message e.g. `"` `` `{found}` ``
  `has no argument that can be filled in at keystroke time"`). These variants have no
  char-typed field, so there's nothing for a hole to fill.
- Unknown ident → reuse the existing `ActionParseErrorKind::UnknownAction` path.

`$` needs no special-casing to avoid colliding with `parse_ident` (which already treats `_` as
an identifier character, `action.rs:231` — this is exactly the ambiguity that ruled out `_` as
the hole token during design). `$` is not in `parse_ident`'s character class and appears
nowhere else in the grammar, so `cursor.peek() == Some('$')` is unambiguous.

`crates/action/src/lib.rs`: re-export `ActionTemplate` alongside the existing `Action`/`Mode`
re-exports.

Tests (`crates/action/src/action.rs`'s `mod test`, mirroring its existing insta-snapshot
style for errors, e.g. `action.rs:611-619`):
- `ActionTemplate::from_str("Insert($)")` → `has_hole() == true`, `.fill('x') == Action::Insert('x')`.
- `ActionTemplate::from_str("Insert('a')")` → `has_hole() == false`, `.fill('z') == Action::Insert('a')` (value ignored).
- `ActionTemplate::from_str("Quit")` → `Err(NotTemplatable { found: "Quit" })`, snapshot the rendered message.
- Malformed input (`"Insert($"`, missing `)`) reuses the existing `expect_char` error path —
  add one snapshot test to confirm it composes correctly through the new entry point.

### Part B — `self_insert` keymap directive (`crates/config/src/keymap.rs`)

Add a field to `ModifierKeymap` (it's the per-mode struct despite the name — see the "Loose
end" note at the bottom of this phase):

```rust
pub struct ModifierKeymap {
    pub base: RowSet,
    pub shifted: RowSet,
    pub alted: RowSet,
    pub ctrled: RowSet,
    /// The action template used for a `KeyCode::Char` in this mode when the
    /// grid genuinely has no binding at the resolved position (fallback
    /// only — an explicit grid binding always wins). `None` means "do
    /// nothing" for an unbound char, same as today's behavior everywhere.
    pub self_insert: Option<ActionTemplate>,
}
```

KDL shape — a plain single-argument node, sibling to `base`/`shifted`/`alted`/`ctrled`:

```kdl
insert {
    self_insert "Insert($)"
}
```

Note there's no `base`/`shifted`/`alted`/`ctrled` block at all in this example — an omitted
grid already defaults to fully unbound today (`ModifierKeymap::default()`), so a mode that's
*pure* self-insert needs nothing else. A mode that wants explicit overrides (e.g. a future
Ctrl+W word-delete) would add a `ctrled { ... }` block as normal; that binding is resolved
first and wins over `self_insert`, which only fires when resolution returns `None`.

Parsing, in `parse_modifier_keymap` (`crates/config/src/keymap.rs:213-247`): today it rejects
any child node name other than `base`/`shifted`/`alted`/`ctrled` via `KeymapParseError::UnknownModifier`.
Add a branch for `name == "self_insert"` *before* that rejection:
- Read the node's first positional string argument the same way `crates/config/src/lib.rs`
  already extracts the keymap name (`node.entries().iter().find(|e| e.name().is_none()).and_then(|e| e.value().as_string())`,
  `lib.rs:176-180`). Missing/non-string argument → new error `SelfInsertMissingArgument { location }`.
- Parse it with `ActionTemplate::from_str`, mapping a parse error to a new error
  `InvalidSelfInsertAction { location, source: ActionParseError }` (mirrors the existing
  `InvalidAction { location, source }` variant, `keymap.rs:182-187`).
- If `.has_hole()` is false, error `SelfInsertMissingHole { location, found }` (`found` = the
  raw argument string, not a re-rendered template — simplest, and matches how other
  `KeymapParseError` variants already just echo back the offending input). A self_insert with
  no hole would silently ignore every keystroke's actual character, which is never what's
  intended — reject it at config-parse time instead of producing confusing runtime behavior.
- On success, `modifier_keymap.self_insert = Some(template)`.

New `KeymapParseError` variants (`keymap.rs:161-206`), inserted alongside the existing ones,
same `thiserror` style:
- `InvalidSelfInsertAction { location: String, source: ActionParseError }`
- `SelfInsertMissingArgument { location: String }`
- `SelfInsertMissingHole { location: String, found: String }`

Tests (`crates/config/src/keymap.rs`'s `mod test`, mirroring the existing `parse`/`err`
helpers and insta-snapshot style established for `KeymapParseError`):
- A mode with only `self_insert "Insert($)"` (no grid at all) parses; `ModifierKeymap.self_insert`
  is `Some`, and `.fill('x')` (via the stored `ActionTemplate`) produces `Action::Insert('x')`.
- `self_insert "Insert('a')"` (no hole) → `SelfInsertMissingHole`, snapshot.
- `self_insert "Quit"` (not templatable) → `InvalidSelfInsertAction` wrapping `NotTemplatable`, snapshot.
- A bare `self_insert` node with no argument → `SelfInsertMissingArgument`, snapshot.
- An explicit grid binding still resolves ahead of `self_insert` — no new mechanism needed
  here since `self_insert` is consulted by the *runtime* only after `Keymap::resolve` returns
  `None` (Part D); this phase's tests just confirm both pieces of data (`self_insert` and the
  grid) parse and coexist correctly on the same `ModifierKeymap`.

### Part C — `Mode` becomes an open identifier (`crates/action/src/mode.rs`)

Today `Mode` is a closed 2-variant enum (`Normal | Insert`, `#[derive(strum::EnumString, strum::VariantNames)]`).
For "any mode name works from config alone" to be literally true (not just true for the two
built-in names), `Mode` needs to stop being a fixed enum:

```rust
use std::{fmt, str::FromStr, sync::Arc};

/// A user-nameable editor mode. Any string is a valid mode name; the set of
/// modes is entirely config-driven. `"normal"` and `"insert"` are the only
/// names the editor's own Rust code still references directly (bootstrap /
/// fallback entry points — see Part D) — any other name works purely
/// through config with zero code changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Mode(Arc<str>);

impl Mode {
    pub fn new(name: &str) -> Mode {
        Mode(Arc::from(name.to_ascii_lowercase()))
    }

    pub fn normal() -> Mode { Mode::new("normal") }
    pub fn insert() -> Mode { Mode::new("insert") }

    pub fn as_str(&self) -> &str { &self.0 }
}

impl Default for Mode {
    fn default() -> Self { Mode::normal() }
}

impl FromStr for Mode {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> { Ok(Mode::new(s)) }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", self.0) }
}
```

`Arc<str>` (not `String`/`Box<str>`) so `Mode` stays cheap to clone even though it can no
longer be `Copy` — `Arc`'s `PartialEq`/`Eq`/`Hash` compare the pointee's *contents*, not the
pointer, so this is a drop-in behavioral replacement for the old enum's derived impls.
Lowercasing in `Mode::new` preserves today's `#[strum(ascii_case_insensitive)]` behavior.

**This removes `Mode::VARIANTS`/`EnumString`/`Copy`, which ripples out:**

- `crates/action/src/action.rs`'s `"ChangeMode"` arm (`action.rs:411-418`) calls
  `parse_enum_ident::<Mode>(arg, arg_span)`, which requires `Mode: VariantNames`. Since any
  identifier is now a valid mode name, replace that whole call with `Mode::new(arg)` directly
  — no error path needed any more for this arm specifically. (`parse_enum_ident`'s generic
  bound stays as-is for its other callers — `PasteSource`, `DeleteDirection`, `Anchor`,
  `Direction` — none of those change.)
- Delete `crates/action/src/action.rs`'s `unknown_mode_value` and
  `unknown_mode_value_without_a_close_match` tests (in `mod test`) — `"ChangeMode(Insrt)"` and
  `"ChangeMode(Xyz123)"` are no longer errors, they're just new mode names now. Replace with
  one test asserting `"ChangeMode(Insrt)".parse::<Action>() == Ok(Action::ChangeMode(Mode::new("insrt")))`,
  documenting the new permissiveness is intentional.
- `crates/config/src/keymap.rs`'s `Keymap::parse` (`keymap.rs:129-159`) currently does
  `Mode::from_str(name).map_err(|_| KeymapParseError::UnknownMode { ... })?` — since
  `Mode::from_str` is now infallible, replace with `Mode::new(name)` directly and delete the
  `KeymapParseError::UnknownMode { location, found, valid, did_you_mean }` variant entirely
  (it can never be constructed any more). Drop the now-unused `action::did_you_mean` import
  (`keymap.rs:3`) — confirm nothing else in the file still calls it (as of this writing,
  `UnknownMode` was its only call site in this file).
- Delete `crates/config/src/keymap.rs`'s `unknown_mode_name_reports_a_suggestion` test — no
  longer reachable. Replace with a test demonstrating the new openness instead: a
  `keymap "default" { my_custom_mode { base { home "Quit" } } }` parses successfully and is
  reachable as `keymap.modes.get(&Mode::new("my_custom_mode"))`.
- `src/main.rs`'s `GlobalContext`/`Editor` (`src/main.rs:113-146`) and `event_to_action`
  (`src/main.rs:183-230`) reference `Mode::default()` (fine, unchanged call) and
  `Mode::Insert`/`Mode::Normal` as enum variants (no longer exist) in five places:
  - `event_to_action`'s three `self.context.mode == Mode::Insert` guards (lines 191, 201, 206)
    → `self.context.mode == Mode::insert()`. **These three hardcoded branches are what
    `self_insert` is meant to eventually replace, but that replacement is Phase 3's job (real
    dispatch doesn't exist yet — `event_to_action` today never consults `Keymap` at all).
    This phase only needs the file to keep compiling against the new `Mode` API; it does not
    remove this hardcoding.**
  - `process_action`'s `Action::ChangeMode(mode) => { self.context.mode = mode; self.screen.change_mode(mode) }`
    (`src/main.rs:163-166`) uses `mode` twice — fine when `Mode: Copy`, a use-after-move once
    it isn't. Change to `self.context.mode = mode.clone(); self.screen.change_mode(mode)`
    (cheap: `Arc` refcount bump).
  - `event_to_action`'s `'i' => Some(Action::ChangeMode(Mode::Insert))` (line 209) and
    `KeyCode::Insert => Some(Action::ChangeMode(Mode::Insert))` (line 204) →
    `Action::ChangeMode(Mode::insert())`.
  - `KeyCode::Esc => Some(Action::ChangeMode(Mode::Normal))` (line 215) →
    `Action::ChangeMode(Mode::normal())`.

**Loose ends flagged, not resolved, by this phase** (record the decision so Phase 3 doesn't
re-litigate it, but don't build it now):

- `ModifierKeymap` is a slightly misleading name once it also holds `self_insert` (which isn't
  a modifier grid). Renaming it (e.g. to `ModeKeymap`) is a pure rename with no behavior
  change — fine to do whenever, not required for this phase, skip unless it's free.
- Enter/Tab (`KeyCode::Enter`/`KeyCode::Tab`) are hardcoded separately in `event_to_action`
  today (`src/main.rs:191,201`, `Insert('\n')`/`Insert('\t')`) because they're distinct
  `KeyCode` variants, not `KeyCode::Char`. Decision made in conversation: when Phase 3 wires up
  real dispatch, Enter/Tab should be treated as producing the characters `'\n'`/`'\t'` and go
  through the *same* self-insert-eligible resolution path as `KeyCode::Char`, rather than
  staying separately special-cased. Recorded here for Phase 3 to pick up; not implemented now.

### Part D — forward reference for Phase 3 (real dispatch)

Not implemented in this phase (Phase 3's dispatch code doesn't exist yet — `event_to_action`
never consults `Keymap`/`Layout` today). Recorded here so Phase 3 doesn't miss it: once a
`KeyCode::Char`/Enter/Tab resolves through the position/layout pipeline (`POSITIONAL_KEYMAP_PLAN.md`'s
existing Phase 3 section) and `Keymap::resolve(mode, ...)` returns `None`, check that mode's
`ModifierKeymap.self_insert`; if `Some(template)`, the action is `template.fill(the_char)`
instead of "do nothing". If `None` (today's `Normal` mode, and any mode that doesn't declare
`self_insert`), an unbound char key stays a no-op, exactly as today.

### Verification for this phase

Pure unit/insta tests in `crates/action` and `crates/config`, run with `cargo test -p action -p config`.
No manual/interactive verification is possible yet since nothing wires this into real
keystrokes (that's Phase 3). Also run `cargo build --workspace` to confirm `src/main.rs`'s
`Mode`-API call sites (Part C) still compile.

---

## Phase 2 — Layout config schema + parsing (`crates/config/src/layout.rs`)

Structurally simpler than `Keymap` because **there is no fallback at all** —
every grid is fully explicit. This is a deliberate, non-negotiable choice (see
Decisions above).

### Shape

```rust
pub struct CharRow(pub Vec<Option<char>>); // up to 12 entries; a slot is either an explicit char or `#null` — never `_`

pub struct CharRowSet { pub top: CharRow, pub home: CharRow, pub bottom: CharRow }

pub struct Layout {
    pub base: CharRowSet,
    pub shifted: CharRowSet,
    pub alted: CharRowSet,
    pub ctrled: CharRowSet,
}
```

Because this is a single active selection (not an open-ended named
collection like keymaps), it fits the **same pattern as `theme`**: a scalar
`layout: Option<Layout>` field on `ConfigLayer`, resolved by a
`parse_layout_node` dispatcher analogous to `parse_theme_node`
(`lib.rs:159-194`) — first positional argument, if a bare string, tried as a
built-in name (`Layout::built_in("qwerty")` / `Layout::built_in("ergo-l")`,
mirroring `Theme::built_in`); else treated as a file path; else, if the node
has children, parsed inline. This can plausibly reuse
`#[derive(ConfigField)]` the same way `theme` does, since it's a scalar
`Option<T>`.

### KDL schema (worked example, bundled `default_config/layout/qwerty.kdl`)

```kdl
base {
    top    "q" "w" "e" "r" "t" "y" "u" "i" "o" "p" "[" "]"
    home   "a" "s" "d" "f" "g" "h" "j" "k" "l" ";" "'" #null
    bottom "z" "x" "c" "v" "b" "n" "m" "," "." "/" #null #null
}
shifted {
    top    "Q" "W" "E" "R" "T" "Y" "U" "I" "O" "P" "{" "}"
    home   "A" "S" "D" "F" "G" "H" "J" "K" "L" ":" "\"" #null
    bottom "Z" "X" "C" "V" "B" "N" "M" "<" ">" "?" #null #null
}
alted {
    // AltGr combos — mostly #null on plain US QWERTY, meaningfully populated
    // on layouts like ergo-l where AltGr produces real characters.
    top    #null #null #null #null #null #null #null #null #null #null #null #null
    home   #null #null #null #null #null #null #null #null #null #null #null #null
    bottom #null #null #null #null #null #null #null #null #null #null #null #null
}
ctrled {
    top    #null #null #null #null #null #null #null #null #null #null #null #null
    home   #null #null #null #null #null #null #null #null #null #null #null #null
    bottom #null #null #null #null #null #null #null #null #null #null #null #null
}
```

A user's own `config.kdl` selects one with `layout "qwerty"` (or `"ergo-l"`,
or an inline/file-based custom one), exactly like `theme "monokai"` today.

### Error type (`LayoutParseError`)

Mirrors `KeymapParseError` minus anything fallback-related, plus:
`FallbackNotAllowed { location, index }` — a hard error if `_` ever appears
anywhere in a layout grid (explicitly rejecting the keymap's shorthand here,
since layout must never use it). `InvalidRowEntry` here also rejects
multi-character strings (each slot must be exactly one character or `#null`).

### Bundled presets

`default_config/layout/qwerty.kdl` and `default_config/layout/ergo-l.kdl`
only, per the user's stated personal need. More presets later are additive.
Add a `default_config/config.kdl` default of `layout "qwerty"` unless told
otherwise, so the config has a working layout out of the box.

### Tests

Mirror `theme.rs`: both bundled presets parse cleanly (a test analogous to
`every_built_in_theme_parses_cleanly`, `theme.rs:228-247`); a `_` anywhere in
any grid is a hard parse error; a multi-char string in a slot errors;
name-or-inline-or-file resolution each work, mirroring the existing
`parses_an_inline_theme` / theme-from-file tests in `lib.rs`.

---

## Phase 3 — Real dispatch: char + layout resolver (no kitty needed yet)

This is the payoff phase: the editor actually uses the keymap for real, using
today's crossterm events (no kitty protocol required).

### Position resolution

Given the active `Layout`, build (once, on load) a reverse index:
`HashMap<char, (Modifier, RowKind, usize)>` scanning all four grids. Given an
incoming `crossterm::event::KeyEvent { code: KeyCode::Char(c), .. }`, look `c`
up in that reverse index to get `(modifier, row, col)`. Feed that into
`Keymap::resolve(current_mode, modifier, row, col)` (Phase 1) to get the
`Action`.

### Open questions to resolve *during* this phase (not now)

- Exactly how crossterm's `KeyModifiers` bitflags (SHIFT/ALT/CONTROL)
  interact with the character it reports varies by terminal and modifier —
  many terminals bake Shift into the character itself (`'A'` rather than
  `'a'` + `SHIFT`) but report Ctrl/Alt as flags alongside an *unchanged* base
  character, while AltGr and legacy control-byte handling both add more
  wrinkles. The char-vs-layout reverse lookup above should be the primary
  source of truth (since it's driven by what the user's layout actually says
  is at each modifier grid), with crossterm's modifier flags used only as a
  disambiguating hint — but the precise algorithm needs to be worked out
  against real terminal behavior (a small empirical spike), not designed
  blind here.
- Whether/how the non-`KeyCode::Char` keys (arrows, Backspace, Enter, etc.,
  see Phase 1's note) get folded into this same positional model, stay
  hardcoded as they are today, or get their own small dedicated keymap
  section. Decide when this phase is actually being implemented.

### Wiring

Replace the char-handling branches of `Editor::event_to_action`
(`src/main.rs:220-229`, today's `match c { 'q' => Quit, 'i' => ..., ' ' =>
..., _ => None }`) with a call into the resolver above, keyed off
`self.context.mode` and the loaded `Config`'s default keymap + active layout.
Non-char `KeyCode`s are untouched by this phase (per the open question above,
unless resolved otherwise during implementation).

Additionally (see Phase 1.5, Part D): once a char/Enter/Tab key resolves through this pipeline
and `Keymap::resolve` returns `None`, check the current mode's `ModifierKeymap.self_insert`
before falling back to "do nothing" — if it's `Some(template)`, the action is
`template.fill(the_char)`. This is what finally replaces `event_to_action`'s hardcoded
`self.context.mode == Mode::insert()` branches (Phase 1.5 only adapted them to keep compiling
against the new `Mode` API; removing them is this phase's job). Per Phase 1.5's decision,
Enter/Tab should be folded into this same char-producing path (as `'\n'`/`'\t'`) rather than
staying separately hardcoded.

### Verify

Build the editor, run it in a real terminal with the bundled `qwerty` layout
and placeholder default keymap active, confirm the ported `Quit`/
`ChangeMode(Insert)`/`OpenPopup` bindings fire from their new (arbitrary)
physical positions instead of `'q'`/`'i'`/`' '` directly. Confirm Shift-held
variants correctly resolve through the layout's `shifted` grid.

---

## Phase 4 — Kitty protocol support (later, separate effort)

Not scoped in detail here — flagged so it's not forgotten and so Phase 3's
design doesn't foreclose it.

- Crossterm 0.29 drops the kitty CSI-u "base layout key" field
  (`crossterm-0.29.0/src/event/sys/unix/parse.rs:496-609`). Need to either
  patch/fork crossterm to surface it, or write a standalone low-level kitty
  CSI-u parser bypassing crossterm's event reader for keyboard events.
- Also need to negotiate the kitty keyboard protocol at startup
  (`PushKeyboardEnhancementFlags`/`PopKeyboardEnhancementFlags`, which exist
  in crossterm already but are never sent today) and detect whether the
  terminal actually supports it, falling back to the Phase 3 char/layout
  resolver when it doesn't.
- Once available, a kitty-based resolver produces the same
  `(Modifier, RowKind, usize)` triple that Phase 3's char/layout resolver
  produces, directly from the base-layout-key field — no `layout` config
  needed at all in that path, since kitty already normalizes across layouts.
  The `layout` config remains relevant only as the fallback for terminals
  without kitty support.
- QMK-style keymap-layer switching (`layer` field from Phase 1: permanent /
  one-shot-next-key / momentary-while-held) is also undesigned and could be
  tackled around the same time or independently.

---

## Summary of what's explicitly NOT being decided/built yet

- Actual ergonomic key assignments (which action goes on which physical
  position) — ergonomics bikeshedding is a separate pass once the mechanism
  works.
- QMK-style layer switching/activation logic.
- Kitty protocol / crossterm patching (Phase 4).
- Any modifier combos beyond base/shift/alt/ctrl (e.g. shift+alt) — not
  requested, not planned.
