use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::CallingConvention32,
};

const KEY: u64 = 0x4d454e55;
const CODE: u32 = 0x70000;
const STACK: u32 = 0x11f00;

fn engine() -> EngineInstance {
    let (context, memory, _) = crate::process::windows::user32::tests::fixture();
    let mut engine = EngineInstance::new(16, KEY).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(CODE), &[0x0f, 0x0b])
        .unwrap();
    engine
}

fn call(engine: &mut EngineInstance, id: u32, args: &[u32]) -> Result<u32, HostError> {
    let descriptors: Vec<u8> = [CODE, 2, CODE, id]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + descriptors.len()]
        .copy_from_slice(&descriptors);
    engine.compile_with_gates(1, 1).unwrap();
    let words: Vec<u8> = std::iter::once(CODE + 2)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect();
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
    engine.complete_windows_call(KEY, engine.generation, token)?;
    assert!(engine.pending_call.is_none());
    Ok(decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0])
}

#[test]
fn captured_menu_tree_queries_and_remove_delete_use_actual_handles_and_lifetimes() {
    let mut engine = engine();
    let parent = call(&mut engine, 0x0002_0077, &[]).unwrap();
    let child = call(&mut engine, 0x0002_0078, &[]).unwrap();
    assert_ne!(parent, 0);
    assert_ne!(child, 0);
    assert_ne!(parent, child);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10500), &[b'X', 0, 0, 0])
        .unwrap();
    assert_eq!(
        call(&mut engine, 0x0002_0081, &[child, 8, 37, 0x10500]),
        Ok(1)
    );
    assert_eq!(
        call(&mut engine, 0x0002_0081, &[parent, 0x10, child, 0x10500]),
        Ok(1)
    );
    assert_eq!(call(&mut engine, 0x0002_007d, &[parent]), Ok(1));
    assert_eq!(call(&mut engine, 0x0002_007f, &[parent, 0]), Ok(child));
    assert_eq!(call(&mut engine, 0x0002_007e, &[parent, 0]), Ok(u32::MAX));
    assert_eq!(call(&mut engine, 0x0002_007e, &[child, 0]), Ok(37));
    assert_eq!(call(&mut engine, 0x0002_008b, &[child, 37, 0]), Ok(8));
    assert_eq!(call(&mut engine, 0x0002_0086, &[parent, 0, 0x400]), Ok(1));
    assert_eq!(call(&mut engine, 0x0002_007a, &[child]), Ok(1));
    assert_eq!(
        call(&mut engine, 0x0002_0081, &[parent, 0x10, child, 0x10500]),
        Ok(1)
    );
    assert_eq!(call(&mut engine, 0x0002_0087, &[parent, 0, 0x400]), Ok(1));
    assert_eq!(call(&mut engine, 0x0002_007a, &[child]), Ok(0));
    assert_eq!(call(&mut engine, 0x0002_007d, &[parent]), Ok(0));
    assert_eq!(call(&mut engine, 0x0002_0079, &[parent]), Ok(1));
    assert_eq!(call(&mut engine, 0x0002_007a, &[parent]), Ok(0));
}

#[test]
fn captured_accelerator_create_copy_and_destroy_preserve_literal_six_byte_entries() {
    let mut engine = engine();
    let entries = [0x41, 0xaa, 65, 0, 7, 0, 0, 0xbb, 0xe9, 0, 9, 0];
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10400), &entries)
        .unwrap();
    let table = call(&mut engine, 0x0002_00a7, &[0x10400, 2]).unwrap();
    assert_ne!(table, 0);
    assert_eq!(call(&mut engine, 0x0002_00a9, &[table, 0, 0]), Ok(2));
    assert_eq!(call(&mut engine, 0x0002_00a9, &[table, 0x10600, 1]), Ok(1));
    let mut copied = [0; 6];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x10600), &mut copied)
        .unwrap();
    assert_eq!(copied, [0x41, 0, 65, 0, 7, 0]);
    assert_eq!(call(&mut engine, 0x0002_00aa, &[table]), Ok(1));
    assert_eq!(call(&mut engine, 0x0002_00a9, &[table, 0, 0]), Ok(0));
    assert_eq!(call(&mut engine, 0x0002_00aa, &[table]), Ok(0));
}

#[path = "tests/fields.rs"]
mod fields;
#[path = "tests/lifetime.rs"]
mod lifetime;
