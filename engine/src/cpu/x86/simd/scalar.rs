use super::{ShuffleKind as S, packed};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScalarWidth {
    Byte,
    Word,
    Dword,
}
impl ScalarWidth {
    pub(crate) fn bytes(self) -> u8 {
        match self {
            Self::Byte => 1,
            Self::Word => 2,
            Self::Dword => 4,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScalarOperation {
    Insert { width: ScalarWidth, immediate: u8 },
    Extract { width: ScalarWidth, immediate: u8 },
    MaskBytes,
}
impl ScalarOperation {
    pub(crate) fn evaluate(self, first: [u8; 16], second: [u8; 16]) -> ([u8; 16], Option<u32>) {
        let (kind, width, immediate) = match self {
            Self::Insert { width, immediate } => (
                match width {
                    ScalarWidth::Byte => S::InsertByte,
                    ScalarWidth::Word => S::InsertWord,
                    ScalarWidth::Dword => S::InsertDword,
                },
                width.bytes(),
                immediate,
            ),
            Self::Extract { width, immediate } => (
                match width {
                    ScalarWidth::Byte => S::ExtractByte,
                    ScalarWidth::Word => S::ExtractWord,
                    ScalarWidth::Dword => S::ExtractDword,
                },
                width.bytes(),
                immediate,
            ),
            Self::MaskBytes => (S::MaskBytes, 1, 0),
        };
        packed::shuffle(
            kind,
            first,
            second,
            16,
            usize::from(width),
            immediate,
            [0; 16],
        )
    }
}
