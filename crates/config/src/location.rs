use std::fmt;

/// A location in the kdl config. It's cheap to clone and build.
///
/// The whole config is meant to share a single [`Location::Root`], created
/// once before any parsing starts; every section then only ever appends to
/// it, so the same root can be threaded through `status_bar`, `theme`,
/// `keymap`, etc.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Location<'a> {
    /// The root of the config document. Renders as nothing on its own.
    Root,
    /// One more named segment appended to `prev`, rendered as `{prev} > {name}`
    /// (or just `{name}` when `prev` is the root).
    Key {
        name: &'a str,
        prev: &'a Location<'a>,
    },
    /// One more indexed segment appended to `prev`, rendered as `{prev}[{index}]`.
    Index {
        index: usize,
        prev: &'a Location<'a>,
    },
}

impl<'a> Location<'a> {
    fn is_root(&self) -> bool {
        matches!(self, Location::Root)
    }

    pub(crate) fn key(&'a self, name: &'a str) -> Location<'a> {
        Location::Key { name, prev: self }
    }

    pub(crate) fn index(&'a self, index: usize) -> Location<'a> {
        Location::Index { index, prev: self }
    }
}

impl fmt::Display for Location<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Location::Root => Ok(()),
            Location::Key { name, prev } if prev.is_root() => write!(f, "{name}"),
            Location::Key { name, prev } => write!(f, "{prev} > {name}"),
            Location::Index { index, prev } => write!(f, "{prev}[{index}]"),
        }
    }
}
