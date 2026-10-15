use super::super::fixtures;

use ring3_engine::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
        x86::{
            SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, decode_state, encode_current_segment_context,
        },
    },
    cpu::x86::{SegmentCache, SegmentRegister},
    loader::ModuleError32,
    memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{CallError, EngineInstance, HostError, ModuleOperation32, ModuleRuntimeConfig32},
};

fn started() -> EngineInstance {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine
        .register_guest_module(
            "main.exe",
            &fixtures::image(fixtures::EXE_BASE, false, true),
        )
        .unwrap();
    assert_eq!(
        engine
            .start_module_process("main.exe", None, ModuleRuntimeConfig32::default())
            .unwrap(),
        ModuleOperation32::Loaded {
            handle: fixtures::EXE_BASE
        }
    );
    engine
        .register_guest_module(
            "entry.dll",
            &fixtures::image(fixtures::DLL_BASE, true, true),
        )
        .unwrap();
    assert_eq!(engine.module_initializer().unwrap(), None);
    engine
}

fn bytes(engine: &EngineInstance, address: u32) -> [u8; 16] {
    let mut output = [0; 16];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut output)
        .unwrap();
    output
}

#[test]
fn flat_initializer_control_uses_the_expected_stack_and_entry() {
    let mut engine = started();
    let before = decode_state(&engine.arena()[..56]).unwrap();
    let raw = before.registers[4] - 16;
    assert_eq!(bytes(&engine, raw), [0; 16]);
    let result = engine.load_library_module("entry.dll", None).unwrap();
    let ModuleOperation32::Pending(init) = result else {
        panic!("flat positive: {result:?}")
    };
    assert_eq!(init.entry, fixtures::DLL_BASE + 0x1000);
    let mut expected = [0; 16];
    for (output, word) in
        expected
            .chunks_exact_mut(4)
            .zip([init.return_pc, fixtures::DLL_BASE, 1, 0])
    {
        output.copy_from_slice(&word.to_le_bytes());
    }
    assert_eq!(bytes(&engine, raw), expected);
    assert_eq!(
        decode_state(&engine.arena()[..56]).unwrap().registers[4],
        raw
    );
}

fn invalid_initializer(mut engine: EngineInstance, label: &str) {
    let state = decode_state(&engine.arena()[..56]).unwrap();
    let raw = state.registers[4] - 16;
    let translated = raw + 0x1000;
    engine.map(translated & !4095, 1, 3).unwrap();
    let raw_before = bytes(&engine, raw);
    let translated_before = bytes(&engine, translated);
    let arena_before = engine.arena().to_vec();
    let pages_before = engine.memory().unwrap().mapped_pages();
    let result = engine.load_library_module("entry.dll", None);
    let raw_after = bytes(&engine, raw);
    let translated_after = bytes(&engine, translated);
    eprintln!(
        "context={label} result={result:?} pages={pages_before}->{} raw_changed={} translated_changed={} arena_changed={} module={:?}",
        engine.memory().unwrap().mapped_pages(),
        raw_after != raw_before,
        translated_after != translated_before,
        engine.arena() != arena_before,
        engine.module_handle(Some("entry.dll")).unwrap(),
    );
    assert!(
        matches!(
            result,
            Err(HostError::Module(ModuleError32::InvalidRequest))
                | Err(HostError::Call(CallError::InvalidRequest))
        ),
        "unsupported nonflat Windows initializer must be rejected before publication"
    );
    assert_eq!(engine.arena(), arena_before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), pages_before);
    assert_eq!(engine.module_handle(Some("entry.dll")).unwrap(), None);
    assert_eq!(raw_after, raw_before);
    assert_eq!(translated_after, translated_before);
}

#[test]
fn nonflat_ss_must_not_write_a_frame_at_untranslated_esp() {
    let mut engine = started();
    engine
        .set_segment_base(SegmentRegister::Ss, 0x1000)
        .unwrap();
    invalid_initializer(engine, "nonflat ss");
}

#[test]
fn nonflat_cs_must_not_schedule_a_linear_dll_entry_as_an_offset() {
    let mut engine = started();
    engine
        .set_segment_base(SegmentRegister::Cs, 0x1000)
        .unwrap();
    invalid_initializer(engine, "nonflat cs");
}

fn stack_cache(bits: u32, label: &str) {
    let mut engine = started();
    let mut context = engine.segment_context().unwrap();
    context.caches[SegmentRegister::Ss.index()] = SegmentCache::from_bits(bits).unwrap();
    assert!(context.is_valid());
    let mut segments = [0; SEGMENTS_SIZE];
    let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
    encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
    let arena = engine.arena_mut().unwrap();
    arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].copy_from_slice(&segments);
    arena[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
        .copy_from_slice(&bases);
    assert_eq!(engine.segment_context().unwrap(), context);
    invalid_initializer(engine, label);
}

#[test]
fn bounded_ss_must_not_publish_an_out_of_limit_initializer_frame() {
    stack_cache(0xf230_7fff, "bounded ss");
}

#[test]
fn b16_ss_must_not_use_full_esp_for_a_dword_callback_frame() {
    stack_cache(0xf13f_ffff, "b16 ss");
}

#[test]
fn expand_down_ss_must_not_publish_an_inaccessible_initializer_frame() {
    stack_cache(0xf37f_ffff, "full-limit expand-down ss");
}

