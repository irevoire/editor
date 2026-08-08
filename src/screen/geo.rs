#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord)]
pub struct ScreenCoord {
    pub line: u16,
    pub column: u16,
}

impl ScreenCoord {
    pub const fn zero() -> Self {
        Self { line: 0, column: 0 }
    }
}

// We implement PartialOrd manually because we want to be 100% sure that the line
// takes priority over the column.
impl PartialOrd for ScreenCoord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.line.partial_cmp(&other.line) {
            Some(core::cmp::Ordering::Equal) => self.column.partial_cmp(&other.column),
            ord => ord,
        }
    }
}

/// Represent a rectangular area of the screen from it's top left coordinate
/// and bottom right coordinate.
/// The coordinates are INCLUDED.
/// Which means the following area is valid and only contains the cell (0, 0):
///
/// ```rust
/// ScreenArea::new(ScreenCoord::zero(), ScreenCoord::zero());
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ScreenArea {
    top_left: ScreenCoord,
    bottom_right: ScreenCoord,
}

impl ScreenArea {
    pub fn new(top_left: ScreenCoord, bottom_right: ScreenCoord) -> Self {
        assert!(
            top_left.line <= bottom_right.line && top_left.column <= bottom_right.column,
            "{top_left:?} > {bottom_right:?}"
        );

        Self {
            top_left,
            bottom_right,
        }
    }

    pub fn width(self) -> u16 {
        self.bottom_right.column - self.top_left.column + 1
    }

    pub fn height(self) -> u16 {
        self.bottom_right.line - self.top_left.line + 1
    }

    pub fn split_after_internal_column(self, column: u16) -> (Self, Self) {
        assert!(
            self.width() > column,
            "Tried to split an area of width {} on column {}",
            self.width(),
            column
        );
        (
            self.shrink_to_internal_area(ScreenArea::new(
                ScreenCoord::zero(),
                ScreenCoord {
                    line: self.height() - 1,
                    column,
                },
            )),
            self.shrink_to_internal_area(ScreenArea::new(
                ScreenCoord {
                    line: 0,
                    column: column + 1,
                },
                ScreenCoord {
                    line: self.height() - 1,
                    column: self.width() - 1,
                },
            )),
        )
    }

    pub fn split_after_internal_line(self, line: u16) -> (Self, Self) {
        assert!(
            self.height() > line,
            "Tried to split an area of height {} on line {}",
            self.height(),
            line
        );
        (
            self.shrink_to_internal_area(ScreenArea::new(
                ScreenCoord::zero(),
                ScreenCoord {
                    line,
                    column: self.width() - 1,
                },
            )),
            self.shrink_to_internal_area(ScreenArea::new(
                ScreenCoord {
                    line: line + 1,
                    column: 0,
                },
                ScreenCoord {
                    line: self.height() - 1,
                    column: self.width() - 1,
                },
            )),
        )
    }

    /// Check if the coord is contained in self.
    /// The coord indexing system starts from the bottom left of self.
    pub fn contains_internal_coord(self, other: ScreenCoord) -> bool {
        other.column < self.width() && other.line < self.height()
    }

    /// Check if the area is contained in self.
    /// The coord indexing system starts from the bottom left of self.
    pub fn contains_internal_area(self, other: Self) -> bool {
        other.bottom_right.column < self.width() && other.bottom_right.line < self.height()
    }

    /// Return the `other` screen area indexed in the same system as `self`.
    /// Panic if the area is not contained in `self`.
    pub fn shrink_to_internal_area(self, other: ScreenArea) -> ScreenArea {
        assert!(self.contains_internal_area(other));
        ScreenArea::new(
            ScreenCoord {
                line: self.top_left.line + other.top_left.line,
                column: self.top_left.column + other.top_left.column,
            },
            ScreenCoord {
                line: self.top_left.line + other.bottom_right.line,
                column: self.top_left.column + other.bottom_right.column,
            },
        )
    }

