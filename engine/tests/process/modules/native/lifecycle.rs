use super::fixtures::{DLL_BASE, EXE_BASE, image};
use ring3_engine::{
    abi::x86::{decode_state, encode_exit_v3, encode_state_v3},
    cpu::{ExecutionExit, ExitReason},
    memory::GuestAddress,
    process::{EngineInstance, ModuleOperation32, ModuleRuntimeConfig32},
};

fn started() -> EngineInstance {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine
        .register_guest_module("main.exe", &image(EXE_BASE, false, true))
        .unwrap();
    assert_eq!(
        engine
            .start_module_process("main.exe", None, ModuleRuntimeConfig32::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: EXE_BASE }
    );
    engine
}

#[test]
fn initial_exe_and_no_entry_dll_keep_one_address_space_and_reference_lifetime() {
    let mut engine = started();
    engine
        .register_guest_module("helper.dll", &image(DLL_BASE, true, false))
        .unwrap();
    let initial = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(EXE_BASE + 0x1000), 8)
        .unwrap();
    assert_eq!(
        engine.load_library_module("HELPER.DLL", None).unwrap(),
        ModuleOperation32::Loaded { handle: DLL_BASE }
    );
    assert_eq!(
        engine.load_library_module("helper.dll", None).unwrap(),
        ModuleOperation32::Loaded { handle: DLL_BASE }
    );
    assert_eq!(
        engine.module_handle(Some("HeLpEr.DlL")).unwrap(),
        Some(DLL_BASE)
    );
    assert_eq!(
        engine.free_library_module(DLL_BASE).unwrap(),
        ModuleOperation32::Released
    );
    assert_eq!(
        engine.module_handle(Some("helper.dll")).unwrap(),
        Some(DLL_BASE)
    );
    assert_eq!(
        engine.free_library_module(DLL_BASE).unwrap(),
        ModuleOperation32::Released
    );
    assert_eq!(engine.module_handle(Some("helper.dll")).unwrap(), None);
    assert!(engine.memory().unwrap().is_code_current(&initial));
    assert_eq!(engine.module_handle(None).unwrap(), Some(EXE_BASE));
}

#[test]
fn failed_load_and_invalid_initializer_token_are_neutral() {
    let mut engine = started();
    let before = engine.arena().to_vec();
    let pages = engine.memory().unwrap().mapped_pages();
    assert!(engine.load_library_module("missing.dll", None).is_err());
    assert!(engine.finish_module_initializer(1).is_err());
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), pages);
    assert_eq!(engine.module_handle(None).unwrap(), Some(EXE_BASE));
}

#[test]
fn initial_thread_rejects_a_raw_gate_at_the_fs_selector_before_publication() {
    use ring3_engine::cpu::x86::SegmentDescriptorRecord;
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine
        .register_guest_module("main.exe", &image(EXE_BASE, false, true))
        .unwrap();
    engine
        .configure_segment_descriptor_raw(
            0x3b,
            SegmentDescriptorRecord::from_words(0xdead_beef, 0x89ab_ec76),
        )
        .unwrap();
    let before = engine.arena().to_vec();
    let pages = engine.memory().unwrap().mapped_pages();
    assert_eq!(
        engine.start_module_process("main.exe", None, ModuleRuntimeConfig32::default()),
        Err(ring3_engine::process::HostError::Module(
            ring3_engine::loader::ModuleError32::InvalidRequest
        ))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), pages);
    assert_eq!(engine.module_handle(None).unwrap(), None);
}

