use super::*;
use crate::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET},
        x86::{encode_exit_v3, encode_state, encode_system_register_bank},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{DescriptorTableRegister32, SegmentDescriptorRecord, State32},
    },
    memory::GuestAddress,
    process::StoreCompletion,
    windows::CallingConvention32,
};

const KEY: u64 = 0x5359_5354_454d_3332;
fn bank() -> UserSystemRegisterBank32 {
    UserSystemRegisterBank32 {
        gdtr: Some(DescriptorTableRegister32 {
            base: 0x1234_5678,
            limit: 0x2345,
        }),
        ldtr: Some(0),
        cr0_low16: Some(0x33),
        ..Default::default()
    }
}
fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}
fn specs(engine: &mut EngineInstance, words: &[u32]) {
    for (i, value) in words.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + i * 4..TRANSFER_OFFSET + i * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
}
fn engine() -> EngineInstance {
    let mut e = EngineInstance::new(4, KEY).unwrap();
    for page in [0x1000, 0x2000, 0x3000, 0x8000] {
        e.map(page, 1, 7).unwrap();
    }
    e
}

#[test]
fn default_application_bank_admits_all_queries_without_system_memory_tables() {
    for bytes in [
        &[0x0f, 0x01, 0x00][..],
        &[0x0f, 0x01, 0x08],
        &[0x0f, 0x00, 0xc0],
        &[0x0f, 0x00, 0xc8],
        &[0x0f, 0x01, 0xe0],
    ] {
        for resident in [false, true] {
            let mut e = engine();
            assert_eq!(
                e.system_register_bank,
                Some(UserSystemRegisterBank32::application())
            );
            upload(&mut e, 0x1000, bytes);
            specs(&mut e, &[0x1000, bytes.len() as u32]);
            let before = e.arena().to_vec();
            if resident {
                e.compile_resident(1).unwrap();
            } else {
                e.compile(1).unwrap();
            }
            assert_eq!(&e.arena()[..TRANSFER_OFFSET], &before[..TRANSFER_OFFSET]);
            assert_eq!(e.memory().unwrap().mapped_pages(), 4);
            assert!(
                e.memory()
                    .unwrap()
                    .permissions_at(GuestAddress(0xffff0000))
                    .is_err()
            );
            assert!(
                e.memory()
                    .unwrap()
                    .permissions_at(GuestAddress(0xffff1000))
                    .is_err()
            );
        }
    }
}

#[test]
fn partial_readback_overrides_fill_defaults_and_can_change_before_publication() {
    let mut e = engine();
    e.configure_segment_descriptor_raw(7, SegmentDescriptorRecord::from_words(0xffff, 0x0040_f200))
        .unwrap();
    let images = UserSystemRegisterBank32 {
        gdtr: Some(DescriptorTableRegister32 {
            base: u32::MAX,
            limit: 0,
        }),
        ldtr: Some(0),
        ..Default::default()
    };
    e.configure_system_register_bank(images).unwrap();
    e.configure_segment_descriptor_raw(
        15,
        SegmentDescriptorRecord::from_words(0xffff, 0x0040_f200),
    )
    .unwrap();
    assert_eq!(
        e.system_register_bank,
        Some(images.with_application_defaults())
    );
    let before = e.arena().to_vec();
    e.configure_system_register_bank(bank()).unwrap();
    assert_eq!(
        e.system_register_bank,
        Some(bank().with_application_defaults())
    );
    assert_eq!(e.arena(), before);
    e.close();
    assert_eq!(e.system_register_bank, None);
}

