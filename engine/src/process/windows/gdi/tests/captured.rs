use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::GuestAddress,
    process::EngineInstance,
    windows::{CallingConvention32, gdi::GdiApi32},
};
fn call(engine: &mut EngineInstance, api: GdiApi32, args: &[u32]) -> u32 {
    call_id(engine, api.id(), args)
}
pub(in crate::process::windows) fn call_id(
    engine: &mut EngineInstance,
    id: u32,
    args: &[u32],
) -> u32 {
    for (index, word) in [0x10000u32, 2, 0x10000, id].iter().enumerate() {
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
            reason: ExitReason::Gate { id },
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
fn captured_gdi_calls_share_checked_gate_dispatch_and_real_object_lifecycle() {
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
    engine.gdi = Some(
        super::super::GdiRuntime32::new(super::super::GdiConfig32 {
            bitmap_base: 0x20000,
            bitmap_pages: 8,
            bitmap_bytes: 65_536,
            screen_width: 8,
            screen_height: 8,
            ..Default::default()
        })
        .unwrap(),
    );
    let dc = call(&mut engine, GdiApi32::CreateCompatibleDC, &[0]);
    assert_ne!(dc, 0);
    let brush = call(&mut engine, GdiApi32::CreateSolidBrush, &[0x00332211]);
    assert_ne!(brush, 0);
    let previous = call(&mut engine, GdiApi32::SelectObject, &[dc, brush]);
    assert_ne!(previous, 0);
    assert_eq!(call(&mut engine, GdiApi32::DeleteObject, &[brush]), 0);
    assert_eq!(
        call(&mut engine, GdiApi32::SelectObject, &[dc, previous]),
        brush
    );
    assert_eq!(call(&mut engine, GdiApi32::DeleteObject, &[brush]), 1);
    assert_eq!(call(&mut engine, GdiApi32::GetObjectType, &[brush]), 0);
    assert_eq!(call(&mut engine, GdiApi32::DeleteDC, &[dc]), 1);
    assert_eq!(call(&mut engine, GdiApi32::SaveDC, &[dc]), 0);
}