#[test]
fn dll_attach_false_runs_detach_and_rolls_back_only_new_modules() {
    let mut engine = started();
    engine
        .register_guest_module("failing.dll", &image(DLL_BASE, true, true))
        .unwrap();
    let before = engine.arena()[..56].to_vec();
    let pages = engine.memory().unwrap().mapped_pages();
    let ModuleOperation32::Pending(attach) =
        engine.load_library_module("failing.dll", None).unwrap()
    else {
        panic!("real DLL attach must be scheduled");
    };
    assert_eq!(attach.entry, DLL_BASE + 0x1000);
    assert_eq!(attach.reason, 1);
    assert_eq!(
        engine.module_handle(Some("failing.dll")).unwrap(),
        Some(DLL_BASE)
    );
    return_initializer(&mut engine, attach, 0);
    let ModuleOperation32::Pending(detach) =
        engine.finish_module_initializer(attach.token).unwrap()
    else {
        panic!("failed attach must detach");
    };
    assert_eq!(detach.reason, 0);
    return_initializer(&mut engine, detach, 1);
    assert_eq!(
        engine.finish_module_initializer(detach.token).unwrap(),
        ModuleOperation32::InitializationFailed
    );
    assert_eq!(engine.arena()[..56], before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), pages);
    assert_eq!(engine.module_handle(Some("failing.dll")).unwrap(), None);
}

pub(super) fn return_initializer(
    engine: &mut EngineInstance,
    init: ring3_engine::process::ModuleInitializer32,
    result: u32,
) {
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
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
}

#[test]
fn nested_explicit_reference_survives_the_outer_failed_attach() {
    use super::fixtures::{entry_library, linked_image};
    let mut engine = started();
    engine
        .register_guest_module("outer.dll", &entry_library(DLL_BASE, &["kept.dll"]))
        .unwrap();
    engine
        .register_guest_module("kept.dll", &linked_image(0x800000, true, &[]))
        .unwrap();
    let ModuleOperation32::Pending(attach) = engine.load_library_module("outer.dll", None).unwrap()
    else {
        panic!("outer attach");
    };
    assert_eq!(
        engine.load_library_module("kept.dll", None).unwrap(),
        ModuleOperation32::Loaded { handle: 0x800000 }
    );
    return_initializer(&mut engine, attach, 0);
    let ModuleOperation32::Pending(detach) =
        engine.finish_module_initializer(attach.token).unwrap()
    else {
        panic!("failed outer detach");
    };
    return_initializer(&mut engine, detach, 1);
    assert_eq!(
        engine.finish_module_initializer(detach.token).unwrap(),
        ModuleOperation32::InitializationFailed
    );
    assert_eq!(engine.module_handle(Some("outer.dll")).unwrap(), None);
    assert_eq!(
        engine.module_handle(Some("kept.dll")).unwrap(),
        Some(0x800000)
    );
    assert_eq!(
        engine.free_library_module(0x800000).unwrap(),
        ModuleOperation32::Released
    );
    assert_eq!(engine.module_handle(Some("kept.dll")).unwrap(), None);
}

#[test]
fn failed_demand_forwarder_removes_only_its_added_edge_and_can_retry() {
    use super::fixtures::{entry_library, forwarding_exe};
    use ring3_engine::loader::ImportSymbol32;
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine
        .register_guest_module("main.exe", &forwarding_exe(EXE_BASE, "failing.answer"))
        .unwrap();
    engine
        .register_guest_module("failing.dll", &entry_library(DLL_BASE, &[]))
        .unwrap();
    assert_eq!(
        engine
            .start_module_process("main.exe", None, ModuleRuntimeConfig32::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: EXE_BASE }
    );
    let symbol = ImportSymbol32::Name("answer".into());
    let pages = engine.memory().unwrap().mapped_pages();
    let ModuleOperation32::Pending(attach) =
        engine.resolve_module_export(EXE_BASE, &symbol).unwrap()
    else {
        panic!("demand forwarder attach");
    };
    return_initializer(&mut engine, attach, 0);
    let ModuleOperation32::Pending(detach) =
        engine.finish_module_initializer(attach.token).unwrap()
    else {
        panic!("demand forwarder failed-attach detach");
    };
    return_initializer(&mut engine, detach, 1);
    assert_eq!(
        engine.finish_module_initializer(detach.token).unwrap(),
        ModuleOperation32::InitializationFailed
    );
    assert_eq!(engine.module_handle(None).unwrap(), Some(EXE_BASE));
    assert_eq!(engine.module_handle(Some("failing.dll")).unwrap(), None);
    assert_eq!(engine.memory().unwrap().mapped_pages(), pages);
    let ModuleOperation32::Pending(retry) =
        engine.resolve_module_export(EXE_BASE, &symbol).unwrap()
    else {
        panic!("fresh demand forwarder retry");
    };
    assert!(retry.module.get() > attach.module.get());
    return_initializer(&mut engine, retry, 1);
    assert_eq!(
        engine.finish_module_initializer(retry.token).unwrap(),
        ModuleOperation32::ExportResolved {
            address: DLL_BASE + 0x1000
        }
    );
    assert_eq!(
        engine.module_export(EXE_BASE, &symbol).unwrap().address(),
        GuestAddress(DLL_BASE + 0x1000)
    );
    assert!(
        engine.free_library_module(DLL_BASE).is_err(),
        "forwarder dependency is not an explicit LoadLibrary reference"
    );
}

