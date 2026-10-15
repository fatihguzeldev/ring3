use ring3_engine::input::{InputContext, InputKind, InputLimits, InputRecord, TimedInputEvent};

pub const EPOCH: u64 = 7;
pub const CLOCK_EPOCH: u64 = 100;

pub fn event(timestamp_ns: u64, kind: InputKind) -> TimedInputEvent {
    TimedInputEvent { timestamp_ns, kind }
}

pub fn focused(limits: InputLimits) -> InputContext {
    let mut context = InputContext::new(EPOCH, CLOCK_EPOCH, limits).unwrap();
    context
        .submit_batch(
            EPOCH,
            1,
            &[event(CLOCK_EPOCH, InputKind::Focus { focused: true })],
        )
        .unwrap();
    context.consume(1).unwrap();
    context
}

pub fn queued(context: &InputContext) -> Vec<InputRecord> {
    let mut records = vec![InputRecord::default(); context.queued_events()];
    assert_eq!(
        context.peek(usize::MAX, &mut records).unwrap(),
        records.len()
    );
    records
}

pub fn bytes(hex: &str) -> Vec<u8> {
    hex.split_whitespace()
        .map(|word| u8::from_str_radix(word, 16).unwrap())
        .collect()
}
