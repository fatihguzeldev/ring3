use super::*;
use crate::{
    abi::arena::{EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET},
    cpu::x86::{Register32, SegmentDescriptorKind, State32},
    process::{
        StoreCompletion,
        calls::{
            call::{PendingCall, PendingOwner},
            callback::{SuspendedCallback, SuspendedRecord},
        },
        cpu::context::ExtendedCpuState,
    },
    windows::{CallFrame32, CallingConvention32},
};

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(3, 7).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x2000, 1, 3).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    engine
}

fn raw(low: u32, high: u32) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(low, high)
}

fn word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn packet(bytes: &mut [u8], status: u32, value: u32, detail: u32) {
    let output = &mut bytes[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE];
    output.fill(0);
    output[..4].copy_from_slice(b"R3MH");
    word(output, 4, 0x10007);
    word(output, 8, 40);
    word(output, 16, status);
    word(output, 20, value);
    word(output, 24, detail);
}

#[test]
fn raw_configuration_preserves_payload_and_every_loaded_arena_byte() {
    let mut engine = engine();
    let before = engine.arena().to_vec();
    let context = engine.segment_context().unwrap();
    for kind in 0..16 {
        let record = raw(0x1234_5678, 0x9abc_e0de | (kind << 8));
        engine
            .configure_segment_descriptor_raw(0x43, record)
            .unwrap();
        assert_eq!(
            engine.segment_descriptors.raw_descriptor(0x40),
            Some(&record)
        );
        assert_eq!(engine.segment_descriptors.descriptor(0x43), None);
        assert_eq!(engine.segment_context().unwrap(), context);
        assert_eq!(engine.arena(), before);
    }
    for selector in 0..4 {
        let table = engine.segment_descriptors.clone();
        assert_eq!(
            engine.configure_segment_descriptor_raw(selector, raw(u32::MAX, u32::MAX)),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.segment_descriptors, table);
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn successful_raw_load_publishes_exact_cache_and_then_sets_table_accessed() {
    let mut engine = engine();
    engine
        .configure_segment_descriptor_raw(0x43, raw(0x2000_1234, 0x0050_f200))
        .unwrap();
    let mut expected = engine.arena().to_vec();
    engine.segment_load(4, 0x43).unwrap();
    packet(&mut expected, 0, 0x2000, 0);
    let segment = &mut expected[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE];
    segment.fill(0);
    segment[..4].copy_from_slice(b"R3SG");
    word(segment, 4, 0x10002);
    word(segment, 8, 64);
    for (index, selector) in [0x23_u16, 0x1b, 0x23, 0x23, 0x43, 0]
        .into_iter()
        .enumerate()
    {
        segment[16 + index * 2..18 + index * 2].copy_from_slice(&selector.to_le_bytes());
    }
    for (offset, bits) in [
        (28, 0xf33f_ffff),
        (32, 0xf3bf_ffff),
        (36, 0xf33f_ffff),
        (40, 0xf33f_ffff),
        (44, 0xfa30_1234),
        (56, 0),
    ] {
        word(segment, offset, bits);
    }
    word(segment, 48, 0x2000);
    assert_eq!(engine.arena(), expected);
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x43)
            .unwrap()
            .to_words(),
        (0x2000_1234, 0x0050_f300)
    );
    let loaded = engine.segment_context().unwrap();
    let before = engine.arena().to_vec();
    engine
        .configure_segment_descriptor_raw(0x43, raw(0x3000_1234, 0x00c0_f000))
        .unwrap();
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.segment_context().unwrap(), loaded);
    engine.segment_load(4, 0x43).unwrap();
    let reloaded = engine.segment_context().unwrap();
    assert_eq!(reloaded.state.bases[4], 0x3000);
    assert_eq!(reloaded.caches[4].bits(), 0xf310_1234);
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x43)
            .unwrap()
            .to_words(),
        (0x3000_1234, 0x00c0_f100)
    );
}

