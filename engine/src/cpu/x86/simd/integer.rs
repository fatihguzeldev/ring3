use super::{PackedKind, packed};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IntegerLaneOperation {
    kind: PackedKind,
    lane_bytes: u8,
}

impl IntegerLaneOperation {
    pub(crate) fn new(kind: PackedKind, lane_bytes: u8) -> Option<Self> {
        use PackedKind::*;
        let valid = match kind {
            Add | Subtract | Equal | GreaterSigned => matches!(lane_bytes, 1 | 2 | 4 | 8),
            AddSignedSaturate
            | AddUnsignedSaturate
            | SubtractSignedSaturate
            | SubtractUnsignedSaturate
            | AverageUnsigned => matches!(lane_bytes, 1 | 2),
            MinimumSigned | MaximumSigned | MinimumUnsigned | MaximumUnsigned | Absolute | Sign => {
                matches!(lane_bytes, 1 | 2 | 4)
            }
            _ => false,
        };
        valid.then_some(Self { kind, lane_bytes })
    }

    pub(crate) fn kind(self) -> PackedKind {
        self.kind
    }

    pub(crate) fn lane_bytes(self) -> u8 {
        self.lane_bytes
    }

    pub(crate) fn unary(self) -> bool {
        self.kind == PackedKind::Absolute
    }

    pub(crate) fn evaluate(self, first: [u8; 16], second: [u8; 16]) -> [u8; 16] {
        packed::packed(
            self.kind,
            first,
            second,
            16,
            usize::from(self.lane_bytes),
            None,
        )
    }
}
