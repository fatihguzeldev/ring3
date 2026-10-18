use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Value {
    Number(u64),
    Text(String),
}

impl Value {
    pub(super) fn number(&self) -> u64 {
        match self {
            Self::Number(value) => *value,
            _ => panic!("numeric annotation"),
        }
    }
}

enum Wire<'a> {
    Number(u64),
    Bytes(&'a [u8]),
    Fixed,
}

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
            _ => panic!("unexpected protobuf wire type"),
        };
        result.push((key >> 3, value));
    }
    result
}

// inspect the standard TrackEvent fields emitted by this narrow diagnostic slice.
pub(super) fn events(trace: &[u8], expected_name: &str) -> Vec<BTreeMap<String, Value>> {
    let mut result = Vec::new();
    for (_, packet) in fields(trace) {
        let Wire::Bytes(packet) = packet else {
            continue;
        };
        for (tag, event) in fields(packet) {
            let Wire::Bytes(event) = event else { continue };
            if tag != 11 {
                continue;
            }
            let event = fields(event);
            if !event.iter().any(|(tag, value)| {
                *tag == 23
                    && matches!(value, Wire::Bytes(bytes) if *bytes == expected_name.as_bytes())
            }) {
                continue;
            }
            let mut annotations = BTreeMap::new();
            for (tag, annotation) in event {
                let Wire::Bytes(annotation) = annotation else {
                    continue;
                };
                if tag != 4 {
                    continue;
                }
                let (mut name, mut value) = (None, None);
                for (tag, field) in fields(annotation) {
                    match (tag, field) {
                        (10, Wire::Bytes(bytes)) => {
                            name = Some(String::from_utf8(bytes.to_vec()).unwrap())
                        }
                        (2 | 3, Wire::Number(number)) => value = Some(Value::Number(number)),
                        (6, Wire::Bytes(bytes)) => {
                            value = Some(Value::Text(String::from_utf8(bytes.to_vec()).unwrap()))
                        }
                        _ => {}
                    }
                }
                if let (Some(name), Some(value)) = (name, value) {
                    annotations.insert(name, value);
                }
            }
            result.push(annotations);
        }
    }
    result
}
