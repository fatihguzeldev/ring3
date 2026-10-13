use super::{PackedKind as P, packed};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MultiplyOperation {
    kind: P,
    lane_bytes: u8,
    immediate: u8,
}
impl MultiplyOperation {
    pub(crate) fn new(kind: P, lane_bytes: u8, immediate: u8) -> Option<Self> {
        let valid = match kind {
            P::MultiplyLow => matches!(lane_bytes, 2 | 4),
            P::MultiplyHighSigned
            | P::MultiplyHighUnsigned
            | P::MultiplyAddWords
            | P::MultiplyHighRoundSigned
            | P::HorizontalAddSignedSaturate
            | P::HorizontalSubtractSignedSaturate
            | P::MinimumPositionUnsignedWord => lane_bytes == 2,
            P::MultiplyUnsignedDword | P::MultiplySignedDword => lane_bytes == 4,
            P::SumAbsoluteByteDifferences
            | P::MultiplyAddUnsignedSignedBytes
            | P::MultipleSumAbsoluteByteDifferences => lane_bytes == 1,
            P::HorizontalAdd | P::HorizontalSubtract => matches!(lane_bytes, 2 | 4),
            _ => false,
        };
        (valid && (kind == P::MultipleSumAbsoluteByteDifferences || immediate == 0)).then_some(
            Self {
                kind,
                lane_bytes,
                immediate,
            },
        )
    }
    pub(crate) fn kind(self) -> P {
        self.kind
    }
    pub(crate) fn lane_bytes(self) -> u8 {
        self.lane_bytes
    }
    pub(crate) fn immediate(self) -> u8 {
        self.immediate
    }
    pub(crate) fn unary(self) -> bool {
        self.kind == P::MinimumPositionUnsignedWord
    }
    pub(crate) fn evaluate(self, a: [u8; 16], b: [u8; 16]) -> [u8; 16] {
        packed::packed(
            self.kind,
            a,
            b,
            16,
            usize::from(self.lane_bytes),
            (self.kind == P::MultipleSumAbsoluteByteDifferences).then_some(self.immediate),
        )
    }
}
