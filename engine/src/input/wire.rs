use super::{
    BUTTON_COUNT, InputContext, InputError, InputKind, InputOrigin, InputRecord, KEY_COUNT,
    MAX_BATCH_EVENTS, QueueRead, TimedInputEvent,
};

pub const VERSION: u16 = 1;
pub const BATCH_HEADER_SIZE: usize = 32;
pub const EVENT_SIZE: usize = 32;
pub const QUEUE_HEADER_SIZE: usize = 32;
pub const QUEUE_RECORD_SIZE: usize = 48;

pub struct EncodedBatch<'a> {
    epoch: u64,
    sequence: u64,
    latest_timestamp_ns: u64,
    records: &'a [u8],
}

impl EncodedBatch<'_> {
    pub fn session_epoch(&self) -> u64 {
        self.epoch
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn event_count(&self) -> usize {
        self.records.len() / EVENT_SIZE
    }

    pub fn latest_timestamp_ns(&self) -> u64 {
        self.latest_timestamp_ns
    }

    pub(super) fn events(&self) -> impl ExactSizeIterator<Item = TimedInputEvent> + Clone {
        self.records.chunks_exact(EVENT_SIZE).map(decode_validated)
    }
}

pub fn inspect_batch(bytes: &[u8]) -> Result<EncodedBatch<'_>, InputError> {
    if bytes.len() < BATCH_HEADER_SIZE
        || &bytes[..4] != b"R3IB"
        || u16::from_le_bytes(bytes[4..6].try_into().unwrap()) != VERSION
        || u16::from_le_bytes(bytes[6..8].try_into().unwrap()) != EVENT_SIZE as u16
        || word(bytes, 28) != 0
    {
        return Err(InputError::InvalidWire);
    }
    let count = word(bytes, 24) as usize;
    if count == 0 || count > MAX_BATCH_EVENTS {
        return Err(InputError::InvalidWire);
    }
    let expected = count
        .checked_mul(EVENT_SIZE)
        .and_then(|length| length.checked_add(BATCH_HEADER_SIZE))
        .ok_or(InputError::InvalidWire)?;
    if bytes.len() != expected {
        return Err(InputError::InvalidWire);
    }
    let mut latest = 0;
    for (index, record) in bytes[BATCH_HEADER_SIZE..]
        .chunks_exact(EVENT_SIZE)
        .enumerate()
    {
        validate_event(record)?;
        let timestamp = long(record, 0);
        if index != 0 && timestamp < latest {
            return Err(InputError::TimeOrder);
        }
        latest = timestamp;
    }
    Ok(EncodedBatch {
        epoch: long(bytes, 8),
        sequence: long(bytes, 16),
        latest_timestamp_ns: latest,
        records: &bytes[BATCH_HEADER_SIZE..],
    })
}

impl InputContext {
    pub fn peek_encoded(&self, maximum: usize, output: &mut [u8]) -> Result<QueueRead, InputError> {
        let count = maximum.min(self.queued_events());
        let bytes = count
            .checked_mul(QUEUE_RECORD_SIZE)
            .and_then(|length| length.checked_add(QUEUE_HEADER_SIZE))
            .ok_or(InputError::OutputCapacity)?;
        if output.len() < bytes || count > u32::MAX as usize || bytes > u32::MAX as usize {
            return Err(InputError::OutputCapacity);
        }
        output[..bytes].fill(0);
        output[..4].copy_from_slice(b"R3IQ");
        output[4..6].copy_from_slice(&VERSION.to_le_bytes());
        output[6..8].copy_from_slice(&(QUEUE_RECORD_SIZE as u16).to_le_bytes());
        output[8..16].copy_from_slice(&self.session_epoch().to_le_bytes());
        output[16..20].copy_from_slice(&(count as u32).to_le_bytes());
        output[24..28].copy_from_slice(&(bytes as u32).to_le_bytes());
        for index in 0..count {
            let start = QUEUE_HEADER_SIZE + index * QUEUE_RECORD_SIZE;
            encode_record(
                self.record(index),
                &mut output[start..start + QUEUE_RECORD_SIZE],
            );
        }
        Ok(QueueRead { count, bytes })
    }

    pub fn read_encoded(
        &mut self,
        maximum: usize,
        output: &mut [u8],
    ) -> Result<QueueRead, InputError> {
        let read = self.peek_encoded(maximum, output)?;
        self.consume(read.count)?;
        Ok(read)
    }
}

fn validate_event(record: &[u8]) -> Result<(), InputError> {
    if long(record, 24) != 0 {
        return Err(InputError::InvalidWire);
    }
    let a = word(record, 12);
    let b = word(record, 16);
    let c = word(record, 20);
    let valid = match word(record, 8) {
        1 => a < KEY_COUNT as u32 && b <= 1 && c == 0,
        2 => a < BUTTON_COUNT as u32 && b <= 1 && c == 0,
        3 => true,
        4 | 5 => a <= 1 && b == 0 && c == 0,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(InputError::InvalidWire)
    }
}

fn decode_validated(record: &[u8]) -> TimedInputEvent {
    let a = word(record, 12);
    let b = word(record, 16);
    let c = word(record, 20);
    let kind = match word(record, 8) {
        1 => InputKind::Key {
            code: a as u16,
            pressed: b != 0,
        },
        2 => InputKind::Button {
            button: a as u8,
            pressed: b != 0,
        },
        3 => InputKind::Relative {
            dx: a as i32,
            dy: b as i32,
            wheel: c as i32,
        },
        4 => InputKind::Focus { focused: a != 0 },
        5 => InputKind::Capture { captured: a != 0 },
        _ => unreachable!("encoded batch retains only validated event records"),
    };
    TimedInputEvent {
        timestamp_ns: long(record, 0),
        kind,
    }
}

fn encode_record(record: InputRecord, output: &mut [u8]) {
    output[..8].copy_from_slice(&record.event.timestamp_ns.to_le_bytes());
    let (kind, a, b, c) = match record.event.kind {
        InputKind::Key { code, pressed } => (1_u32, u32::from(code), u32::from(pressed), 0),
        InputKind::Button { button, pressed } => (2, u32::from(button), u32::from(pressed), 0),
        InputKind::Relative { dx, dy, wheel } => (3, dx as u32, dy as u32, wheel as u32),
        InputKind::Focus { focused } => (4, u32::from(focused), 0, 0),
        InputKind::Capture { captured } => (5, u32::from(captured), 0, 0),
    };
    for (offset, value) in [(8, kind), (12, a), (16, b), (20, c)] {
        output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    output[32..40].copy_from_slice(&record.batch_sequence.to_le_bytes());
    let origin: u32 = match record.origin {
        InputOrigin::Host => 0,
        InputOrigin::FocusRelease => 1,
        InputOrigin::Window => 2,
    };
    output[40..44].copy_from_slice(&origin.to_le_bytes());
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn long(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