#[test]
fn raw_verification_uses_current_attributes_without_accessed_or_cache_mutation() {
    let mut engine = engine();
    engine
        .configure_segment_descriptor_raw(0x43, raw(0x2000_0042, 0x0040_7200))
        .unwrap();
    let loaded = engine.segment_context().unwrap();
    for query in [0x30001, 0x30002] {
        for (index, value) in [
            0x43_u32,
            0x24_4e97,
            0xaabb_ccdd,
            0x1122_3344,
            0x5566_7788,
            0x99aa_bbcc,
        ]
        .into_iter()
        .enumerate()
        {
            word(
                engine.arena_mut().unwrap(),
                TRANSFER_OFFSET + index * 4,
                value,
            );
        }
        let mut expected = engine.arena().to_vec();
        word(&mut expected, TRANSFER_OFFSET + 4, 0x24_4ed7);
        packet(&mut expected, 0, 0, 0);
        let table = engine.segment_descriptors.clone();
        assert_eq!(engine.cpu_integer(query, 0), Ok(StoreCompletion::Complete));
        assert_eq!(engine.arena(), expected);
        assert_eq!(engine.segment_descriptors, table);
        assert_eq!(engine.segment_context().unwrap(), loaded);
    }
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x43)
            .unwrap()
            .descriptor_type(),
        2
    );
}

#[test]
fn architectural_load_failures_publish_only_the_exact_packet_without_setting_accessed() {
    let mut engine = engine();
    for (target, high, status) in [
        (4, 0x0040_7200, 6),
        (2, 0x0040_7200, 7),
        (4, 0x0040_e000, 3),
        (4, 0x0040_d200, 3),
        (2, 0x0040_f000, 3),
    ] {
        engine
            .configure_segment_descriptor_raw(0x43, raw(0xffff, high))
            .unwrap();
        let table = engine.segment_descriptors.clone();
        let mut expected = engine.arena().to_vec();
        packet(&mut expected, status, 0, 0x40);
        engine.segment_load(target, 0x43).unwrap();
        assert_eq!(engine.arena(), expected);
        assert_eq!(engine.segment_descriptors, table);
    }
}

#[test]
fn unsupported_raw_profiles_are_neutral_host_refusals_before_any_publication() {
    let mut engine = engine();
    {
        let (target, low, high) = (4, 0xffff, 0x0060_f200);
        engine
            .configure_segment_descriptor_raw(0x43, raw(low, high))
            .unwrap();
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        assert_eq!(
            engine.segment_load(target, 0x43),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.segment_descriptors, table);
    }
}

#[test]
fn host_base_updates_preserve_raw_limits_rights_and_loaded_cache() {
    let mut engine = engine();
    engine
        .configure_segment_descriptor_raw(0x43, raw(0x2000_1234, 0x0050_f600))
        .unwrap();
    engine.segment_load(4, 0x43).unwrap();
    let cache = engine.segment_context().unwrap().caches[4];
    let mut expected = engine.arena().to_vec();
    word(&mut expected, SEGMENTS_OFFSET + 48, 0x1122_3344);
    engine
        .set_segment_base(SegmentRegister::Fs, 0x1122_3344)
        .unwrap();
    assert_eq!(engine.arena(), expected);
    assert_eq!(engine.segment_context().unwrap().caches[4], cache);
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x43)
            .unwrap()
            .to_words(),
        (0x3344_1234, 0x1150_f722)
    );
    // replacing the table with a raw gate does not turn its payload into a segment base.
    engine
        .configure_segment_descriptor_raw(0x43, raw(0xdead_beef, 0x89ab_ec76))
        .unwrap();
    let before = engine.arena().to_vec();
    let table = engine.segment_descriptors.clone();
    assert_eq!(
        engine.set_segment_base(SegmentRegister::Fs, 0),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.segment_descriptors, table);
    // the old explicit system constructor retains its opaque compatibility base observation.
    engine
        .configure_segment_descriptor(SegmentDescriptor {
            selector: 0x43,
            base: 0x55,
            dpl: 3,
            present: true,
            kind: SegmentDescriptorKind::System,
        })
        .unwrap();
    engine.set_segment_base(SegmentRegister::Fs, 0x66).unwrap();
    assert_eq!(engine.segment_context().unwrap().caches[4], cache);
    assert_eq!(
        engine.segment_descriptors.descriptor(0x43).unwrap().base,
        0x66
    );
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x43)
            .unwrap()
            .to_words(),
        (0x0066_0000, 0x0000_e000)
    );
}

