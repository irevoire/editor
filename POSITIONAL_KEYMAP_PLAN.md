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
  node's name via `Mode::from_str` (already derived via `strum::EnumString`
  on `Mode`, `src/main.rs:104-118`) rather than two fixed struct fields. A
  future `Mode` variant (e.g. `Visual`) becomes configurable with zero
  parser changes.
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
