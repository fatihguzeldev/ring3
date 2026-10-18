use crate::input::dinput::DeviceKind32;
use ring3_observability::{Field, Level, categories, current};

const KEYS: [&str; 16] = [
    "key_0", "key_1", "key_2", "key_3", "key_4", "key_5", "key_6", "key_7", "key_8", "key_9",
    "key_10", "key_11", "key_12", "key_13", "key_14", "key_15",
];

pub(super) fn state(pointer: u32, kind: DeviceKind32, bytes: &[u8]) {
    if kind != DeviceKind32::Keyboard {
        return;
    }
    let observer = current();
    if !observer.enabled(Level::Basic, categories::ENGINE) {
        return;
    }
    let nonzero = bytes.iter().filter(|&&value| value != 0).count();
    let mut fields = [Field::unsigned("", 0); 5 + KEYS.len()];
    fields[..5].copy_from_slice(&[
        Field::unsigned("device", u64::from(pointer)),
        Field::unsigned("bytes", bytes.len() as u64),
        Field::unsigned("nonzero_count", nonzero as u64),
        Field::unsigned("recorded_indices", nonzero.min(KEYS.len()) as u64),
        Field::boolean("truncated", nonzero > KEYS.len()),
    ]);
    let mut length = 5;
    for (index, _) in bytes.iter().enumerate().filter(|(_, value)| **value != 0) {
        if length == fields.len() {
            break;
        }
        // these are output byte offsets in the application's configured data format.
        fields[length] = Field::unsigned(KEYS[length - 5], index as u64);
        length += 1;
    }
    observer.instant(
        Level::Basic,
        categories::ENGINE,
        "dinput.keyboard.state",
        &fields[..length],
    );
}

pub(super) fn data(
    pointer: u32,
    requested: u32,
    returned: u32,
    flags: u32,
    result: u32,
    first: &[u8],
) {
    let observer = current();
    if !observer.enabled(Level::Basic, categories::ENGINE) {
        return;
    }
    let mut fields = [Field::unsigned("", 0); 11];
    fields[..6].copy_from_slice(&[
        Field::unsigned("device", u64::from(pointer)),
        Field::unsigned("requested_count", u64::from(requested)),
        Field::unsigned("returned_count", u64::from(returned)),
        Field::unsigned("flags", u64::from(flags)),
        Field::unsigned("result", u64::from(result)),
        Field::unsigned("recorded_events", u64::from(!first.is_empty())),
    ]);
    let mut length = 6;
    for (name, word) in [
        "first_offset",
        "first_value",
        "first_timestamp_ms",
        "first_sequence",
        "first_app_data",
    ]
    .into_iter()
    .zip(first.chunks_exact(4))
    {
        fields[length] = Field::unsigned(
            name,
            u64::from(u32::from_le_bytes(word.try_into().unwrap())),
        );
        length += 1;
    }
    observer.instant(
        Level::Basic,
        categories::ENGINE,
        "dinput.device.data",
        &fields[..length],
    );
}
