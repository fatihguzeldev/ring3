use super::super::fixtures;

use ring3_engine::{
    abi::{
        arena::{EXIT_OFFSET, HELPER_OFFSET, TRANSFER_OFFSET},
        x86::{decode_state, encode_exit_v3, encode_state_v3},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{SegmentDescriptor, SegmentDescriptorKind, SegmentRegister},
    },
    process::{EngineInstance, ModuleInitializer32, ModuleOperation32, ModuleRuntimeConfig32},
};

fn return_initializer(engine: &mut EngineInstance, init: ModuleInitializer32, result: u32) {
    // typed native return evidence; this probe does not execute guest code.
    let mut state = decode_state(&engine.arena()[..56]).unwrap();
    state.registers[0] = result;
    state.registers[4] = init.return_esp;
    state.eip = init.return_pc;
    encode_state_v3(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: init.return_id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + 40],
    )
    .unwrap();
}

fn failed_initial_attach_then_reload_fs(base: u32) {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine
        .register_guest_module(
            "main.exe",
            &fixtures::linked_image(fixtures::EXE_BASE, false, &["failing.dll"]),
        )
        .unwrap();
    engine
        .register_guest_module(
            "failing.dll",
            &fixtures::entry_library(fixtures::DLL_BASE, &[]),
        )
        .unwrap();

    let initial_context = engine.segment_context().unwrap();
    assert_eq!(
        initial_context.state.selectors[SegmentRegister::Fs.index()],
        0x3b
    );
    assert_eq!(initial_context.state.bases[SegmentRegister::Fs.index()], 0);
    if base != 0 {
        engine
            .configure_segment_descriptor(SegmentDescriptor {
                selector: 0x3b,
                base,
                dpl: 3,
                present: true,
                kind: SegmentDescriptorKind::Data { writable: true },
            })
            .unwrap();
    }
    assert_eq!(
        engine.segment_context().unwrap(),
        initial_context,
        "descriptor configuration preserves the already-loaded initial FS cache"
    );

    let ModuleOperation32::Pending(attach) = engine
        .start_module_process("main.exe", None, ModuleRuntimeConfig32::default())
        .unwrap()
    else {
        panic!("the dependency DLL must run process attach");
    };
    assert_eq!(
        (attach.entry, attach.reason),
        (fixtures::DLL_BASE + 0x1000, 1)
    );
    assert_eq!(
        engine.segment_state().unwrap().bases[SegmentRegister::Fs.index()],
        ModuleRuntimeConfig32::default().runtime_base
    );
    return_initializer(&mut engine, attach, 0);
    let ModuleOperation32::Pending(detach) =
        engine.finish_module_initializer(attach.token).unwrap()
    else {
        panic!("failed process attach must run detach before rollback");
    };
    assert_eq!(
        (detach.entry, detach.reason),
        (fixtures::DLL_BASE + 0x1000, 0)
    );
    return_initializer(&mut engine, detach, 1);
    assert_eq!(
        engine.finish_module_initializer(detach.token).unwrap(),
        ModuleOperation32::InitializationFailed
    );
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.module_handle(None).unwrap(), None);
    assert_eq!(engine.module_handle(Some("failing.dll")).unwrap(), None);
    assert_eq!(engine.module_teb_address(), None);
    assert_eq!(
        engine.segment_context().unwrap(),
        initial_context,
        "the cached architectural FS context was restored"
    );

    // material admission enables the real public segment-load helper after rollback.
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    for (index, word) in [0x1000_u32, 2].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile(1).unwrap();
    engine
        .segment_load(SegmentRegister::Fs.index() as u32, 0x3b)
        .unwrap();
    let helper = &engine.arena()[HELPER_OFFSET..HELPER_OFFSET + 40];
    assert_eq!(
        u32::from_le_bytes(helper[16..20].try_into().unwrap()),
        0,
        "the FS reload succeeds rather than failing descriptor validation"
    );
    let loaded_base = engine.segment_state().unwrap().bases[SegmentRegister::Fs.index()];
    let helper_base = u32::from_le_bytes(helper[20..24].try_into().unwrap());
    eprintln!(
        "configured_base={base:#010x} reloaded_base={loaded_base:#010x} helper_base={helper_base:#010x}"
    );
    assert_eq!(
        helper_base, base,
        "rollback must retain the preconfigured descriptor base"
    );
    assert_eq!(
        loaded_base, base,
        "a later FS reload must observe the restored descriptor"
    );
}

#[test]
fn default_fs_descriptor_is_restored_after_failed_initial_attach() {
    failed_initial_attach_then_reload_fs(0);
}

#[test]
fn preconfigured_fs_descriptor_base_survives_failed_initial_attach() {
    failed_initial_attach_then_reload_fs(0x1234_5000);
}
