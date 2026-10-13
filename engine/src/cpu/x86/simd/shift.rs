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
    pub(crate) fn supports_elements(self) -> bool {
        matches!(
            (self.lane_kind(), self.lane_bytes),
            (Some(P::ShiftLeft | P::ShiftRight), 4 | 8) | (Some(P::ShiftRightSigned), 4)
        )
    }
    pub(crate) fn evaluate_elements(self, source: [u8; 16], counts: [u8; 16]) -> [u8; 16] {
        let kind = self.lane_kind().expect("validated element shift");
        let width = usize::from(self.lane_bytes);
        let mut value = [0; 16];
        for offset in (0..16).step_by(width) {
            let source = packed::unsigned(&source, offset, width);
            let count = packed::unsigned(&counts, offset, width);
            packed::put(
                &mut value,
                offset,
                width,
                lane_value(kind, source, width, count),
            );
        }
        value
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

pub(super) fn lane_value(kind: P, value: u64, width: usize, count: u64) -> u64 {
    let bits = width * 8;
    match kind {
        P::ShiftLeft => {
            if count >= bits as u64 {
                0
            } else {
                value << count
            }
        }
        P::ShiftRight => {
            if count >= bits as u64 {
                0
            } else {
                value >> count
            }
        }
        P::ShiftRightSigned => {
            (packed::signed(value, width) >> count.min((bits - 1) as u64)) as u64
        }
        _ => unreachable!("validated scalar lane shift"),
    }
}
