use crate::{
    memory::{GuestAddress, MemoryError, PageRange},
    process::{EngineInstance, HostError},
    windows::thread::ThreadError32,
};

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(8, 19).unwrap();
    engine.map(0x10000, 2, 3).unwrap();
    let primary = engine
        .threads
        .prepare_primary_thread(0x10000, PageRange::new(GuestAddress(0x11000), 1).unwrap())
        .unwrap();
    engine.threads.commit_primary_thread(primary);
    engine
}
#[test]
fn tls_reused_indices_zero_actual_guest_slots_and_expansion_is_lazy() {
    let mut engine = engine();
    let mut indices = Vec::new();
    for _ in 0..65 {
        indices.push(engine.allocate_windows_tls().unwrap());
    }
    assert_eq!(indices[64], 64);
    assert_eq!(read(&engine, 0x10f94), 0);
    engine.set_windows_tls(0, 0x12345678).unwrap();
    assert_eq!(read(&engine, 0x10e10), 0x12345678);
    engine.free_windows_tls(0).unwrap();
    assert_eq!(engine.allocate_windows_tls().unwrap(), 0);
    assert_eq!(engine.get_windows_tls(0).unwrap(), 0);
    engine.set_windows_tls(64, 0xcafe).unwrap();
    let expansion = read(&engine, 0x10f94);
    assert_ne!(expansion, 0);
    assert_eq!(read(&engine, expansion), 0xcafe);
    assert_eq!(engine.get_windows_tls(64).unwrap(), 0xcafe);
    engine.write32(expansion, 0xface).unwrap();
    assert_eq!(engine.get_windows_tls(64).unwrap(), 0xface);
    assert_eq!(
        engine.free_windows_tls(1088),
        Err(HostError::Thread(ThreadError32::InvalidParameter))
    );
}
#[test]
fn tls_slot_output_fault_does_not_allocate_index_or_publish_expansion() {
    let mut engine = engine();
    engine.protect(0x10000, 1, 1).unwrap();
    assert!(matches!(
        engine.allocate_windows_tls(),
        Err(HostError::Memory(MemoryError::Fault(_)))
    ));
    engine.protect(0x10000, 1, 3).unwrap();
    assert_eq!(engine.allocate_windows_tls().unwrap(), 0);
    for _ in 1..65 {
        engine.allocate_windows_tls().unwrap();
    }
    engine.protect(0x10000, 1, 1).unwrap();
    let regions = engine.windows_memory.reservation_count();
    let mapped = engine.memory().unwrap().mapped_pages();
    assert!(engine.set_windows_tls(64, 7).is_err());
    assert_eq!(engine.windows_memory.reservation_count(), regions);
    assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
    assert_eq!(read(&engine, 0x10f94), 0);
    engine.protect(0x10000, 1, 3).unwrap();
    engine.set_windows_tls(64, 7).unwrap();
    assert_eq!(engine.get_windows_tls(64).unwrap(), 7);
}

#[cfg(feature = "external-cpu")]
fn prepare_external_tls_get(engine: &mut EngineInstance, index: u32) -> u32 {
    use crate::{
        abi::{arena::STATE_OFFSET, x86::{STATE_SIZE, encode_state_current}},
        cpu::x86::State32,
        windows::thread::ThreadApi32,
    };
    engine.enable_core_execution().unwrap();
    engine.external_cpu_memory_pinned = true;
    let mut state = State32::default();
    state.eip = 0x9000;
    state.registers[4] = 0x11000;
    engine.write32(0x11000, 0x9002).unwrap();
    engine.write32(0x11004, index).unwrap();
    encode_state_current(&state,
        &mut engine.arena.as_mut().get_mut()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    ThreadApi32::TlsGetValue.id()
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_cpu_dirty_policy_tls_get_reads_inline_and_lazy_or_real_expansion() {
    use crate::memory::Permissions;
    let mut engine = engine();
    for index in 0..65 {
        assert_eq!(engine.allocate_windows_tls().unwrap(), index);
    }
    engine.set_windows_tls(0, 0x12345678).unwrap();
    engine.write32(0x10034, 0xf1234567).unwrap();
    for index in [0, 64] {
        let id = prepare_external_tls_get(&mut engine, index);
        let version = engine.memory().unwrap().transaction_version();
        let mapped = engine.memory().unwrap().mapped_pages();
        assert!(engine.external_cpu_can_defer_gate(0x9000, id), "index {index}");
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
        assert_eq!(read(&engine, 0x10034), 0xf1234567);
    }
    assert_eq!(read(&engine, 0x10f94), 0);
    engine.set_windows_tls(64, 0xcafefeed).unwrap();
    let expansion = read(&engine, 0x10f94);
    engine.memory.as_mut().unwrap().protect(
        PageRange::new(GuestAddress(expansion & !4095), 1).unwrap(), Permissions::READ).unwrap();
    let id = prepare_external_tls_get(&mut engine, 64);
    let version = engine.memory().unwrap().transaction_version();
    assert!(engine.external_cpu_can_defer_gate(0x9000, id));
    assert_eq!(engine.get_windows_tls(64).unwrap(), 0xcafefeed);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_cpu_dirty_policy_tls_get_rejects_invalid_or_faulting_reads_without_mutation() {
    use crate::memory::Permissions;
    let mut engine = engine();
    for index in [0, 1088] {
        let id = prepare_external_tls_get(&mut engine, index);
        let version = engine.memory().unwrap().transaction_version();
        assert!(!engine.external_cpu_can_defer_gate(0x9000, id));
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
    }
    for _ in 0..65 { engine.allocate_windows_tls().unwrap(); }
    engine.set_windows_tls(64, 0xabcdef01).unwrap();
    let expansion = read(&engine, 0x10f94);
    let page = PageRange::new(GuestAddress(expansion & !4095), 1).unwrap();
    engine.memory.as_mut().unwrap().protect(page, Permissions::READ.with_guard()).unwrap();
    let id = prepare_external_tls_get(&mut engine, 64);
    let version = engine.memory().unwrap().transaction_version();
    assert!(!engine.external_cpu_can_defer_gate(0x9000, id));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.memory().unwrap().permissions_at(GuestAddress(expansion)),
        Ok(Permissions::READ.with_guard()));
    engine.write32(0x10f94, u32::MAX - 1).unwrap();
    let version = engine.memory().unwrap().transaction_version();
    assert!(!engine.external_cpu_can_defer_gate(0x9000, id));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
}

fn read(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}
