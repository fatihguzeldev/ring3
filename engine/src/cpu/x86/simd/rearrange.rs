use super::{ShuffleKind as S, packed};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RearrangeOperation {
    kind: S,
    lane_bytes: u8,
    immediate: u8,
}

impl RearrangeOperation {
    pub(crate) fn new(kind: S, lane_bytes: u8, immediate: u8) -> Option<Self> {
        let valid = match kind {
            S::UnpackLow | S::UnpackHigh => matches!(lane_bytes, 1 | 2 | 4 | 8) && immediate == 0,
            S::PackSigned | S::PackUnsigned => matches!(lane_bytes, 1 | 2) && immediate == 0,
            S::Dwords => lane_bytes == 4,
            S::HighWords | S::LowWords | S::BlendImmediate => lane_bytes == 2,
            S::Bytes | S::BlendVariable => lane_bytes == 1 && immediate == 0,
            S::AlignRight => lane_bytes == 1,
            _ => kind.widening().is_some_and(|(from, _)| {
                matches!(lane_bytes, 2 | 4 | 8) && lane_bytes > from && immediate == 0
            }),
        };
        valid.then_some(Self {
            kind,
            lane_bytes,
            immediate,
        })
    }

    pub(crate) fn kind(self) -> S {
        self.kind
    }
    pub(crate) fn lane_bytes(self) -> u8 {
        self.lane_bytes
    }
    pub(crate) fn immediate(self) -> u8 {
        self.immediate
    }
    pub(crate) fn unary(self) -> bool {
        self.kind.widening().is_some()
            || matches!(self.kind, S::Dwords | S::HighWords | S::LowWords)
    }
    pub(crate) fn uses_mask(self) -> bool {
        self.kind == S::BlendVariable
    }
    pub(crate) fn memory_bytes(self) -> u8 {
        self.kind
            .widening()
            .map_or(16, |(from, _)| 16 / self.lane_bytes * from)
    }
    pub(crate) fn evaluate(self, first: [u8; 16], second: [u8; 16], mask: [u8; 16]) -> [u8; 16] {
        packed::shuffle(
            self.kind,
            first,
            second,
            16,
            usize::from(self.lane_bytes),
            self.immediate,
            mask,
        )
        .0
    }
}
