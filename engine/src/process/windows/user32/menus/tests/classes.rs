use super::*;
use crate::{
    memory::PageRange,
    process::windows::user32::{User32CallContext32, memory},
    windows::user32::{User32Api32 as Api, User32Limits32, menus::MenuHandle32},
};

#[path = "../../../../../../tests/loader/modules/linking/native/image.rs"]
mod pe;

fn fixture(named: bool, missing: bool) -> (EngineInstance, u32, User32CallContext32) {
    let imports = [pe::Import {
        module: "kernel32.dll",
        symbols: vec![pe::Symbol::Name("GetLastError")],
    }];
    let mut image = pe::image(false, 0x400000, &imports, &[]);
    for (offset, value) in [(0x108, 0x2c00u32), (0x10c, 0x200)] {
        image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let r = &mut image[0x1000..0x1200];
    for (offset, value) in [
        (14, 1u16),
        (0x20 + if named { 12 } else { 14 }, 1),
        (0x40 + 14, 1),
    ] {
        r[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [
        (0x10, 4u32),
        (0x14, 0x80000020),
        (0x30, if named { 0x80000080 } else { 37 }),
        (0x34, 0x80000040),
        (0x50, 0x409),
        (0x54, 0x60),
        (0x60, 0x2d00),
        (0x64, 12),
    ] {
        r[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    r[0x80..0x8a].copy_from_slice(&[4, 0, b'M', 0, b'E', 0, b'N', 0, b'U', 0]);
    let template: Vec<u8> = [0u16, 0, 0x80, 37, 88, 0]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    r[0x100..0x10c].copy_from_slice(&template);
    let mut engine = EngineInstance::new(128, KEY).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine
        .configure_user32(
            User32Limits32 {
                classes: 4,
                windows: 8,
                messages: 8,
                continuations: 16,
            },
            PageRange::new(GuestAddress(0x30000), 16).unwrap(),
        )
        .unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine.register_user32_module(0x71000000).unwrap();
    engine
        .register_guest_module("class-menu.exe", &image)
        .unwrap();
    engine
        .start_module_process("class-menu.exe", None, Default::default())
        .unwrap();
    engine.load_library_module("user32.dll", None).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.map(0x10000, 2, 3).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(CODE), &[0x0f, 0x0b])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10100), b"ResourceClass\0")
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10200), b"MENU\0")
        .unwrap();
    let mut words = [0u32; 10];
    words[1] = CODE;
    words[4] = 0x400000;
    words[8] = if missing {
        99
    } else if named {
        0x10200
    } else {
        37
    };
    words[9] = 0x10100;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x10000),
            &words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let atom = call(&mut engine, Api::RegisterClassA.id(), &[0x10000]).unwrap();
    let call = engine.user32_call_context().unwrap();
    (engine, atom, call)
}

fn begin(engine: &mut EngineInstance, atom: u32, parent: u32, menu: u32, style: u32) {
    let args = [0, atom, 0, style, 0, 0, 100, 80, parent, menu, 0x400000, 0];
    let descriptors: Vec<u8> = [CODE, 2, CODE, Api::CreateWindowExA.id()]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + descriptors.len()]
        .copy_from_slice(&descriptors);
    engine.compile_with_gates(1, 1).unwrap();
    let words = std::iter::once(CODE + 2)
        .chain(args)
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
            reason: ExitReason::Gate {
                id: Api::CreateWindowExA.id(),
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_windows_call(KEY, engine.generation)
        .unwrap()
        .token;
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
}

#[test]
fn captured_creation_loads_named_or_integer_class_menu_before_the_real_nonclient_create() {
    for named in [false, true] {
        let (mut engine, atom, _) = fixture(named, false);
        begin(&mut engine, atom, 0, 0, 0xcf0000);
        let callback = engine
            .pending_windows_callback()
            .expect("real creation callback");
        assert_eq!(callback.arguments[1], 0x81);
        let words =
            memory::words::<12>(engine.memory.as_ref().unwrap(), callback.arguments[3]).unwrap();
        let menu = MenuHandle32::try_from(words[2])
            .expect("class resource must be loaded before WM_NCCREATE");
        let context = engine.user32.as_ref().unwrap();
        assert_eq!(context.menus.count(menu), Ok(1));
        assert_eq!(context.menus.item(menu, 0, true).unwrap().id, 37);
        assert_eq!(context.menus.item(menu, 0, true).unwrap().text, [88]);
        let window =
            crate::windows::window::WindowHandle32::try_from(callback.arguments[0]).unwrap();
        assert_eq!(context.state.record(window).unwrap().menu, menu.get());
        assert!(engine.pending_call.is_some());
    }
}

#[test]
fn explicit_menu_and_child_identifier_do_not_fetch_a_missing_class_resource() {
    for child in [false, true] {
        let (mut engine, atom, call_context) = fixture(false, true);
        let (parent, menu) = if child {
            let context = engine.user32.as_mut().unwrap();
            let plain = crate::process::windows::user32::tests::register(
                context,
                engine.memory.as_mut().unwrap(),
                call_context,
            );
            let parent = crate::process::windows::user32::tests::create(
                context,
                engine.memory.as_mut().unwrap(),
                call_context,
                plain,
            );
            (parent, 37)
        } else {
            (0, call(&mut engine, Api::CreateMenu.id(), &[]).unwrap())
        };
        begin(
            &mut engine,
            atom,
            parent,
            menu,
            if child { 0x40000000 } else { 0xcf0000 },
        );
        let callback = engine
            .pending_windows_callback()
            .expect("explicit menu and child IDs bypass class resource");
        assert_eq!(
            memory::words::<12>(engine.memory.as_ref().unwrap(), callback.arguments[3]).unwrap()[2],
            menu
        );
    }
}
