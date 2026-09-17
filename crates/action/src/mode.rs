#[derive(
    Default, Copy, Clone, Debug, PartialEq, Eq, Hash, strum::VariantNames, strum::EnumString
)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Insert => "insert",
        }
    }
}
