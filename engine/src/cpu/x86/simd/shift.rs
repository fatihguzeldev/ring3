use super::{PackedKind as P, ShuffleKind as S, packed};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Lanes(P),
    Bytes(S),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShiftOperation {
    kind: Kind,
    lane_bytes: u8,
}
impl ShiftOperation {
    pub(crate) fn lanes(kind: P, lane_bytes: u8) -> Option<Self> {
        let valid = match kind {
            P::ShiftLeft | P::ShiftRight => matches!(lane_bytes, 2 | 4 | 8),
            P::ShiftRightSigned => matches!(lane_bytes, 2 | 4),
            _ => false,
        };
        valid.then_some(Self {
            kind: Kind::Lanes(kind),
            lane_bytes,
        })
    }
    pub(crate) fn bytes(left: bool) -> Self {
        Self {
            kind: Kind::Bytes(if left {
                S::ShiftBytesLeft
            } else {
                S::ShiftBytesRight
            }),
            lane_bytes: 1,
        }
    }
    pub(crate) fn lane_kind(self) -> Option<P> {
        if let Kind::Lanes(kind) = self.kind {
            Some(kind)
        } else {
            None
        }
    }
    pub(crate) fn byte_kind(self) -> Option<S> {
        if let Kind::Bytes(kind) = self.kind {
            Some(kind)
        } else {
            None
        }
    }
    pub(crate) fn lane_bytes(self) -> u8 {
        self.lane_bytes
    }
    pub(crate) fn evaluate(
        self,
        source: [u8; 16],
        count: [u8; 16],
        immediate: Option<u8>,
    ) -> [u8; 16] {
        match self.kind {
            Kind::Lanes(kind) => packed::packed(
                kind,
                source,
                count,
                16,
                usize::from(self.lane_bytes),
                immediate,
            ),
            Kind::Bytes(kind) => {
                packed::shuffle(kind, source, [0; 16], 16, 1, immediate.unwrap(), [0; 16]).0
            }
        }
    }
}