#[test]
fn repeated_live_export_lookup_preserves_state_and_does_not_retain_the_library() {
    use super::fixtures::linked_image;
    use ring3_engine::loader::ImportSymbol32;
    let mut engine = started();
    engine
        .register_guest_module("helper.dll", &linked_image(DLL_BASE, true, &[]))
        .unwrap();
    assert_eq!(
        engine.load_library_module("helper.dll", None).unwrap(),
        ModuleOperation32::Loaded { handle: DLL_BASE }
    );
    let arena = engine.arena().to_vec();
    let pages = engine.memory().unwrap().mapped_pages();
    let code = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(DLL_BASE + 0x1000), 16)
        .unwrap();
    let mut before = [0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(DLL_BASE + 0x3000), &mut before)
        .unwrap();
    let symbol = ImportSymbol32::Name("answer".into());
    for _ in 0..3 {
        assert_eq!(
            engine.resolve_module_export(DLL_BASE, &symbol).unwrap(),
            ModuleOperation32::ExportResolved {
                address: DLL_BASE + 0x1000
            }
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.memory().unwrap().mapped_pages(), pages);
        assert!(engine.memory().unwrap().is_code_current(&code));
        let mut after = [0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(DLL_BASE + 0x3000), &mut after)
            .unwrap();
        assert_eq!(after, before);
    }
    assert_eq!(
        engine.free_library_module(DLL_BASE).unwrap(),
        ModuleOperation32::Released
    );
    assert_eq!(engine.module_handle(Some("helper.dll")).unwrap(), None);
}

#[test]
fn a_failed_attach_cannot_survive_its_recursive_self_reference() {
    let mut engine = started();
    engine
        .register_guest_module("failing.dll", &image(DLL_BASE, true, true))
        .unwrap();
    let pages = engine.memory().unwrap().mapped_pages();
    let ModuleOperation32::Pending(attach) =
        engine.load_library_module("failing.dll", None).unwrap()
    else {
        panic!("real DLL attach must be scheduled");
    };
    assert_eq!(
        engine.load_library_module("failing.dll", None),
        Err(ring3_engine::process::HostError::Module(
            ring3_engine::loader::ModuleError32::Busy
        ))
    );
    return_initializer(&mut engine, attach, 0);
    let ModuleOperation32::Pending(detach) =
        engine.finish_module_initializer(attach.token).unwrap()
    else {
        panic!("failed attach must detach despite a recursive reference");
    };
    assert_eq!(detach.reason, 0);
    return_initializer(&mut engine, detach, 1);
    assert_eq!(
        engine.finish_module_initializer(detach.token).unwrap(),
        ModuleOperation32::InitializationFailed
    );
    assert_eq!(engine.module_handle(Some("failing.dll")).unwrap(), None);
    assert_eq!(engine.memory().unwrap().mapped_pages(), pages);
}
