use super::{
    fixtures::{EXE_BASE, tls_image},
    lifecycle::return_initializer,
};
use ring3_engine::{
    abi::x86::decode_state,
    cpu::x86::SegmentRegister,
    memory::GuestAddress,
    process::{EngineInstance, ModuleOperation32, ModuleRuntimeConfig32},
};

fn word(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn initial_thread_tls_template_index_teb_and_callback_precede_exe_entry() {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine
        .register_guest_module("main.exe", &tls_image(EXE_BASE, false))
        .unwrap();
    let ModuleOperation32::Pending(callback) = engine
        .start_module_process("main.exe", None, ModuleRuntimeConfig32::default())
        .unwrap()
    else {
        panic!("TLS callback must precede EXE entry");
    };
    assert_eq!(callback.entry, EXE_BASE + 0x1010);
    let fs = engine.segment_state().unwrap().bases[SegmentRegister::Fs.index()];
    assert_ne!(fs, 0);
    assert_eq!(word(&engine, fs + 0x18), fs);
    let array = word(&engine, fs + 0x2c);
    let index = word(&engine, EXE_BASE + 0x5000);
    let block = word(&engine, array + index * 4);
    assert_eq!(block % 4, 0);
    let mut data = [0xff; 16];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(block), &mut data)
        .unwrap();
    assert_eq!(
        data,
        [0xaa, 0x11, 0x22, 0x33, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    let state = decode_state(&engine.arena()[..56]).unwrap();
    assert_eq!(word(&engine, state.registers[4]), callback.return_pc);
    assert_eq!(word(&engine, state.registers[4] + 4), EXE_BASE);
    assert_eq!(word(&engine, state.registers[4] + 8), 1);
    assert_eq!(word(&engine, state.registers[4] + 12), 0);
    return_initializer(&mut engine, callback, 0);
    assert_eq!(
        engine.finish_module_initializer(callback.token).unwrap(),
        ModuleOperation32::Loaded { handle: EXE_BASE }
    );
    assert_eq!(
        decode_state(&engine.arena()[..56]).unwrap().eip,
        EXE_BASE + 0x1000
    );
    assert_eq!(
        engine.segment_state().unwrap().bases[SegmentRegister::Fs.index()],
        fs
    );
}

#[test]
fn callback_iteration_reads_the_live_array_after_each_return() {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine
        .register_guest_module("main.exe", &tls_image(EXE_BASE, false))
        .unwrap();
    let ModuleOperation32::Pending(first) = engine
        .start_module_process("main.exe", None, ModuleRuntimeConfig32::default())
        .unwrap()
    else {
        panic!("first TLS callback");
    };
    engine
        .write32(EXE_BASE + 0x3044, EXE_BASE + 0x1010)
        .unwrap();
    engine.write32(EXE_BASE + 0x3048, 0).unwrap();
    return_initializer(&mut engine, first, 0);
    let ModuleOperation32::Pending(second) = engine.finish_module_initializer(first.token).unwrap()
    else {
        panic!("newly supplied live callback");
    };
    assert_eq!(second.entry, first.entry);
    assert_ne!(second.token, first.token);
    return_initializer(&mut engine, second, 0);
    assert_eq!(
        engine.finish_module_initializer(second.token).unwrap(),
        ModuleOperation32::Loaded { handle: EXE_BASE }
    );
}

#[test]
fn pending_initializers_own_host_descriptor_configuration() {
    use ring3_engine::{
        cpu::x86::SegmentDescriptorRecord,
        process::{CallError, HostError},
    };
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine
        .register_guest_module("main.exe", &tls_image(EXE_BASE, false))
        .unwrap();
    let ModuleOperation32::Pending(callback) = engine
        .start_module_process("main.exe", None, ModuleRuntimeConfig32::default())
        .unwrap()
    else {
        panic!("TLS callback must precede EXE entry");
    };
    let arena = engine.arena().to_vec();
    let fs = engine.segment_state().unwrap().bases[SegmentRegister::Fs.index()];
    assert_eq!(
        engine.configure_segment_descriptor_raw(
            0x3b,
            SegmentDescriptorRecord::from_words(0xdead_beef, 0x89ab_ec76),
        ),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(
        engine.set_segment_base(SegmentRegister::Fs, 0),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), arena);
    return_initializer(&mut engine, callback, 0);
    assert_eq!(
        engine.finish_module_initializer(callback.token).unwrap(),
        ModuleOperation32::Loaded { handle: EXE_BASE }
    );
    let arena = engine.arena().to_vec();
    engine.set_segment_base(SegmentRegister::Fs, fs).unwrap();
    assert_eq!(engine.arena(), arena);
}

#[test]
fn accepted_return_retries_next_frame_without_replaying_the_completed_callback() {
    use ring3_engine::abi::arena::CANCEL_OFFSET;
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine
        .register_guest_module("main.exe", &tls_image(EXE_BASE, false))
        .unwrap();
    let config = ModuleRuntimeConfig32::default();
    let ModuleOperation32::Pending(first) = engine
        .start_module_process("main.exe", None, config)
        .unwrap()
    else {
        panic!("first TLS callback");
    };
    engine
        .write32(EXE_BASE + 0x3044, EXE_BASE + 0x1010)
        .unwrap();
    engine.write32(EXE_BASE + 0x3048, 0).unwrap();
    return_initializer(&mut engine, first, 0);
    engine
        .protect(config.stack_base, config.stack_pages, 1)
        .unwrap();
    let returned = engine.arena()[..96].to_vec();
    assert!(engine.finish_module_initializer(first.token).is_err());
    assert_eq!(engine.arena()[..96], returned);
    assert_eq!(engine.module_initializer().unwrap(), None);
    engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
        .copy_from_slice(&1_u32.to_le_bytes());
    assert_eq!(
        engine.finish_module_initializer(first.token),
        Err(ring3_engine::process::HostError::Call(
            ring3_engine::process::CallError::Cancelled
        ))
    );
    engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
        .copy_from_slice(&0_u32.to_le_bytes());
    engine
        .protect(config.stack_base, config.stack_pages, 3)
        .unwrap();
    let ModuleOperation32::Pending(second) = engine.finish_module_initializer(first.token).unwrap()
    else {
        panic!("resumed second TLS callback");
    };
    assert_ne!(second.token, first.token);
    let before = engine.arena().to_vec();
    assert!(engine.finish_module_initializer(first.token).is_err());
    assert_eq!(engine.arena(), before);
    return_initializer(&mut engine, second, 0);
    assert_eq!(
        engine.finish_module_initializer(second.token).unwrap(),
        ModuleOperation32::Loaded { handle: EXE_BASE }
    );
}