    /// Translate an internal as an external coordinate.
    /// Panic if the coordinate is not contained in the area.
    pub fn translate_internal_coord(&self, other: ScreenCoord) -> ScreenCoord {
        assert!(self.contains_internal_coord(other));
        ScreenCoord {
            line: other.line + self.top_left.line,
            column: other.column + self.top_left.column,
        }
    }
}

#[cfg(test)]
mod test {
    use insta::assert_debug_snapshot;

    use crate::screen::geo::{ScreenArea, ScreenCoord};

    #[test]
    fn test_area_size() {
        let area = ScreenArea::new(
            ScreenCoord::zero(),
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        assert_eq!(area.width(), 41);
        assert_eq!(area.height(), 21);

        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        assert_eq!(area.width(), 21);
        assert_eq!(area.height(), 11);
    }

    #[test]
    fn test_contains_coords() {
        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        // The point is relative to the start of the area so it's inside
        // even though the area starts at 10,20
        let ret = area.contains_internal_coord(ScreenCoord { line: 0, column: 0 });
        assert!(ret);

        // The areas includes their bounds, which means 10,20 is included
        let ret = area.contains_internal_coord(ScreenCoord {
            line: 10,
            column: 20,
        });
        assert!(ret);

        let ret = area.contains_internal_coord(ScreenCoord {
            line: 5,
            column: 10,
        });
        assert!(ret);
    }

    #[test]
    fn test_doesnt_contains_coords() {
        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        let ret = area.contains_internal_coord(ScreenCoord {
            line: 11,
            column: 20,
        });
        assert!(!ret);
        let ret = area.contains_internal_coord(ScreenCoord {
            line: 10,
            column: 21,
        });
        assert!(!ret);
        let ret = area.contains_internal_coord(ScreenCoord {
            line: 11,
            column: 21,
        });
        assert!(!ret);
        let ret = area.contains_internal_coord(ScreenCoord {
            line: 20,
            column: 40,
        });
        assert!(!ret);
    }

    #[test]
    fn test_contains_area() {
        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        // The point is relative to the start of the area so it's inside
        // even though the area starts at 10,20
        let ret = area.contains_internal_area(ScreenArea::new(
            ScreenCoord::zero(),
            ScreenCoord {
                line: 10,
                column: 20,
            },
        ));
        assert!(ret);

        let ret = area.contains_internal_area(ScreenArea::new(
            ScreenCoord { line: 2, column: 8 },
            ScreenCoord {
                line: 7,
                column: 16,
            },
        ));
        assert!(ret);

        let ret = area.contains_internal_area(ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 10,
                column: 20,
            },
        ));
        assert!(ret);
    }

    #[test]
    fn test_doesnt_contains_area() {
        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        let ret = area.contains_internal_area(ScreenArea::new(
            ScreenCoord::zero(),
            ScreenCoord {
                line: 10,
                column: 21,
            },
        ));
        assert!(!ret);
        let ret = area.contains_internal_area(ScreenArea::new(
            ScreenCoord::zero(),
            ScreenCoord {
                line: 11,
                column: 20,
            },
        ));
        assert!(!ret);
        let ret = area.contains_internal_area(ScreenArea::new(
            ScreenCoord::zero(),
            ScreenCoord {
                line: 11,
                column: 21,
            },
        ));
        assert!(!ret);
        let ret = area.contains_internal_area(ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        ));
        assert!(!ret);
    }

    #[test]
    #[should_panic]
    fn area_inverted_column() {
        let _area = ScreenArea::new(
            ScreenCoord { line: 0, column: 2 },
            ScreenCoord { line: 9, column: 1 },
        );
    }

    #[test]
    #[should_panic]
    fn area_inverted_line() {
        let _area = ScreenArea::new(
            ScreenCoord { line: 2, column: 0 },
            ScreenCoord { line: 1, column: 9 },
        );
    }

    #[test]
    fn test_translate_internal_coords() {
        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        let coord = area.translate_internal_coord(ScreenCoord { line: 0, column: 0 });
        assert_eq!(
            coord,
            ScreenCoord {
                line: 10,
                column: 20
            }
        );
        let coord = area.translate_internal_coord(ScreenCoord {
            line: 5,
            column: 10,
        });
        assert_eq!(
            coord,
            ScreenCoord {
                line: 15,
                column: 30
            }
        );
        let coord = area.translate_internal_coord(ScreenCoord {
            line: 10,
            column: 20,
        });
        assert_eq!(
            coord,
            ScreenCoord {
                line: 20,
                column: 40
            }
        );
    }

    #[test]
    #[should_panic]
    fn test_translate_internal_coords_oob_line() {
        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        let coord = area.translate_internal_coord(ScreenCoord {
            line: 11,
            column: 0,
        });
        assert_eq!(
            coord,
            ScreenCoord {
                line: 10,
                column: 20
            }
        );
    }

    #[test]
    #[should_panic]
    fn test_translate_internal_coords_oob_column() {
        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        let coord = area.translate_internal_coord(ScreenCoord {
            line: 0,
            column: 21,
        });
        assert_eq!(
            coord,
            ScreenCoord {
                line: 10,
                column: 20
            }
        );
    }

    #[test]
    #[should_panic]
    fn test_translate_internal_coords_oob_both() {
        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        let coord = area.translate_internal_coord(ScreenCoord {
            line: 11,
            column: 21,
        });
        assert_eq!(
            coord,
            ScreenCoord {
                line: 10,
                column: 20
            }
        );
    }

    #[test]
    fn test_split_on_internal_column() {
        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        let (left, right) = area.split_after_internal_column(0);
        assert_debug_snapshot!(left, @r"
        ScreenArea {
            top_left: ScreenCoord {
                line: 10,
                column: 20,
            },
            bottom_right: ScreenCoord {
                line: 20,
                column: 20,
            },
        }
        ");
        assert_debug_snapshot!(right, @r"
        ScreenArea {
            top_left: ScreenCoord {
                line: 10,
                column: 21,
            },
            bottom_right: ScreenCoord {
                line: 20,
                column: 40,
            },
        }
        ");
        let (left, right) = right.split_after_internal_column(8);
        assert_debug_snapshot!(left, @r"
        ScreenArea {
            top_left: ScreenCoord {
                line: 10,
                column: 21,
            },
            bottom_right: ScreenCoord {
                line: 20,
                column: 29,
            },
        }
        ");
        assert_debug_snapshot!(right, @r"
        ScreenArea {
            top_left: ScreenCoord {
                line: 10,
                column: 30,
            },
            bottom_right: ScreenCoord {
                line: 20,
                column: 40,
            },
        }
        ");
    }

    #[test]
    #[should_panic]
    fn test_split_on_internal_column_oob() {
        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        area.split_after_internal_column(20);
    }

    #[test]
    fn test_split_on_internal_line() {
        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        let (top, bottom) = area.split_after_internal_line(0);
        assert_debug_snapshot!(top, @r"
        ScreenArea {
            top_left: ScreenCoord {
                line: 10,
                column: 20,
            },
            bottom_right: ScreenCoord {
                line: 10,
                column: 40,
            },
        }
        ");
        assert_debug_snapshot!(bottom, @r"
        ScreenArea {
            top_left: ScreenCoord {
                line: 11,
                column: 20,
            },
            bottom_right: ScreenCoord {
                line: 20,
                column: 40,
            },
        }
        ");
        let (top, bottom) = bottom.split_after_internal_line(8);
        assert_debug_snapshot!(top, @r"
        ScreenArea {
            top_left: ScreenCoord {
                line: 11,
                column: 20,
            },
            bottom_right: ScreenCoord {
                line: 19,
                column: 40,
            },
        }
        ");
        assert_debug_snapshot!(bottom, @r"
        ScreenArea {
            top_left: ScreenCoord {
                line: 20,
                column: 20,
            },
            bottom_right: ScreenCoord {
                line: 20,
                column: 40,
            },
        }
        ");
    }

    #[test]
    #[should_panic]
    fn test_split_on_internal_line_oob() {
        let area = ScreenArea::new(
            ScreenCoord {
                line: 10,
                column: 20,
            },
            ScreenCoord {
                line: 20,
                column: 40,
            },
        );
        area.split_after_internal_line(10);
    }
}