#[test]
fn no_frame_load_and_unload_preserve_a_nonflat_host_context() {
    for register in [SegmentRegister::Cs, SegmentRegister::Ss] {
        let mut engine = started();
        engine
            .register_guest_module(
                "plain.dll",
                &fixtures::image(fixtures::DLL_BASE, true, false),
            )
            .unwrap();
        engine.set_segment_base(register, 0x1000).unwrap();
        let arena = engine.arena().to_vec();
        let pages = engine.memory().unwrap().mapped_pages();
        let stack = decode_state(&arena[..56]).unwrap().registers[4] - 16;
        let stack_before = bytes(&engine, stack);
        assert_eq!(
            engine.load_library_module("plain.dll", None).unwrap(),
            ModuleOperation32::Loaded {
                handle: fixtures::DLL_BASE
            }
        );
        assert_eq!(engine.module_initializer().unwrap(), None);
        assert_eq!(engine.arena(), arena);
        assert_eq!(bytes(&engine, stack), stack_before);
        assert_eq!(
            engine.free_library_module(fixtures::DLL_BASE).unwrap(),
            ModuleOperation32::Released
        );
        assert_eq!(engine.module_handle(Some("plain.dll")).unwrap(), None);
        assert_eq!(engine.memory().unwrap().mapped_pages(), pages);
        assert_eq!(engine.arena(), arena);
        assert_eq!(bytes(&engine, stack), stack_before);
    }
}

#[test]
fn rejected_detach_does_not_publish_an_operation_or_release_a_reference() {
    for shutdown in [false, true] {
        let mut engine = started();
        let ModuleOperation32::Pending(attach) =
            engine.load_library_module("entry.dll", None).unwrap()
        else {
            panic!("the DLL has a process-attach entry");
        };
        super::super::lifecycle::return_initializer(&mut engine, attach, 1);
        assert_eq!(
            engine.finish_module_initializer(attach.token).unwrap(),
            ModuleOperation32::Loaded {
                handle: fixtures::DLL_BASE
            }
        );
        engine
            .set_segment_base(SegmentRegister::Ss, 0x1000)
            .unwrap();
        let arena = engine.arena().to_vec();
        let pages = engine.memory().unwrap().mapped_pages();
        let stack = decode_state(&arena[..56]).unwrap().registers[4] - 16;
        let stack_before = bytes(&engine, stack);
        let result = if shutdown {
            engine.detach_modules_for_exit()
        } else {
            engine.free_library_module(fixtures::DLL_BASE)
        };
        assert_eq!(
            result,
            Err(HostError::Module(ModuleError32::InvalidRequest))
        );
        assert_eq!(engine.module_initializer().unwrap(), None);
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.memory().unwrap().mapped_pages(), pages);
        assert_eq!(bytes(&engine, stack), stack_before);
        assert_eq!(
            engine.module_handle(Some("entry.dll")).unwrap(),
            Some(fixtures::DLL_BASE)
        );
        engine.set_segment_base(SegmentRegister::Ss, 0).unwrap();
        let retried = if shutdown {
            engine.detach_modules_for_exit()
        } else {
            engine.free_library_module(fixtures::DLL_BASE)
        }
        .unwrap();
        let ModuleOperation32::Pending(detach) = retried else {
            panic!("the failed attempt must retain its detach work and explicit reference");
        };
        assert_eq!(
            (detach.entry, detach.reason),
            (fixtures::DLL_BASE + 0x1000, 0)
        );
    }
}

#[test]
fn shutdown_retries_an_unpublished_first_frame_after_memory_repair() {
    let mut engine = started();
    let ModuleOperation32::Pending(attach) = engine.load_library_module("entry.dll", None).unwrap()
    else {
        panic!("the DLL has a process-attach entry");
    };
    super::super::lifecycle::return_initializer(&mut engine, attach, 1);
    engine.finish_module_initializer(attach.token).unwrap();
    let config = ModuleRuntimeConfig32::default();
    engine
        .protect(config.stack_base, config.stack_pages, 1)
        .unwrap();
    let arena = engine.arena().to_vec();
    let frame_address = decode_state(&arena[..56]).unwrap().registers[4] - 16;
    let frame = bytes(&engine, frame_address);
    assert_eq!(
        engine.detach_modules_for_exit(),
        Err(HostError::Memory(MemoryError::Fault(MemoryFault {
            address: GuestAddress(frame_address),
            access: Access::Write,
            reason: FaultReason::Permission,
        })))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(bytes(&engine, frame_address), frame);
    assert_eq!(engine.module_initializer().unwrap(), None);
    assert_eq!(
        engine.finish_module_initializer(0),
        Err(HostError::Module(ModuleError32::InvalidToken))
    );
    assert_eq!(engine.arena(), arena);
    engine.set_segment_base(SegmentRegister::Ss, 0).unwrap();
    engine
        .protect(config.stack_base, config.stack_pages, 3)
        .unwrap();
    let ModuleOperation32::Pending(detach) = engine.detach_modules_for_exit().unwrap() else {
        panic!("shutdown retries its unpublished first frame after memory repair");
    };
    assert_eq!(
        (detach.entry, detach.reason),
        (fixtures::DLL_BASE + 0x1000, 0)
    );
}
