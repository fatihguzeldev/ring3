use super::support::*;
use crate::{
    input::{InputKind, TimedInputEvent, dinput::InterfaceKind32},
    memory::GuestAddress,
    process::windows::dinput::memory::words,
};
use ring3_observability::{CategorySelection, Config, Level, Observer, categories};
use std::collections::BTreeMap;

fn read_outputs(observer: &Observer) -> (Vec<u8>, [u32; 5]) {
    let _capture = observer.enter();
    let (mut runtime, mut memory, pointer, mut source) = runtime(true);
    keyboard_format(&mut runtime, &mut memory, pointer);
    runtime
        .device_mut(pointer)
        .unwrap()
        .native
        .acquire(&source, 0)
        .unwrap();
    source
        .publish_physical(
            [
                TimedInputEvent {
                    timestamp_ns: 2_000_000,
                    kind: InputKind::Key {
                        code: 30,
                        pressed: true,
                    },
                },
                TimedInputEvent {
                    timestamp_ns: 3_000_000,
                    kind: InputKind::Key {
                        code: 31,
                        pressed: true,
                    },
                },
            ]
            .into_iter(),
        )
        .unwrap();
    runtime
        .device_mut(pointer)
        .unwrap()
        .native
        .synchronize(&source, 0, 0);
    runtime
        .get_state(&mut memory, pointer, 8, DATA + 512)
        .unwrap();
    let mut state = vec![0; 8];
    memory.read(GuestAddress(DATA + 512), &mut state).unwrap();
    memory
        .write(GuestAddress(DATA + 520), &2u32.to_le_bytes())
        .unwrap();
    runtime
        .get_data(
            &mut memory,
            pointer,
            &[pointer.0, 20, DATA + 768, DATA + 520, 0],
            InterfaceKind32::Device8A,
        )
        .unwrap();
    let [count] = words::<1>(&memory, DATA + 520).unwrap();
    assert_eq!(count, 2);
    (state, words(&memory, DATA + 768).unwrap())
}

#[test]
fn successful_output_observation_matches_guest_state_and_buffered_bytes() {
    let observer = Observer::new(Config {
        level: Level::Basic,
        ..Config::default()
    })
    .unwrap();
    let (state, event) = read_outputs(&observer);
    let trace = observer.drain();
    let state_fields = annotations(&trace, "dinput.keyboard.state");
    let indices = state
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte != 0).then_some(index as u64))
        .collect::<Vec<_>>();
    assert_eq!(state_fields["nonzero_count"], indices.len() as u64);
    assert_eq!(state_fields["recorded_indices"], 2);
    assert_eq!(
        [state_fields["key_0"], state_fields["key_1"]],
        indices.as_slice()
    );
    assert_eq!(indices, [3, 6]);
    let data_fields = annotations(&trace, "dinput.device.data");
    assert_eq!(data_fields["returned_count"], 2);
    assert_eq!(data_fields["recorded_events"], 1);
    for (field, value) in [
        "first_offset",
        "first_value",
        "first_timestamp_ms",
        "first_sequence",
        "first_app_data",
    ]
    .into_iter()
    .zip(event)
    {
        assert_eq!(data_fields[field], u64::from(value));
    }
    assert_eq!(event[..4], [3, 0x80, 2, 2]);
    assert_eq!(observer.stats().dropped_events, 0);
}

#[test]
fn disabled_or_filtered_output_observation_emits_no_events() {
    for observer in [
        Observer::disabled(),
        Observer::new(Config {
            level: Level::Basic,
            categories: CategorySelection::Only(vec![categories::ENGINE_CPU]),
            ..Config::default()
        })
        .unwrap(),
    ] {
        let before = observer.stats().emitted_events;
        let (state, event) = read_outputs(&observer);
        assert_eq!((state[3], event[1]), (0x80, 0x80));
        assert_eq!(observer.stats().emitted_events, before);
    }
}

#[test]
fn disabled_output_context_does_not_leak_into_an_outer_capture() {
    let outer = Observer::new(Config {
        level: Level::Basic,
        ..Config::default()
    })
    .unwrap();
    let _capture = outer.enter();
    let before = outer.stats().emitted_events;
    read_outputs(&Observer::disabled());
    assert_eq!(outer.stats().emitted_events, before);
}

