#[path = "../../../../../../tests/loader/modules/linking/native/image.rs"]
mod pe;

use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{GuestAddress, PageRange},
    process::{EngineInstance, HostError},
    windows::{CallingConvention32, user32::User32Limits32, window::WindowHandle32},
};

const KEY: u64 = 0x4449414c;
const CODE: u32 = 0x50000;
const DATA: u32 = 0x60000;
const STACK: u32 = DATA + 0x1f00;

mod guard;
mod lifecycle;

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(256, KEY).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine
        .configure_user32(
            User32Limits32 {
                classes: 4,
                windows: 16,
                messages: 32,
                continuations: 8,
            },
            PageRange::new(GuestAddress(0x30000), 16).unwrap(),
        )
        .unwrap();
    engine
        .configure_gdi(crate::process::GdiConfig32 {
            bitmap_base: 0x100000,
            bitmap_pages: 256,
            bitmap_bytes: 1 << 20,
            screen_width: 320,
            screen_height: 240,
            ..Default::default()
        })
        .unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine.register_user32_module(0x71000000).unwrap();
    let imports = [pe::Import {
        module: "kernel32.dll",
        symbols: vec![pe::Symbol::Name("GetLastError")],
    }];
    engine
        .register_guest_module(
            "resource-dialog.exe",
            &pe::image(false, 0x400000, &imports, &[]),
        )
        .unwrap();
    engine
        .start_module_process("resource-dialog.exe", None, Default::default())
        .unwrap();
    engine.load_library_module("user32.dll", None).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.map(DATA, 2, 3).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(CODE), &[0x0f, 0x0b, 0x90])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(CODE + 0x100), &[0xc2, 0x10, 0])
        .unwrap();
    engine
}

fn call(engine: &mut EngineInstance, id: u32, args: &[u32]) -> Result<(), HostError> {
    for (index, word) in [CODE, 2, CODE, id].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    let words = std::iter::once(CODE + 2)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(STACK), &words)
        .unwrap();
    let mut state = State32 {
        eip: CODE,
        ..Default::default()
    };
    state.registers[4] = STACK;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_call(
            KEY,
            engine.generation,
            CallingConvention32::Stdcall,
            args.len() as u32,
        )
        .unwrap()
        .token;
    engine.complete_windows_call(KEY, engine.generation, token)
}

#[test]
fn captured_modeless_and_modal_template_calls_begin_real_dialog_window_creation() {
    for api in [0x0002_0069, 0x0002_006d] {
        let mut engine = engine();
        let template: Vec<u8> = [0u16, 0x8000, 0, 0, 0, 0, 0, 100, 50, 0, 0, 0x0044, 0]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(DATA), &template)
            .unwrap();
        assert_eq!(
            call(
                &mut engine,
                api,
                &[0x400000, DATA, 0, CODE + 0x100, 0x12345678]
            ),
            Ok(())
        );
        let request = engine
            .pending_windows_callback()
            .expect("window creation must run its real callback");
        assert_eq!(request.count, 4);
        assert_eq!(request.arguments[1], 0x81);
        let window = WindowHandle32::try_from(request.arguments[0]).unwrap();
        let context = engine.user32.as_ref().unwrap();
        assert!(context.windows().get(window).is_ok());
        let record = context.state.record(window).unwrap();
        assert!(context.state.is_system_class(record.class_atom));
        assert_eq!(
            context
                .state
                .class(record.instance, &[], Some(record.class_atom))
                .unwrap()
                .name,
            "#32770".encode_utf16().collect::<Vec<_>>()
        );
        assert!(engine.pending_call.is_some());
    }
}

mod metadata;

mod bridge;
