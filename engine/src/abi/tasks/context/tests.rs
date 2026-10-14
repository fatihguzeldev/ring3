use super::*;
use crate::cpu::x86::State32;

fn pending() -> TaskIncomingContext32 {
    let mut segments = SegmentContext {
        state: SegmentState {
            selectors: [0x23, 0x53, 0x23, 0x23, 0x3b, 0],
            bases: [0; 6],
        },
        caches: [SegmentCache::UNUSABLE; 6],
    };
    segments.caches[2] = SegmentCache::from_bits(0xf33fffff).unwrap();
    TaskIncomingContext32::from_image(TaskContextImage32 {
        state: State32 {
            registers: [0x11223344; 8],
            eip: 0x12345678,
            eflags: 0x250fd7,
        },
        segments,
        loaded: 4,
        ldtr: 0,
        ldt_cache: None,
        task_trap: true,
        fault: Some(TaskTransitionFault32::NotPresent(0x50)),
    })
    .unwrap()
}
fn literal() -> [u8; 184] {
    let mut bytes = [0; 184];
    bytes[..16].copy_from_slice(b"R3TC\x01\0\x01\0\xb8\0\0\0\0\0\0\0");
    bytes[16..32].copy_from_slice(b"R3ST\x03\0\x01\0\x38\0\0\0\0\0\0\0");
    for offset in (32..64).step_by(4) {
        bytes[offset..offset + 4].copy_from_slice(&0x11223344_u32.to_le_bytes());
    }
    for (offset, value) in [
        (64, 0x12345678_u32),
        (68, 0x250fd7),
        (72, 4),
        (76, 0x5f),
        (80, 2),
        (84, 0x50),
        (88, 0x3f03),
        (96, 0x23),
        (100, 0x53),
        (104, 0x23),
        (108, 0x23),
        (112, 0x3b),
        (152, 0xf33fffff),
        (176, 2),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
#[test]
fn literal_pending_image_preserves_raw_invalid_cs_known_masks_and_qualified_ss() {
    let record = TaskContextRecord32::Loaded {
        revision: 2,
        context: pending(),
        ss_debug_shadow: false,
    };
    let mut output = [0xa5; 184];
    encode_task_context(record, &mut output).unwrap();
    assert_eq!(output, literal());
    assert_eq!(decode_task_context(&literal()), Ok(record));
    assert!(pending().validated_segments().is_none());
    let shadow = TaskContextRecord32::Loaded {
        revision: 2,
        context: pending(),
        ss_debug_shadow: true,
    };
    encode_task_context(shadow, &mut output).unwrap();
    assert_eq!(decode_task_context(&output), Ok(shadow));
    assert_eq!(&output[28..32], &1_u32.to_le_bytes());
}
#[test]
fn absent_and_loading_have_no_claimed_incoming_state_and_reject_noncanonical_payloads_atomically() {
    let mut absent = [0; 184];
    absent[..16].copy_from_slice(b"R3TC\x01\0\x01\0\xb8\0\0\0\0\0\0\0");
    let mut output = [0xa5; 184];
    encode_task_context(TaskContextRecord32::Absent, &mut output).unwrap();
    assert_eq!(output, absent);
    assert_eq!(
        decode_task_context(&absent),
        Ok(TaskContextRecord32::Absent)
    );
    absent[176..184].copy_from_slice(&7_u64.to_le_bytes());
    encode_task_context(TaskContextRecord32::Loading { revision: 7 }, &mut output).unwrap();
    assert_eq!(output, absent);
    assert_eq!(
        decode_task_context(&absent),
        Ok(TaskContextRecord32::Loading { revision: 7 })
    );
    assert!(output[16..176].iter().all(|&byte| byte == 0));
    let before = output;
    assert_eq!(
        encode_task_context(TaskContextRecord32::Loading { revision: 0 }, &mut output),
        Err(AbiError::TaskContext)
    );
    assert_eq!(output, before);
    for (offset, value) in [
        (12, 1_u32),
        (72, 0x40),
        (76, 0x9f),
        (80, 5),
        (84, 0x51),
        (88, 0x3e03),
        (96, 0x10023),
        (120, 1),
        (148, 0xf3bfffff),
        (168, 1),
        (180, 0),
    ] {
        let mut bytes = literal();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        if offset == 180 {
            bytes[176..184].fill(0);
        }
        assert!(decode_task_context(&bytes).is_err(), "offset{offset}");
    }
}
