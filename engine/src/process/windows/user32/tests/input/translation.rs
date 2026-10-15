use super::*;
use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    input::{InputLimits, TimedInputEvent, keyboard::KeyboardLayout32},
    memory::Permissions,
    process::{EngineInstance, MediaConfig},
};

const ENGINE_KEY: u64 = 46;
const CODE: u32 = 0x70000;
const STACK: u32 = 0x11e00;

fn engine() -> (EngineInstance, u32) {
    let (mut context, mut memory, mut call) = super::super::fixture();
    call.thread_id = 1;
    let atom = super::super::register(&mut context, &mut memory, call);
    let window = super::super::create(&mut context, &mut memory, call, atom);
    context
        .state
        .windows_mut()
        .set_focus(1, Some(WindowHandle32::try_from(window).unwrap()))
        .unwrap();
    context.input.focus = window;
    context.input.point = Point32 { x: 12, y: 24 };
    context.input.query_point = context.input.point;
    memory.write(GuestAddress(0x10180), b"0000041F\0").unwrap();
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::LoadKeyboardLayoutA,
        &[0x10180, 0],
    );
    for index in 0..8 {
        context
            .state
            .post(crate::windows::user32::Message32 {
                window,
                message: 0x400,
                wparam: index,
                thread_id: 1,
                ..Default::default()
            })
            .unwrap();
    }
    memory
        .map_zeroed(
            crate::memory::PageRange::new(GuestAddress(CODE), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(CODE), &[0x0f, 0x0b]).unwrap();
    let mut engine = EngineInstance::new(16, ENGINE_KEY).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    engine
        .configure_media(MediaConfig {
            input: InputLimits {
                queue_events: 1024,
                batch_events: 16,
            },
            ..Default::default()
        })
        .unwrap();
    engine.observe_cpu_clock(1_000_000).unwrap();
    (engine, window)
}

fn captured(engine: &mut EngineInstance, api: User32Api32, args: &[u32]) -> u32 {
    for (index, word) in [CODE, 2, CODE, api.id()].iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    for (index, word) in std::iter::once(0x70100)
        .chain(args.iter().copied())
        .enumerate()
    {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(STACK + index as u32 * 4), &word.to_le_bytes())
            .unwrap();
    }
    let mut state = State32 {
        eip: CODE,
        ..Default::default()
    };
    state.registers[4] = STACK;
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
        .capture_windows_call(ENGINE_KEY, engine.generation)
        .unwrap()
        .token;
    engine
        .complete_windows_call(ENGINE_KEY, engine.generation, token)
        .unwrap();
    assert!(engine.pending_call.is_none());
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}

fn submit(engine: &mut EngineInstance, sequence: u64, kinds: &[InputKind]) {
    let epoch = engine.media_epoch().unwrap();
    let events = kinds
        .iter()
        .copied()
        .map(|kind| TimedInputEvent {
            timestamp_ns: 1_000_000,
            kind,
        })
        .collect::<Vec<_>>();
    engine.submit_input(epoch, sequence, &events).unwrap();
}
fn peek(
    engine: &mut EngineInstance,
    window: u32,
    message: u32,
) -> crate::windows::user32::Message32 {
    assert_eq!(
        captured(
            engine,
            User32Api32::PeekMessageW,
            &[0x10200, window, message, message, 1]
        ),
        1
    );
    super::super::super::memory::message(engine.memory.as_ref().unwrap(), 0x10200).unwrap()
}

#[test]
fn accepted_layout_switch_preserves_old_scan_mapping_behind_full_queue_then_applies_new_policy() {
    let (mut engine, window) = engine();
    submit(
        &mut engine,
        1,
        &[
            InputKind::Focus { focused: true },
            InputKind::Key {
                code: 0x0c,
                pressed: true,
            },
        ],
    );
    assert!(engine.pending_user32_input());
    assert_eq!(
        captured(
            &mut engine,
            User32Api32::ActivateKeyboardLayout,
            &[KeyboardLayout32::TurkishQ.handle(), 0]
        ),
        KeyboardLayout32::Us.handle()
    );
    assert!(engine.pending_user32_input());
    assert_eq!(
        engine.user32.as_ref().unwrap().input.layout,
        KeyboardLayout32::TurkishQ
    );
    assert_eq!(
        engine.user32.as_ref().unwrap().input.delivery_layout,
        KeyboardLayout32::Us
    );
    peek(&mut engine, window, 0x400);
    assert_eq!(peek(&mut engine, window, 0x100).wparam, 0xbd);
    assert!(!engine.pending_user32_input());
    submit(
        &mut engine,
        2,
        &[
            InputKind::Key {
                code: 0x0c,
                pressed: false,
            },
            InputKind::Key {
                code: 0x0c,
                pressed: true,
            },
        ],
    );
    peek(&mut engine, window, 0x101);
    assert_eq!(peek(&mut engine, window, 0x100).wparam, 0xdf);
    assert_eq!(captured(&mut engine, User32Api32::GetKeyState, &[0xbd]), 0);
}

#[test]
fn accepted_button_swap_preserves_old_left_message_and_maps_later_physical_left_to_right() {
    let (mut engine, window) = engine();
    submit(
        &mut engine,
        1,
        &[
            InputKind::Focus { focused: true },
            InputKind::Button {
                button: 0,
                pressed: true,
            },
        ],
    );
    assert!(engine.pending_user32_input());
    assert_eq!(captured(&mut engine, User32Api32::SwapMouseButton, &[1]), 0);
    assert!(engine.user32.as_ref().unwrap().input.swapped);
    assert!(!engine.user32.as_ref().unwrap().input.delivery_swapped);
    peek(&mut engine, window, 0x400);
    assert_eq!(peek(&mut engine, window, 0x201).wparam & 3, 1);
    assert!(!engine.pending_user32_input());
    submit(
        &mut engine,
        2,
        &[
            InputKind::Button {
                button: 0,
                pressed: false,
            },
            InputKind::Button {
                button: 0,
                pressed: true,
            },
        ],
    );
    peek(&mut engine, window, 0x205);
    assert_eq!(peek(&mut engine, window, 0x204).wparam & 3, 2);
}
