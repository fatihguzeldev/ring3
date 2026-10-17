#[path = "../../../../tests/loader/modules/linking/native/image.rs"]
mod pe;
use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{GuestAddress, PageRange},
    process::{EngineInstance, HostError},
    windows::{CallingConvention32, user32::User32Limits32},
};
const KEY: u64 = 0x43444c47;
const CODE: u32 = 0x50000;
const DATA: u32 = 0x60000;
const STACK: u32 = DATA + 0x1f00;
pub(in crate::process::windows) fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(256, KEY).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine
        .configure_user32(
            User32Limits32 {
                classes: 8,
                windows: 32,
                messages: 32,
                continuations: 16,
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
    engine.configure_files(Default::default()).unwrap();
    engine.register_commdlg32_module(0x72000000).unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine.register_user32_module(0x71000000).unwrap();
    let imports = [pe::Import {
        module: "kernel32.dll",
        symbols: vec![pe::Symbol::Name("GetLastError")],
    }];
    engine
        .register_guest_module(
            "file-dialog.exe",
            &pe::image(false, 0x400000, &imports, &[]),
        )
        .unwrap();
    engine
        .start_module_process("file-dialog.exe", None, Default::default())
        .unwrap();
    engine.load_library_module("user32.dll", None).unwrap();
    engine.load_library_module("comdlg32.dll", None).unwrap();
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

pub(in crate::process::windows) fn call(
    engine: &mut EngineInstance,
    id: u32,
    args: &[u32],
) -> Result<(), HostError> {
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

fn returned(engine: &EngineInstance) -> u32 {
    crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}
#[test]
fn captured_invalid_structure_records_extended_error_without_entering_user32_modal_state() {
    let mut engine = engine();
    call(&mut engine, 0x00210002, &[DATA]).unwrap();
    assert_eq!(returned(&engine), 0);
    assert!(engine.pending_windows_callback().is_none());
    call(&mut engine, 0x00210005, &[]).unwrap();
    assert_eq!(returned(&engine), 1);
    assert_eq!(engine.comdlg.error(999), 0);
    assert_eq!(engine.user32.as_ref().unwrap().windows().live_windows(), 0);
}
#[test]
fn captured_file_title_uses_frozen_ansi_width_and_reports_required_units_before_any_write() {
    let mut engine = engine();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA), b"C:\\directory\\file.txt\0")
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA + 0x100), &[0xcc; 32])
        .unwrap();
    call(&mut engine, 0x00210006, &[DATA, DATA + 0x100, 2]).unwrap();
    assert_eq!(returned(&engine), 9);
    let mut bytes = [0; 9];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(DATA + 0x100), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0xcc; 9]);
    call(&mut engine, 0x00210006, &[DATA, DATA + 0x100, 9]).unwrap();
    assert_eq!(returned(&engine), 0);
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(DATA + 0x100), &mut bytes)
        .unwrap();
    assert_eq!(&bytes, b"file.txt\0");
}