#[test]
fn startup_configuration_freezes_for_both_material_owners_and_request_is_atomic() {
    for resident in [false, true] {
        let mut e = engine();
        upload(&mut e, 0x1000, &[0xeb, 0xfe]);
        specs(&mut e, &[0x1000, 2]);
        if resident {
            e.compile_resident(1).unwrap();
        } else {
            e.compile(1).unwrap();
        }
        let before = e.arena().to_vec();
        assert_eq!(
            e.configure_system_register_bank(bank()),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(
            e.system_register_bank,
            Some(UserSystemRegisterBank32::application())
        );
        assert_eq!(e.arena(), before);
    }
    let mut e = engine();
    let mut bytes = [0; SYSTEM_REGISTER_BANK_SIZE];
    encode_system_register_bank(&bank(), &mut bytes).unwrap();
    bytes[34] = 1;
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()].copy_from_slice(&bytes);
    let before = e.arena().to_vec();
    assert_eq!(
        e.configure_system_register_bank_request(),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(e.arena(), before);
    assert_eq!(
        e.system_register_bank,
        Some(UserSystemRegisterBank32::application())
    );
    bytes[34] = 0;
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()].copy_from_slice(&bytes);
    let before = e.arena().to_vec();
    e.configure_system_register_bank_request().unwrap();
    assert_eq!(
        e.system_register_bank,
        Some(bank().with_application_defaults())
    );
    assert_eq!(e.arena(), before);
}

#[test]
fn six_byte_store_preflights_late_page_before_bytes_and_versions_publish() {
    let mut e = engine();
    e.configure_system_register_bank(bank()).unwrap();
    upload(&mut e, 0x1000, &[0xeb, 0xfe]);
    specs(&mut e, &[0x1000, 2]);
    e.compile(1).unwrap();
    upload(&mut e, 0x2ffd, &[0xa5; 6]);
    e.protect(0x3000, 1, 5).unwrap();
    let memory = e.memory().unwrap();
    let before_version = memory.code_validation_version();
    let snapshot = memory.snapshot_code(GuestAddress(0x2ffd), 6).unwrap();
    let wanted = [0x45, 0x23, 0x78, 0x56, 0x34, 0x12];
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 6].copy_from_slice(&wanted);
    assert_eq!(e.cpu_write(0x2ffd, 6, 1), Ok(StoreCompletion::Complete));
    let memory = e.memory().unwrap();
    let mut observed = [0; 6];
    memory.read(GuestAddress(0x2ffd), &mut observed).unwrap();
    assert_eq!(observed, [0xa5; 6]);
    assert_eq!(memory.code_validation_version(), before_version);
    assert!(memory.is_code_current(&snapshot));
    let packet = &e.arena()[HELPER_OFFSET..HELPER_OFFSET + 40];
    let field = |at| u32::from_le_bytes(packet[at..at + 4].try_into().unwrap());
    assert_eq!(
        (field(16), field(24), field(28), field(32), field(36)),
        (9, 7, 0x3000, 2, 6)
    );
    assert_eq!(&packet[4..6], &15_u16.to_le_bytes());
    assert_eq!(e.cr2(), Some(GuestAddress(0x3000)));
    e.protect(0x3000, 1, 7).unwrap();
    assert_eq!(e.cpu_write(0x2ffd, 6, 1), Ok(StoreCompletion::Complete));
    e.memory()
        .unwrap()
        .read(GuestAddress(0x2ffd), &mut observed)
        .unwrap();
    assert_eq!(observed, wanted);
    assert!(!e.memory().unwrap().is_code_current(&snapshot));
}

#[test]
fn pending_call_and_callback_busy_precede_frozen_or_invalid_configuration() {
    let mut e = engine();
    e.configure_system_register_bank(bank()).unwrap();
    upload(&mut e, 0x1000, &[0x0f, 0x0b]);
    upload(&mut e, 0x1100, &[0x0f, 0x0b]);
    upload(&mut e, 0x2000, &[0x90, 0xeb, 0xfe]);
    specs(
        &mut e,
        &[0x1000, 2, 0x1100, 2, 0x2000, 3, 0x1000, 7, 0x1100, 8],
    );
    e.compile_with_gates(3, 2).unwrap();
    e.write32(0x8f00, 0x2000).unwrap();
    let arena = e.arena_mut().unwrap();
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
    let token = e
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap()
        .token;
    for callback in [false, true] {
        if callback {
            e.begin_callback(KEY, 1, token, 0x2000, 0x1100, 8, &[])
                .unwrap();
        }
        let before = e.arena().to_vec();
        assert_eq!(
            e.configure_system_register_bank(UserSystemRegisterBank32::default()),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            e.configure_system_register_bank_request(),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(e.arena(), before);
        assert_eq!(
            e.system_register_bank,
            Some(bank().with_application_defaults())
        );
    }
}
