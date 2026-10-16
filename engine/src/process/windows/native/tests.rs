use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::GuestAddress,
    process::EngineInstance,
    windows::{CallingConvention32, native::NativeApi32},
};
fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(32, 7).unwrap();
    engine.map(0x10000, 2, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10000), &[0x0f, 0x0b])
        .unwrap();
    engine.protect(0x10000, 1, 5).unwrap();
    engine.protect(0x11000, 1, 3).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
}
fn call(engine: &mut EngineInstance, api: NativeApi32, args: &[u32]) -> u32 {
    for (index, word) in [0x10000u32, 2, 0x10000, api.id()].iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    let mut state = crate::cpu::x86::State32 {
        eip: 0x10000,
        ..Default::default()
    };
    state.registers[4] = 0x11800;
    let words = std::iter::once(0x10001)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11800), &words)
        .unwrap();
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: api.id() },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_call(
            7,
            engine.generation,
            CallingConvention32::Stdcall,
            args.len() as u32,
        )
        .unwrap()
        .token;
    engine
        .complete_windows_call(7, engine.generation, token)
        .unwrap();
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}
#[test]
fn native_clock_returns_access_violation_without_partial_output_or_last_error_change() {
    for address in [0, 0x20000, 0x10000, 0x11ffc, u32::MAX - 3] {
        let mut engine = engine();
        engine.windows_thread = engine.windows_thread.with_last_error(77);
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x11ffc), &[0xa5; 4])
            .unwrap();
        assert_eq!(
            call(&mut engine, NativeApi32::NtQuerySystemTime, &[address]),
            0xc0000005,
            "output {address:#x}"
        );
        let mut tail = [0; 4];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x11ffc), &mut tail)
            .unwrap();
        assert_eq!(tail, [0xa5; 4]);
        assert_eq!(engine.windows_thread.last_error(), 77);
    }
}
#[test]
fn native_clock_and_debug_services_preserve_win32_last_error_and_publish_real_bounded_receipts() {
    let mut engine = engine();
    engine.windows_thread = engine.windows_thread.with_last_error(77);
    assert_eq!(
        call(&mut engine, NativeApi32::NtQuerySystemTime, &[0x11000]),
        0
    );
    let mut bytes = [0; 8];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x11000), &mut bytes)
        .unwrap();
    assert_eq!(
        u64::from_le_bytes(bytes),
        engine.windows_filetime().unwrap()
    );
    assert_eq!(engine.windows_thread.last_error(), 77);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11100), b"crt: \0")
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11200), b"%s %08x %I64u\0")
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11300), b"ready\0")
        .unwrap();
    let words = [0x11300u32, 0x1234, 0xffffffff, 1]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11400), &words)
        .unwrap();
    assert_eq!(
        call(
            &mut engine,
            NativeApi32::VDbgPrintExWithPrefix,
            &[0x11100, 9, 2, 0x11200, 0x11400]
        ),
        0
    );
    let receipt = engine.last_windows_debug_output().unwrap().clone();
    assert_eq!(receipt.bytes(), b"crt: ready 00001234 8589934591");
    assert_eq!(receipt.component, 9);
    assert_eq!(receipt.level, 2);
    assert_eq!(engine.windows_thread.last_error(), 77);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11200), b"%n\0")
        .unwrap();
    assert_eq!(
        call(
            &mut engine,
            NativeApi32::VDbgPrintExWithPrefix,
            &[0x11100, 9, 2, 0x11200, 0x11400]
        ),
        0xc000000d
    );
    assert_eq!(engine.last_windows_debug_output(), Some(&receipt));
    assert_eq!(engine.windows_thread.last_error(), 77);
    let long = [b'x'; 600];
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11200), &long)
        .unwrap();
    assert_eq!(
        call(
            &mut engine,
            NativeApi32::VDbgPrintExWithPrefix,
            &[0x11100, 9, 2, 0x11200, 0]
        ),
        0
    );
    assert_eq!(
        engine.last_windows_debug_output().unwrap().bytes().len(),
        512
    );
}
#[test]
fn native_loader_reference_requires_exact_base_and_real_pinning_survives_release() {
    use crate::loader::SystemModule32;
    let mut engine = EngineInstance::new(128, 7).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    let mut image = vec![0; 1024];
    image[..2].copy_from_slice(b"MZ");
    image[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (at, value) in [
        (0x84, 0x14cu16),
        (0x86, 1),
        (0x94, 224),
        (0x96, 0x103),
        (0x98, 0x10b),
        (0xdc, 3),
    ] {
        image[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in [
        (0x3c, 0x80u32),
        (0xa8, 0x1000),
        (0xb4, 0x400000),
        (0xb8, 4096),
        (0xbc, 512),
        (0xd0, 0x2000),
        (0xd4, 512),
        (0xf4, 16),
        (0x180, 512),
        (0x184, 4096),
        (0x188, 512),
        (0x18c, 512),
        (0x19c, 0x60000020),
    ] {
        image[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    image[0x178..0x180].copy_from_slice(b".text\0\0\0");
    image[512] = 0xc3;
    engine.register_guest_module("native.exe", &image).unwrap();
    engine
        .start_module_process("native.exe", None, Default::default())
        .unwrap();
    engine.map(0x10000, 2, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10000), &[0x0f, 0x0b])
        .unwrap();
    engine.protect(0x10000, 1, 5).unwrap();
    engine.protect(0x11000, 1, 3).unwrap();
    engine
        .register_system_module(SystemModule32 {
            name: "retained.dll".into(),
            preferred_base: 0x600000,
            exports: vec![],
        })
        .unwrap();
    engine.load_library_module("retained.dll", None).unwrap();
    assert_eq!(
        call(&mut engine, NativeApi32::LdrAddRefDll, &[0, 0x600001]),
        0xc000000d
    );
    assert_eq!(
        call(&mut engine, NativeApi32::LdrAddRefDll, &[2, 0x600000]),
        0xc000000d
    );
    assert_eq!(
        call(&mut engine, NativeApi32::LdrAddRefDll, &[0, 0x600000]),
        0
    );
    engine.free_library_module(0x600000).unwrap();
    assert_eq!(
        engine.module_handle(Some("retained.dll")).unwrap(),
        Some(0x600000)
    );
    assert_eq!(
        call(&mut engine, NativeApi32::LdrAddRefDll, &[1, 0x600000]),
        0
    );
    for _ in 0..3 {
        engine.free_library_module(0x600000).unwrap();
    }
    assert_eq!(
        engine.module_handle(Some("retained.dll")).unwrap(),
        Some(0x600000)
    );
}

#[test]
fn unterminated_debug_format_flags_stop_at_the_bounded_input_before_the_next_page() {
    let mut engine = engine();
    engine.map(0x12000, 1, 3).unwrap();
    let mut format = [b'0'; 4096];
    format[0] = b'%';
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12000), &format)
        .unwrap();
    engine.windows_thread = engine.windows_thread.with_last_error(77);
    assert_eq!(
        call(
            &mut engine,
            NativeApi32::VDbgPrintExWithPrefix,
            &[0x10010, 9, 2, 0x12000, 0]
        ),
        0xc000000d
    );
    assert!(engine.last_windows_debug_output().is_none());
    assert_eq!(engine.windows_thread.last_error(), 77);
}
