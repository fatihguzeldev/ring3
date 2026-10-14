use super::*;
use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{
        DescriptorTableRegister32, SegmentDescriptorRecord,
        decode::{BoundDecode, DecodeAuthority, DecodePolicy, decode_with_authority},
        ir::{Operation, Value32, WordValue},
        tasks::TaskRegister,
    },
    memory::GuestAddress,
};

fn controls() -> TaskControlState32 {
    TaskControlState32::new(0x33, 0xdeadb123, 0, 0x400, None).unwrap()
}
fn instance() -> EngineInstance {
    let mut engine = EngineInstance::new(2, 0x5443).unwrap();
    for page in [0x1000, 0x5000] {
        engine.map(page, 1, 7).unwrap();
    }
    engine
}
fn sources(engine: &mut EngineInstance) {
    engine
        .configure_task_return_sources(
            TaskRegister::from_cached(
                0x43,
                SegmentDescriptorRecord::from_words(0x50000067, 0x8b00),
            )
            .unwrap(),
            0x57,
        )
        .unwrap();
}
#[test]
fn configuration_requires_task_sources_once_before_material_and_clears_on_close() {
    let mut engine = instance();
    assert_eq!(
        engine.configure_task_controls(controls()),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.task_control_state(), Ok(None));
    sources(&mut engine);
    let before = engine.arena().to_vec();
    assert_eq!(engine.configure_task_controls(controls()), Ok(()));
    assert_eq!(engine.task_control_state(), Ok(Some(controls())));
    assert_eq!(engine.task_execution_revision(), Some(1));
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine.configure_task_controls(controls()),
        Err(HostError::InvalidRequest)
    );
    engine.close();
    assert_eq!(engine.task_control_state(), Err(HostError::Closed));
    assert_eq!(engine.task_incoming_context(), Err(HostError::Closed));
    assert!(engine.task_controls.is_none());
    assert!(engine.task_incoming.is_none());
    assert_eq!(engine.task_execution_revision, 0);
    let mut engine = instance();
    sources(&mut engine);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    assert_eq!(
        engine.configure_task_controls(controls()),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.task_execution_revision(), None);
}

#[test]
fn live_task_queries_project_tr_ldtr_smsw_without_repurposing_immutable_readback_codec() {
    let mut engine = instance();
    let readback = UserSystemRegisterBank32 {
        gdtr: Some(DescriptorTableRegister32 {
            base: 0xdeadbeef,
            limit: 0x1234,
        }),
        tr: Some(0x70),
        ldtr: Some(0x68),
        cr0_low16: Some(0x31),
        ..Default::default()
    };
    engine.configure_system_register_bank(readback).unwrap();
    assert_eq!(engine.task_system_register_bank(), Some(readback));
    sources(&mut engine);
    engine.configure_task_controls(controls()).unwrap();
    let bank = engine.task_system_register_bank().unwrap();
    assert_eq!(
        (bank.tr, bank.ldtr, bank.cr0_low16, bank.gdtr),
        (Some(0x43), Some(0), Some(0x33), readback.gdtr)
    );
    // a real phase consumer updates live controls; the original configured query image stays exact.
    engine.task_controls.as_mut().unwrap().state = controls().after_return(0x50, 3);
    engine.task_execution_revision += 1;
    let bank = engine.task_system_register_bank().unwrap();
    assert_eq!(
        (bank.tr, bank.ldtr, bank.cr0_low16),
        (Some(0x43), Some(0x50), Some(0x3b))
    );
    assert_eq!(engine.system_register_bank, Some(readback));
    for (bytes, value, word) in [
        ([0x0f, 0x00, 0xc8], 0x43, false),
        ([0x0f, 0x00, 0xc0], 0x50, false),
        ([0x0f, 0x01, 0xe0], 0x3b, true),
    ] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .cpu_write(GuestAddress(0x1000), &bytes)
            .unwrap();
        let BoundDecode::Instruction(decoded) = decode_with_authority(
            engine.memory().unwrap(),
            GuestAddress(0x1000),
            DecodePolicy::Strict,
            Some(engine.code_fetch_context().unwrap()),
            DecodeAuthority::new(engine.task_system_register_bank(), false)
                .with_task_revision(engine.task_execution_revision()),
        )
        .unwrap() else {
            panic!("query instruction")
        };
        if word {
            assert!(
                matches!(decoded.operation(), Operation::MoveWord { source: WordValue::Immediate(got), .. } if *got == value)
            );
        } else {
            assert!(
                matches!(decoded.operation(), Operation::Move { source: Value32::Immediate(got), .. } if *got == u32::from(value))
            );
        }
        assert_eq!(decoded.segment_accesses(), 0);
    }
}

#[test]
fn wire_configuration_rejects_malformed_controls_without_publishing_and_reads_exact_live_values() {
    use crate::abi::tasks::{TASK_CONTROLS_SIZE, encode_task_controls};
    let mut engine = instance();
    sources(&mut engine);
    let mut record = [0; TASK_CONTROLS_SIZE];
    encode_task_controls(controls(), &mut record).unwrap();
    for (offset, value) in [(16, 0x71_u32), (24, 4), (28, 1), (32, 3), (36, 1)] {
        let mut malformed = record;
        malformed[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTROLS_SIZE]
            .copy_from_slice(&malformed);
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.configure_task_controls_request(),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.task_control_state(), Ok(None));
        assert_eq!(engine.task_execution_revision(), None);
    }
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTROLS_SIZE]
        .copy_from_slice(&record);
    engine.configure_task_controls_request().unwrap();
    assert_eq!(engine.task_control_state(), Ok(Some(controls())));
    assert_eq!(engine.task_execution_revision(), Some(1));
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTROLS_SIZE].fill(0xa5);
    let mut expected = engine.arena().to_vec();
    expected[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTROLS_SIZE].copy_from_slice(&record);
    engine.read_task_controls().unwrap();
    assert_eq!(engine.arena(), expected);
    assert_eq!(
        engine.configure_task_controls_request(),
        Err(HostError::InvalidRequest)
    );
}
