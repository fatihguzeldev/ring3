use super::*;
use crate::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{encode_exit_v3, encode_state_current},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{SegmentDescriptorRecord, State32, UserSystemRegisterBank32},
    },
    memory::GuestAddress,
    windows::CallingConvention32,
};

const KEY: u64 = 0x5441_534b_4346_4731;
const PAGES: [u32; 3] = [0x1000, 0x5000, 0x8000];

fn current() -> TaskRegister {
    TaskRegister::from_cached(
        0x43,
        SegmentDescriptorRecord::from_words(0x5000_0067, 0x8b00),
    )
    .unwrap()
}

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
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
        let offset = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    arena: Vec<u8>,
    ram: [Vec<u8>; 3],
    descriptors: crate::cpu::x86::SegmentDescriptorTable,
    version: u64,
    generation: u32,
}

fn snapshot(engine: &EngineInstance) -> Snapshot {
    let memory = engine.memory().unwrap();
    Snapshot {
        arena: engine.arena().to_vec(),
        ram: PAGES.map(|page| {
            let mut bytes = vec![0; 4096];
            memory.read(GuestAddress(page), &mut bytes).unwrap();
            bytes
        }),
        descriptors: engine.segment_descriptors.clone(),
        version: memory.code_validation_version(),
        generation: engine.generation,
    }
}

#[test]
fn explicit_cached_tr_and_extent_configure_once_without_inferring_query_or_table_images() {
    for bank_first in [false, true] {
        let mut engine = engine();
        let bank = UserSystemRegisterBank32 {
            tr: Some(0x28),
            ..Default::default()
        };
        if bank_first {
            engine.configure_system_register_bank(bank).unwrap();
        }
        let before = snapshot(&engine);
        // the cached current tr need not be inside the new live gdt extent.
        engine.configure_task_return_sources(current(), 0).unwrap();
        let sources = engine.task_return_sources.unwrap();
        assert_eq!((sources.current, sources.gdt_limit), (current(), 0));
        assert_eq!(snapshot(&engine), before);
        assert!(engine.segment_descriptors.raw_descriptor(0x40).is_none());
        if !bank_first {
            engine.configure_system_register_bank(bank).unwrap();
        }
        assert_eq!(engine.system_register_bank, Some(bank));
        let before = snapshot(&engine);
        assert_eq!(
            engine.configure_task_return_sources(current(), u16::MAX),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(snapshot(&engine), before);
        assert_eq!(engine.task_return_sources, Some(sources));
        engine.close();
        assert!(engine.task_return_sources.is_none());
        assert_eq!(
            engine.configure_task_return_sources(current(), 0x4f),
            Err(HostError::Closed)
        );
    }
}

#[test]
fn replacement_resident_entries_and_started_images_freeze_sources_before_first_install() {
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = engine();
            upload(&mut engine, 0x1000, &[0xeb, 0xfe]);
            specs(&mut engine, if entries { &[0x1000] } else { &[0x1000, 2] });
            let id = match (resident, entries) {
                (false, false) => {
                    engine.compile(1).unwrap();
                    None
                }
                (false, true) => {
                    engine.compile_entries(1, 0).unwrap();
                    None
                }
                (true, false) => Some(engine.compile_resident(1).unwrap().get()),
                (true, true) => Some(engine.compile_resident_entries(1, 0).unwrap().get()),
            };
            let material = id.map_or_else(
                || engine.artifact_bytes().unwrap().to_vec(),
                |id| engine.resident_bytes(id).unwrap().to_vec(),
            );
            let before = snapshot(&engine);
            assert_eq!(
                engine.configure_task_return_sources(current(), 0x4f),
                Err(HostError::InvalidRequest)
            );
            assert!(engine.task_return_sources.is_none());
            assert_eq!(snapshot(&engine), before);
            assert_eq!(
                id.map_or_else(
                    || engine.artifact_bytes().unwrap(),
                    |id| engine.resident_bytes(id).unwrap(),
                ),
                material
            );
        }
    }
    let mut engine = engine();
    engine.image_started = true;
    let before = snapshot(&engine);
    assert_eq!(
        engine.configure_task_return_sources(current(), 0x4f),
        Err(HostError::InvalidRequest)
    );
    assert!(engine.task_return_sources.is_none());
    assert_eq!(snapshot(&engine), before);
}

#[test]
fn real_pending_call_and_callback_busy_precede_configuration_and_task_preparation() {
    let mut engine = engine();
    engine
        .configure_task_return_sources(current(), 0x4f)
        .unwrap();
    upload(&mut engine, 0x1000, &[0x0f, 0x0b]);
    upload(&mut engine, 0x1100, &[0x0f, 0x0b]);
    upload(&mut engine, 0x1200, &[0x90, 0xeb, 0xfe]);
    specs(
        &mut engine,
        &[0x1000, 2, 0x1100, 2, 0x1200, 3, 0x1000, 7, 0x1100, 8],
    );
    engine.compile_with_gates(3, 2).unwrap();
    engine.write32(0x8f00, 0x1200).unwrap();
    let arena = engine.arena_mut().unwrap();
    encode_state_current(
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
                .begin_callback(KEY, 1, token, 0x1200, 0x1100, 8, &[])
                .unwrap();
        }
        let before = snapshot(&engine);
        assert_eq!(
            engine.configure_task_return_sources(current(), 0xffff),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.prepare_task_return(),
            if callback {
                Ok(crate::process::TaskReturnAdmission::InvalidTss(0))
            } else {
                Err(HostError::Call(CallError::Busy))
            }
        );
        assert_eq!(snapshot(&engine), before);
    }
}
