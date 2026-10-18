#[derive(Clone, Copy, Debug)]
pub enum Value<'a> {
    Unsigned(u64),
    Signed(i64),
    Float(f64),
    Bool(bool),
    String(&'a str),
}

#[derive(Clone, Copy, Debug)]
pub struct Field<'a> {
    pub name: &'a str,
    pub value: Value<'a>,
}

impl<'a> Field<'a> {
    pub const fn unsigned(name: &'a str, value: u64) -> Self {
        Self {
            name,
            value: Value::Unsigned(value),
        }
    }
    pub const fn signed(name: &'a str, value: i64) -> Self {
        Self {
            name,
            value: Value::Signed(value),
        }
    }
    pub const fn float(name: &'a str, value: f64) -> Self {
        Self {
            name,
            value: Value::Float(value),
        }
    }
    pub const fn boolean(name: &'a str, value: bool) -> Self {
        Self {
            name,
            value: Value::Bool(value),
        }
    }
    pub const fn string(name: &'a str, value: &'a str) -> Self {
        Self {
            name,
            value: Value::String(value),
        }
    }
}

fn varint(output: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        output.push(value as u8 | 128);
        value >>= 7;
    }
    output.push(value as u8);
}
fn number(output: &mut Vec<u8>, field: u32, value: u64) {
    varint(output, u64::from(field) << 3);
    varint(output, value);
}
fn bytes(output: &mut Vec<u8>, field: u32, value: &[u8]) {
    varint(output, (u64::from(field) << 3) | 2);
    varint(output, value.len() as u64);
    output.extend_from_slice(value);
}

fn packet(sequence: u32, timestamp: u64, kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(payload.len() + 30);
    number(&mut packet, 8, timestamp);
    number(&mut packet, 58, 1);
    number(&mut packet, 10, u64::from(sequence));
    number(&mut packet, 13, 1);
    bytes(&mut packet, kind, payload);
    let mut output = Vec::with_capacity(packet.len() + 8);
    bytes(&mut output, 1, &packet);
    output
}

pub(crate) fn descriptor(
    sequence: u32,
    timestamp: u64,
    track: u64,
    name: &str,
    parent: Option<u64>,
    counter: bool,
) -> Vec<u8> {
    let mut descriptor = Vec::new();
    number(&mut descriptor, 1, track);
    bytes(&mut descriptor, 2, name.as_bytes());
    if let Some(parent) = parent {
        number(&mut descriptor, 5, parent);
    }
    if counter {
        bytes(&mut descriptor, 8, &[]);
    }
    packet(sequence, timestamp, 60, &descriptor)
}

pub(crate) fn clock_snapshot(sequence: u32, timestamp: u64) -> Vec<u8> {
    let mut clock = Vec::new();
    number(&mut clock, 1, 1);
    number(&mut clock, 2, timestamp);
    let mut snapshot = Vec::new();
    bytes(&mut snapshot, 1, &clock);
    let mut file_clock = Vec::new();
    number(&mut file_clock, 1, 11);
    number(&mut file_clock, 2, timestamp);
    bytes(&mut snapshot, 1, &file_clock);
    number(&mut snapshot, 2, 11);
    let mut packet = Vec::new();
    number(&mut packet, 10, u64::from(sequence));
    bytes(&mut packet, 6, &snapshot);
    let mut output = Vec::new();
    bytes(&mut output, 1, &packet);
    output
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn event(
    sequence: u32,
    timestamp: u64,
    track: u64,
    kind: u64,
    context: &str,
    name: &str,
    fields: &[Field<'_>],
    counter: Option<f64>,
) -> Vec<u8> {
    let mut event = Vec::new();
    number(&mut event, 9, kind);
    number(&mut event, 11, track);
    if kind != 4 && kind != 2 {
        bytes(&mut event, 22, context.as_bytes());
        bytes(&mut event, 23, name.as_bytes());
    }
    if let Some(value) = counter {
        varint(&mut event, (44 << 3) | 1);
        event.extend_from_slice(&value.to_le_bytes());
    }
    for field in fields {
        let mut annotation = Vec::new();
        bytes(&mut annotation, 10, field.name.as_bytes());
        match field.value {
            Value::Unsigned(value) => number(&mut annotation, 3, value),
            Value::Signed(value) => number(&mut annotation, 4, value as u64),
            Value::Bool(value) => number(&mut annotation, 2, u64::from(value)),
            Value::String(value) => bytes(&mut annotation, 6, value.as_bytes()),
            Value::Float(value) => {
                varint(&mut annotation, (5 << 3) | 1);
                annotation.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes(&mut event, 4, &annotation);
    }
    packet(sequence, timestamp, 11, &event)
}
