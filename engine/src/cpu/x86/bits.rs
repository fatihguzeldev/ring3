#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarWidth {
    Word,
    Dword,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitCountKind {
    TrailingZero,
    LeadingZero,
    Population,
}
