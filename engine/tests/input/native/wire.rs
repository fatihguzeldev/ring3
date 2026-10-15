use super::fixtures::{CLOCK_EPOCH, EPOCH, bytes, queued};
use ring3_engine::input::{
    InputContext, InputError, InputKind, InputLimits, InputOrigin, MAX_BATCH_EVENTS,
    wire::inspect_batch,
};

fn focus_batch() -> Vec<u8> {
    bytes(
        "52 33 49 42 01 00 20 00 07 00 00 00 00 00 00 00
           01 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00
           64 00 00 00 00 00 00 00 04 00 00 00 01 00 00 00
           00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00",
    )
}

#[test]
fn literal_batch_and_queue_records_have_exact_versioned_bytes() {
    let input = focus_batch();
    let batch = inspect_batch(&input).unwrap();
    assert_eq!(batch.session_epoch(), EPOCH);
    assert_eq!(batch.sequence(), 1);
    assert_eq!(batch.event_count(), 1);
    assert_eq!(batch.latest_timestamp_ns(), CLOCK_EPOCH);
    let mut context = InputContext::new(EPOCH, CLOCK_EPOCH, InputLimits::default()).unwrap();
    context.submit_encoded(&batch).unwrap();
    let records = queued(&context);
    assert_eq!(records[0].event.kind, InputKind::Focus { focused: true });
    assert_eq!(records[0].origin, InputOrigin::Host);
    let expected = bytes(
        "52 33 49 51 01 00 30 00 07 00 00 00 00 00 00 00
                          01 00 00 00 00 00 00 00 50 00 00 00 00 00 00 00
                          64 00 00 00 00 00 00 00 04 00 00 00 01 00 00 00
                          00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
                          01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00",
    );
    let mut output = [0xa5; 83];
    let read = context.peek_encoded(1, &mut output).unwrap();
    assert_eq!((read.count, read.bytes), (1, 80));
    assert_eq!(&output[..80], expected);
    assert_eq!(&output[80..], &[0xa5; 3]);
    assert_eq!(context.queued_events(), 1);
    assert_eq!(context.read_encoded(1, &mut output).unwrap(), read);
    assert_eq!(&output[..80], expected);
    assert_eq!(context.queued_events(), 0);
}

#[test]
fn malformed_wire_and_short_output_do_not_publish_or_consume() {
    let original = focus_batch();
    for (offset, value) in [
        (0, 0),
        (4, 2),
        (6, 31),
        (24, 2),
        (28, 1),
        (40, 6),
        (44, 2),
        (48, 1),
        (56, 1),
    ] {
        let mut malformed = original.clone();
        malformed[offset] = value;
        assert_eq!(
            inspect_batch(&malformed).err(),
            Some(InputError::InvalidWire)
        );
    }
    for length in [0, 31, 32, 63] {
        assert_eq!(
            inspect_batch(&original[..length]).err(),
            Some(InputError::InvalidWire)
        );
    }
    let mut trailing = original.clone();
    trailing.push(0);
    assert_eq!(
        inspect_batch(&trailing).err(),
        Some(InputError::InvalidWire)
    );
    for count in [MAX_BATCH_EVENTS as u32 + 1, u32::MAX] {
        let mut header = original[..32].to_vec();
        header[24..28].copy_from_slice(&count.to_le_bytes());
        assert_eq!(inspect_batch(&header).err(), Some(InputError::InvalidWire));
    }
    let mut context = InputContext::new(EPOCH, CLOCK_EPOCH, InputLimits::default()).unwrap();
    context
        .submit_encoded(&inspect_batch(&original).unwrap())
        .unwrap();
    let before = queued(&context);
    let mut output = [0xa5; 79];
    assert_eq!(
        context.read_encoded(1, &mut output),
        Err(InputError::OutputCapacity)
    );
    assert_eq!(output, [0xa5; 79]);
    assert_eq!(queued(&context), before);
}
