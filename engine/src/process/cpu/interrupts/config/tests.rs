use super::*;
use crate::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{encode_exit_v3, encode_interrupt_descriptor_table, encode_state},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{
            DescriptorTableRegister32, SegmentDescriptorRecord, State32, UserSystemRegisterBank32,
        },
    },
    memory::GuestAddress,
    windows::CallingConvention32,
};

const KEY: u64 = 0x4944_5441_5554_4831;
const PAGES: [u32; 4] = [0x1000, 0x2000, 0x3000, 0x8000];
fn table(limit: u16) -> InterruptDescriptorTable32 {
    InterruptDescriptorTable32::new(
        limit,
        Box::new(std::array::from_fn(|vector| {
            SegmentDescriptorRecord::from_words(0xdead_0000 | vector as u32, 0xffff_ffff)
        })),
    )
}
fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
    }
    engine
}
fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}
fn specs(engine: &mut EngineInstance, words: &[u32]) {
    for (index, value) in words.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
}
fn state(engine: &EngineInstance) -> (Vec<u8>, [Vec<u8>; 4], u64, u32) {
    let memory = engine.memory().unwrap();
    let ram = PAGES.map(|page| {
        let mut bytes = vec![0; 4096];
        memory.read(GuestAddress(page), &mut bytes).unwrap();
        bytes
    });
    (
        engine.arena().to_vec(),
        ram,
        memory.code_validation_version(),
        engine.generation(),
    )
}

#[test]
fn configuration_moves_the_box_once_and_is_independent_of_query_readback() {
    for bank_first in [false, true] {
        let mut engine = engine();
        let bank = UserSystemRegisterBank32 {
            idtr: Some(DescriptorTableRegister32 {
                base: u32::MAX,
                limit: 7,
            }),
            ..Default::default()
        };
        if bank_first {
            engine.configure_system_register_bank(bank).unwrap();
        }
        let before = state(&engine);
        assert!(engine.interrupt_descriptors.is_none());
        let authority = table(0);
        let allocation = authority.record(0) as *const _;
        engine
            .configure_interrupt_descriptor_table(authority)
            .unwrap();
        let installed = engine.interrupt_descriptors.as_ref().unwrap();
        assert_eq!(installed.limit(), 0);
        assert_eq!(installed.record(255).to_words(), (0xdead_00ff, 0xffff_ffff));
        assert_eq!(installed.record(0) as *const _, allocation);
        assert_eq!(state(&engine), before);
        if !bank_first {
            engine.configure_system_register_bank(bank).unwrap();
        }
        assert_eq!(engine.system_register_bank, Some(bank));
        let before = state(&engine);
        assert_eq!(
            engine.configure_interrupt_descriptor_table(table(u16::MAX)),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(state(&engine), before);
        assert_eq!(engine.interrupt_descriptors.as_ref().unwrap().limit(), 0);
        engine.close();
        assert!(engine.interrupt_descriptors.is_none());
        assert_eq!(
            engine.configure_interrupt_descriptor_table(table(7)),
            Err(HostError::Closed)
        );
    }
}

#[test]
fn published_material_or_started_image_freezes_configuration_without_altering_old_material() {
    for resident in [false, true] {
        let mut engine = engine();
        upload(&mut engine, 0x1000, &[0xeb, 0xfe]);
        specs(&mut engine, &[0x1000, 2]);
        let id = if resident {
            Some(engine.compile_resident(1).unwrap())
        } else {
            engine.compile(1).unwrap();
            None
        };
        let bytes = id.map_or_else(
            || engine.artifact_bytes().unwrap().to_vec(),
            |id| engine.resident_bytes(id.get()).unwrap().to_vec(),
        );
        let before = state(&engine);
        assert_eq!(
            engine.configure_interrupt_descriptor_table(table(0xffff)),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(
            engine.configure_interrupt_descriptor_table_request(),
            Err(HostError::InvalidRequest)
        );
        assert!(engine.interrupt_descriptors.is_none());
        assert_eq!(state(&engine), before);
        let after = id.map_or_else(
            || engine.artifact_bytes().unwrap(),
            |id| engine.resident_bytes(id.get()).unwrap(),
        );
        assert_eq!(after, bytes);
    }
    let mut engine = engine();
    engine.image_started = true;
    let before = state(&engine);
    assert_eq!(
        engine.configure_interrupt_descriptor_table(table(0)),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(state(&engine), before);
    assert!(engine.interrupt_descriptors.is_none());
}

#[test]
fn strict_transfer_failure_does_not_publish_and_a_corrected_request_moves_complete_authority() {
    let mut engine = engine();
    let mut bytes = [0; INTERRUPT_DESCRIPTOR_TABLE_SIZE];
    encode_interrupt_descriptor_table(&table(0xffff), &mut bytes).unwrap();
    bytes[18] = 1;
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    let before = state(&engine);
    assert_eq!(
        engine.configure_interrupt_descriptor_table_request(),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(state(&engine), before);
    assert!(engine.interrupt_descriptors.is_none());
    bytes[18] = 0;
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    let before = state(&engine);
    engine
        .configure_interrupt_descriptor_table_request()
        .unwrap();
    assert_eq!(state(&engine), before);
    assert_eq!(
        engine.interrupt_descriptors.as_ref().unwrap().limit(),
        0xffff
    );
    for vector in 0..=u8::MAX {
        assert_eq!(
            engine
                .interrupt_descriptors
                .as_ref()
                .unwrap()
                .record(vector)
                .to_words(),
            (0xdead_0000 | u32::from(vector), 0xffff_ffff)
        );
    }
}

#[test]
fn real_pending_call_and_callback_busy_precede_frozen_or_malformed_configuration() {
    let mut engine = engine();
    engine
        .configure_interrupt_descriptor_table(table(7))
        .unwrap();
    upload(&mut engine, 0x1000, &[0x0f, 0x0b]);
    upload(&mut engine, 0x1100, &[0x0f, 0x0b]);
    upload(&mut engine, 0x2000, &[0x90, 0xeb, 0xfe]);
    specs(
        &mut engine,
        &[0x1000, 2, 0x1100, 2, 0x2000, 3, 0x1000, 7, 0x1100, 8],
    );
    engine.compile_with_gates(3, 2).unwrap();
    engine.write32(0x8f00, 0x2000).unwrap();
    let arena = engine.arena_mut().unwrap();
    encode_state(
        &State32 {
            registers: [0, 0, 0, 0, 0x8f00, 0, 0, 0],
            eip: 0x1000,
            eflags: 2,
        },
        &mut arena[..56],
    )
    .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: 7 },
        },
        &mut arena[56..96],
    )
    .unwrap();
    let token = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap()
        .token;
    for callback in [false, true] {
        if callback {
            engine
                .begin_callback(KEY, 1, token, 0x2000, 0x1100, 8, &[])
                .unwrap();
        }
        engine.arena_mut().unwrap()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + INTERRUPT_DESCRIPTOR_TABLE_SIZE]
            .fill(0);
        let before = state(&engine);
        assert_eq!(
            engine.configure_interrupt_descriptor_table(table(0)),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.configure_interrupt_descriptor_table_request(),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(state(&engine), before);
        assert_eq!(engine.interrupt_descriptors.as_ref().unwrap().limit(), 7);
    }
}