#[test]
fn empty_or_null_data_output_does_not_record_a_stale_first_event() {
    let observer = Observer::new(Config {
        level: Level::Basic,
        ..Config::default()
    })
    .unwrap();
    let _capture = observer.enter();
    let (mut runtime, mut memory, pointer, mut source) = runtime(true);
    keyboard_format(&mut runtime, &mut memory, pointer);
    runtime
        .device_mut(pointer)
        .unwrap()
        .native
        .acquire(&source, 0)
        .unwrap();
    memory
        .write(GuestAddress(DATA + 520), &2u32.to_le_bytes())
        .unwrap();
    runtime
        .get_data(
            &mut memory,
            pointer,
            &[pointer.0, 20, DATA + 768, DATA + 520, 0],
            InterfaceKind32::Device8A,
        )
        .unwrap();
    let empty = annotations(&observer.drain(), "dinput.device.data");
    assert_eq!(words::<1>(&memory, DATA + 520).unwrap(), [0]);
    assert_eq!(empty["returned_count"], 0);
    assert_eq!(empty["recorded_events"], 0);
    assert!(!empty.contains_key("first_offset"));

    source
        .publish_physical(
            [TimedInputEvent {
                timestamp_ns: 2_000_000,
                kind: InputKind::Key {
                    code: 30,
                    pressed: true,
                },
            }]
            .into_iter(),
        )
        .unwrap();
    runtime
        .device_mut(pointer)
        .unwrap()
        .native
        .synchronize(&source, 0, 0);
    runtime
        .get_state(&mut memory, pointer, 8, DATA + 512)
        .unwrap();
    memory
        .write(GuestAddress(DATA + 520), &2u32.to_le_bytes())
        .unwrap();
    runtime
        .get_data(
            &mut memory,
            pointer,
            &[pointer.0, 20, 0, DATA + 520, 0],
            InterfaceKind32::Device8A,
        )
        .unwrap();
    let null = annotations(&observer.drain(), "dinput.device.data");
    assert_eq!(words::<1>(&memory, DATA + 520).unwrap(), [1]);
    assert_eq!(null["returned_count"], 1);
    assert_eq!(null["recorded_events"], 0);
    assert!(!null.contains_key("first_offset"));
}

// decode only standard protobuf fields needed to inspect these unsigned annotations.
fn varint(input: &mut &[u8]) -> u64 {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let byte = input[0];
        *input = &input[1..];
        value |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return value;
        }
    }
    panic!("invalid protobuf varint")
}

enum Wire<'a> {
    Number(u64),
    Bytes(&'a [u8]),
    Fixed,
}
fn fields(mut input: &[u8]) -> Vec<(u64, Wire<'_>)> {
    let mut result = Vec::new();
    while !input.is_empty() {
        let key = varint(&mut input);
        let value = match key & 7 {
            0 => Wire::Number(varint(&mut input)),
            2 => {
                let length = varint(&mut input) as usize;
                let (bytes, rest) = input.split_at(length);
                input = rest;
                Wire::Bytes(bytes)
            }
            1 => {
                input = &input[8..];
                Wire::Fixed
            }
            5 => {
                input = &input[4..];
                Wire::Fixed
            }
            _ => panic!("unsupported protobuf wire type"),
        };
        result.push((key >> 3, value));
    }
    result
}

fn annotations(trace: &[u8], name: &str) -> BTreeMap<String, u64> {
    for (_, packet) in fields(trace) {
        let Wire::Bytes(packet) = packet else {
            continue;
        };
        for (field, event) in fields(packet) {
            let Wire::Bytes(event) = event else { continue };
            if field != 11 {
                continue;
            }
            let event = fields(event);
            if !event.iter().any(|(field, value)| {
                *field == 23 && matches!(value, Wire::Bytes(bytes) if *bytes == name.as_bytes())
            }) {
                continue;
            }
            let mut result = BTreeMap::new();
            for (field, annotation) in event {
                let Wire::Bytes(annotation) = annotation else {
                    continue;
                };
                if field != 4 {
                    continue;
                }
                let mut name = None;
                let mut value = None;
                for (field, wire) in fields(annotation) {
                    match (field, wire) {
                        (10, Wire::Bytes(bytes)) => {
                            name = Some(String::from_utf8(bytes.to_vec()).unwrap())
                        }
                        (3, Wire::Number(number)) => value = Some(number),
                        _ => {}
                    }
                }
                if let (Some(name), Some(value)) = (name, value) {
                    result.insert(name, value);
                }
            }
            return result;
        }
    }
    panic!("trace event {name} absent")
}