#[test]
fn raw_configuration_respects_pending_call_callback_and_closed_authority() {
    let mut engine = engine();
    let mut state = State32::default();
    state.registers[Register32::Esp.index()] = 0x2000;
    let frame = CallFrame32::capture(
        engine.memory().unwrap(),
        state,
        CallingConvention32::Cdecl,
        0,
    )
    .unwrap();
    engine.pending_call = Some(PendingCall {
        token: 1,
        owner: PendingOwner::Replacement(engine.generation()),
        frame,
        state: engine.arena()[STATE_OFFSET..STATE_OFFSET + 56]
            .try_into()
            .unwrap(),
        exit: engine.arena()[EXIT_OFFSET..EXIT_OFFSET + 40]
            .try_into()
            .unwrap(),
        extended: ExtendedCpuState::capture(engine.arena()).unwrap(),
    });
    let before = engine.arena().to_vec();
    let table = engine.segment_descriptors.clone();
    assert_eq!(
        engine.configure_segment_descriptor_raw(0x43, raw(0, 0)),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.segment_descriptors, table);
    let outer = engine.pending_call.take().unwrap();
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Replacement(crate::abi::callback::CallbackRecord32 {
            token: 2,
            outer_token: 1,
            phase: 1,
            outcome: 0,
            entry_pc: 0x1000,
            entry_esp: 0x2000,
            return_pc: 0x1000,
            return_id: 7,
            stack_words: 0,
            result: 0,
            generation: engine.generation(),
        }),
    });
    assert_eq!(
        engine.configure_segment_descriptor_raw(0x43, raw(0, 0)),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.segment_descriptors, table);
    engine.callback = None;
    engine.close();
    let before = engine.arena().to_vec();
    let table = engine.segment_descriptors.clone();
    assert_eq!(
        engine.configure_segment_descriptor_raw(0x43, raw(0, 0)),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.segment_descriptors, table);
}

#[test]
fn invalid_segment_context_never_sets_accessed_or_changes_any_arena_byte() {
    let mut engine = engine();
    engine
        .configure_segment_descriptor_raw(0x43, raw(0x2000_1234, 0x0050_f200))
        .unwrap();
    engine.arena_mut().unwrap()[SEGMENTS_OFFSET + 60] = 1;
    let before = engine.arena().to_vec();
    let table = engine.segment_descriptors.clone();
    assert_eq!(engine.segment_load(4, 0x43), Err(HostError::Infrastructure));
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.segment_descriptors, table);
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x43)
            .unwrap()
            .descriptor_type(),
        2
    );
}

#[test]
fn failed_far_pointer_read_preserves_raw_accessed_and_the_prior_loaded_context() {
    let mut engine = engine();
    engine
        .configure_segment_descriptor_raw(0x43, raw(0x2000_1234, 0x0050_f200))
        .unwrap();
    let before = engine.arena().to_vec();
    let loaded = engine.segment_context().unwrap();
    let table = engine.segment_descriptors.clone();
    engine.segment_load_far(4, 0x2ffc, 0).unwrap();
    assert_eq!(engine.segment_context().unwrap(), loaded);
    assert_eq!(engine.segment_descriptors, table);
    let mut expected = before;
    packet(&mut expected, 9, 0, 4);
    word(&mut expected, HELPER_OFFSET + 4, 0x1000f);
    word(&mut expected, HELPER_OFFSET + 28, 0x3000);
    word(&mut expected, HELPER_OFFSET + 32, 1);
    word(&mut expected, HELPER_OFFSET + 36, 6);
    assert_eq!(engine.arena(), expected);
    assert_eq!(engine.cr2(), Some(GuestAddress(0x3000)));
}

#[test]
fn nonflat_ss_load_and_long_mode_refusal_publish_atomically_with_the_table_accessed_bit() {
    let mut engine = engine();
    engine
        .configure_segment_descriptor_raw(0x43, raw(0x5000_003f, 0x0040_f200))
        .unwrap();
    engine.segment_load(2, 0x43).unwrap();
    let loaded = engine.segment_context().unwrap();
    assert_eq!(
        (
            loaded.state.selectors[2],
            loaded.state.bases[2],
            loaded.caches[2].bits()
        ),
        (0x43, 0x5000, 0xf230_003f)
    );
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x43)
            .unwrap()
            .descriptor_type(),
        3
    );
    engine
        .configure_segment_descriptor_raw(0x43, raw(0x6000_003f, 0x0060_f200))
        .unwrap();
    let before = engine.arena().to_vec();
    let table = engine.segment_descriptors.clone();
    assert_eq!(engine.segment_load(2, 0x43), Err(HostError::InvalidRequest));
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.segment_context().unwrap(), loaded);
    assert_eq!(engine.segment_descriptors, table);
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x43)
            .unwrap()
            .descriptor_type(),
        2
    );
}
