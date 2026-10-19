use super::*;
use crate::{abi::arena::HELPER_OFFSET, cpu::dbt::RegistryError};

#[test]
fn fresh_private_rw_does_not_authorize_a_pending_install_receipt() {
    let mut engine = fixture();
    engine.unmap(0x8000, 1).unwrap();
    let installed = install(&mut engine, 0x1000);
    let bytes = engine.resident_bytes(installed.unit_id).unwrap().to_vec();
    let pending = engine.prepare_cache_entry(KEY, 0x1020).unwrap();
    let candidate = engine.resident_bytes(pending.unit_id).unwrap().to_vec();
    let before = engine.cache_usage().unwrap();
    let source = engine.memory().unwrap().code_source_version();
    let transaction = engine.memory().unwrap().transaction_version();
    engine.map(0x9000, 1, 3).unwrap();
    assert_eq!(engine.memory().unwrap().code_source_version(), source);
    assert_eq!(
        engine.memory().unwrap().code_validation_version(),
        transaction + 1
    );
    assert_eq!(
        engine.install_cached_entry(KEY, pending.serial, pending.unit_id, pending.slot),
        Err(HostError::Call(CallError::StateChanged))
    );
    assert_eq!(engine.cache_usage().unwrap(), before);
    assert_eq!(engine.pending_cache_request().unwrap(), Some(pending));
    assert_eq!(engine.resident_bytes(installed.unit_id).unwrap(), bytes);
    assert_eq!(engine.resident_bytes(pending.unit_id).unwrap(), candidate);
    engine.cancel_cache_request(KEY, pending.serial).unwrap();
    assert_eq!(engine.cache_usage().unwrap().units, 1);
    assert_eq!(engine.cache_usage().unwrap().installed, 1);
    assert!(engine.pending_cache_request().unwrap().is_none());
    assert_eq!(engine.resident_bytes(installed.unit_id).unwrap(), bytes);
}

#[test]
fn fresh_rw_keeps_denied_fault_owner_but_rederives_present_geometry_and_read_view_authority() {
    for consumed in [false, true] {
        let mut engine = EngineInstance::new(3, KEY).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        let pc = if consumed { 0x1fff } else { 0x2000 };
        let length = if consumed { 5 } else { 1 };
        if consumed {
            engine
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(pc), &[0xb8])
                .unwrap();
        }
        engine
            .configure_execution(ExecutionConfig32 {
                units: 4,
                wasm_bytes: 262144,
                promotion_hits: 8,
            })
            .unwrap();
        let owner = install(&mut engine, pc);
        let epoch = engine
            .begin_exception_detection(PendingOwner::Resident(owner.unit_id))
            .unwrap();
        assert_ne!(engine.cpu_read32_view_address(epoch), 0);
        let sequence = engine.cpu_page_fault_sequence32();
        engine
            .cpu_fetch_fault_resident(KEY, owner.unit_id, pc, length)
            .unwrap();
        let first = engine.certified_page_fault32(epoch).unwrap();
        assert_eq!(first.error_code(), 20);
        assert_eq!(first.linear(), GuestAddress(0x2000));
        assert_eq!(engine.cpu_page_fault_sequence32(), sequence + 1);
        assert_eq!(engine.cpu_read32_view_address(epoch), 0);
        let source = engine.memory().unwrap().code_source_version();
        engine.map(0x2000, 1, 3).unwrap();
        assert_eq!(engine.memory().unwrap().code_source_version(), source);
        assert_eq!(
            engine.certified_page_fault32(epoch),
            Some(first),
            "mapping cannot clear a certified witness"
        );
        assert_eq!(engine.cpu_read32_view_address(epoch), 0);
        assert_eq!(
            engine.lookup_installed_resident(KEY, pc).unwrap().unit_id,
            owner.unit_id
        );
        engine
            .cpu_fetch_fault_resident(KEY, owner.unit_id, pc, length)
            .unwrap();
        let second = engine.certified_page_fault32(epoch).unwrap();
        assert_eq!(second.error_code(), 21);
        assert_eq!(second.linear(), GuestAddress(0x2000));
        assert_eq!(engine.cr2(), Some(GuestAddress(0x2000)));
        assert_eq!(engine.cpu_page_fault_sequence32(), sequence + 2);
        let packet = &engine.arena()[HELPER_OFFSET..HELPER_OFFSET + 40];
        let mut expected = [0; 40];
        expected[..16].copy_from_slice(b"R3MH\x0f\0\x01\0\x28\0\0\0\0\0\0\0");
        for (index, value) in [9_u32, 0, 21, 0x2000, 3, length].into_iter().enumerate() {
            expected[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(packet, expected);
        engine.protect(0x2000, 1, 7).unwrap();
        assert!(engine.memory().unwrap().code_source_version() > source);
        assert_eq!(
            engine.lookup_installed_resident(KEY, pc),
            Err(HostError::Resident(RegistryError::NotFound {
                pc: GuestAddress(pc)
            }))
        );
        assert_eq!(engine.cpu_read32_view_address(epoch), 0);
        engine.cpu_read32(0x2000).unwrap();
        assert_eq!(engine.certified_page_fault32(epoch), None);
        assert_ne!(engine.cpu_read32_view_address(epoch), 0);
    }
}
