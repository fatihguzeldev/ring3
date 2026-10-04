use ring3_engine::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{decode_state, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    process::EngineInstance,
    windows::CallingConvention32,
};

#[allow(dead_code)]
#[path = "support/pe32.rs"]
mod pe;

#[test]
fn numeric_null_module_handle_completion_returns_the_privately_loaded_image_base() {
    const KEY: u64 = 0xe353_cdef_1234_5678;
    const GATE: u32 = 0x7000_0000;
    const ID: u32 = 0x0001_0004;
    const ESP: u32 = 0x8080;
    let return_pc = pe::BASE + 0x1000;
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    let image = engine.load_pe32(&pe::image()).unwrap();
    assert_eq!(image.image_base, pe::BASE);
    assert_eq!(image.mapped_pages, pe::MAPPED_PAGES);

    engine.map(GATE, 1, 3).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0x0f, 0x0b]);
    engine.upload(GATE, 2).unwrap();
    engine.protect(GATE, 1, 5).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    for (index, word) in [GATE, 2, GATE, ID].into_iter().enumerate() {
        let offset = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    engine.write32(ESP, return_pc).unwrap();
    engine.write32(ESP + 4, 0).unwrap();

    // native saved-stop input; actual call execution belongs to the wasm proof.
    let state = State32 {
        registers: [
            0x1357_9bdf,
            0x2468_ace0,
            0x3456_789a,
            0x4567_89ab,
            ESP,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: GATE,
        eflags: 0xcd7,
    };
    {
        let arena = engine.arena_mut().unwrap();
        encode_state(&state, &mut arena[..56]).unwrap();
        encode_exit_v3(
            &ExecutionExit {
                retired: 3,
                reason: ExitReason::Gate { id: ID },
            },
            &mut arena[EXIT_OFFSET..EXIT_OFFSET + 40],
        )
        .unwrap();
    }
    let call = engine
        .capture_call(KEY, generation, CallingConvention32::Stdcall, 1)
        .unwrap();
    assert_eq!(call.id, ID);
    assert_eq!(call.arguments[0], 0);
    assert_eq!(call.return_pc, return_pc);
    let mut expected = engine.arena().to_vec();
    let mut returned = state;
    returned.registers[0] = image.image_base;
    returned.registers[4] = ESP + 8;
    returned.eip = return_pc;
    encode_state(&returned, &mut expected[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode,
        },
        &mut expected[EXIT_OFFSET..EXIT_OFFSET + 40],
    )
    .unwrap();

    assert_eq!(
        engine.complete_windows_call(KEY, generation, call.token),
        Ok(())
    );
    assert_eq!(decode_state(&engine.arena()[..56]).unwrap(), returned);
    assert_eq!(engine.arena(), expected);
}

use ring3_engine::{
    abi::arena::CANCEL_OFFSET,
    memory::GuestAddress,
    process::{CallError, HostError},
    windows::WindowsApi32,
};

const KEY: u64 = 0xe353_cdef_9876_5432;
const GATE: u32 = 0x7000_0000;
const ESP: u32 = 0x8080;
const GET: u32 = 0x0001_0001;
const SET: u32 = 0x0001_0002;
const MODULE: u32 = 0x0001_0004;
const LAST_ERROR: u32 = 0x89ab_cdef;

#[derive(Clone, Copy)]
enum Owner {
    Replacement,
    Resident,
}

struct Fixture {
    engine: EngineInstance,
    generation: u32,
    resident: u64,
    base: u32,
}

fn source_image() -> Vec<u8> {
    let mut bytes = pe::image();
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::OPTIONAL + 96 + 5 * 8, 0x31e0);
    pe::put32(&mut bytes, pe::OPTIONAL + 100 + 5 * 8, 12);
    pe::put32(&mut bytes, pe::DATA_RAW, pe::BASE);
    pe::put32(&mut bytes, pe::DATA_RAW + 0x1e0, 0x3000);
    pe::put32(&mut bytes, pe::DATA_RAW + 0x1e4, 12);
    pe::put16(&mut bytes, pe::DATA_RAW + 0x1e8, 0x3000);
    pe::put16(&mut bytes, pe::DATA_RAW + 0x1ea, 0);
    bytes
}

fn fixture(base: Option<u32>) -> Fixture {
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    if let Some(base) = base {
        let image = engine.load_pe32_at(&source_image(), base).unwrap();
        assert_eq!(image.image_base, base);
    }
    engine.map(GATE, 1, 3).unwrap();
    for offset in [0, 16, 32, 48] {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0x0f, 0x0b]);
        engine.upload(GATE + offset, 2).unwrap();
    }
    engine.protect(GATE, 1, 5).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    let words: Vec<u32> = (0..4)
        .flat_map(|index| [GATE + index * 16, 2])
        .chain((0..4).flat_map(|index| [GATE + index * 16, GET + index]))
        .collect();
    for (index, word) in words.iter().enumerate() {
        let at = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    let resident = engine.compile_resident_with_gates(4, 4).unwrap().get();
    let generation = engine.compile_with_gates(4, 4).unwrap();
    Fixture {
        engine,
        generation,
        resident,
        base: base.unwrap_or(pe::BASE),
    }
}

fn saved_call(
    f: &mut Fixture,
    owner: Owner,
    id: u32,
    convention: CallingConvention32,
    words: u32,
    argument: u32,
) -> (u32, State32) {
    f.engine.write32(ESP, f.base + 0x1000).unwrap();
    f.engine.write32(ESP + 4, argument).unwrap();
    let state = State32 {
        registers: [
            0x1357_9bdf,
            0x2468_ace0,
            0x3456_789a,
            0x4567_89ab,
            ESP,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: GATE + (id - GET) * 16,
        eflags: 0xcd7,
    };
    let arena = f.engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 7,
            reason: ExitReason::Gate { id },
        },
        &mut arena[56..96],
    )
    .unwrap();
    let call = match owner {
        Owner::Replacement => f.engine.capture_call(KEY, f.generation, convention, words),
        Owner::Resident => f
            .engine
            .capture_resident_call(KEY, f.resident, convention, words),
    }
    .unwrap();
    (call.token, state)
}

fn complete(f: &mut Fixture, owner: Owner, token: u32) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => f.engine.complete_windows_call(KEY, f.generation, token),
        Owner::Resident => f
            .engine
            .complete_resident_windows_call(KEY, f.resident, token),
    }
}

fn scalar(f: &mut Fixture, owner: Owner, token: u32, value: u32) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => f.engine.complete_call(KEY, f.generation, token, value),
        Owner::Resident => f
            .engine
            .complete_resident_call(KEY, f.resident, token, value),
    }
}

fn returned(
    f: &mut Fixture,
    owner: Owner,
    token: u32,
    mut state: State32,
    value: u32,
    cleanup: u32,
) {
    let mut expected = f.engine.arena().to_vec();
    state.registers[0] = value;
    state.registers[4] = state.registers[4].wrapping_add(cleanup);
    state.eip = f.base + 0x1000;
    encode_state(&state, &mut expected[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode,
        },
        &mut expected[56..96],
    )
    .unwrap();
    complete(f, owner, token).unwrap();
    assert_eq!(f.engine.arena(), expected);
}

fn last_error(f: &mut Fixture, owner: Owner, value: u32) {
    let (token, state) = saved_call(f, owner, GET, CallingConvention32::Stdcall, 0, 0);
    returned(f, owner, token, state, value, 4);
}

fn seed_error(f: &mut Fixture, owner: Owner) {
    let (token, state) = saved_call(f, owner, SET, CallingConvention32::Stdcall, 1, LAST_ERROR);
    returned(f, owner, token, state, state.registers[0], 8);
}

fn reject(
    f: &mut Fixture,
    expected: HostError,
    action: impl FnOnce(&mut Fixture) -> Result<(), HostError>,
) {
    let arena = f.engine.arena().to_vec();
    let mut stack = [0; 4096];
    f.engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x8000), &mut stack)
        .unwrap();
    assert_eq!(action(f), Err(expected));
    assert_eq!(f.engine.arena(), arena);
    let mut after = [0; 4096];
    f.engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x8000), &mut after)
        .unwrap();
    assert_eq!(after, stack);
}

#[test]
fn closed_name_id_and_stdcall_shape_have_one_new_entry() {
    for module in ["kernel32.dll", "KERNEL32.DLL", "KeRnEl32.dLl"] {
        assert_eq!(
            WindowsApi32::resolve(module, "GetModuleHandleA"),
            Some(WindowsApi32::GetModuleHandleA)
        );
    }
    let api = WindowsApi32::GetModuleHandleA;
    assert_eq!(WindowsApi32::from_id(MODULE), Some(api));
    assert_eq!(api.id(), MODULE);
    assert_eq!(api.convention(), CallingConvention32::Stdcall);
    assert_eq!(api.stack_words(), 1);
    for name in [
        "GetModuleHandleW",
        "getmodulehandlea",
        "GetModuleHandle",
        "GetModuleHandleA\0",
    ] {
        assert_eq!(WindowsApi32::resolve("kernel32.dll", name), None);
    }
    for module in ["kernel32", "kernel32.dll ", "ntdll.dll", "./kernel32.dll"] {
        assert_eq!(WindowsApi32::resolve(module, "GetModuleHandleA"), None);
    }
    assert_eq!(WindowsApi32::from_id(MODULE + 2), None);
}

#[test]
fn both_owners_return_selected_base_and_preserve_distinct_last_error_and_full_arena() {
    for base in [pe::BASE, 0x0050_0000] {
        for owner in [Owner::Replacement, Owner::Resident] {
            let mut f = fixture(Some(base));
            seed_error(&mut f, owner);
            let (token, state) =
                saved_call(&mut f, owner, MODULE, CallingConvention32::Stdcall, 1, 0);
            returned(&mut f, owner, token, state, base, 8);
            last_error(&mut f, owner, LAST_ERROR);
        }
    }
}

#[test]
fn no_image_non_null_and_wrong_shapes_keep_pending_and_allow_old_scalar_recovery() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for base in [None, Some(pe::BASE)] {
            for (convention, count, arg) in [
                (CallingConvention32::Stdcall, 1, 0),
                (CallingConvention32::Stdcall, 1, 1),
                (CallingConvention32::Stdcall, 1, u32::MAX),
                (CallingConvention32::Stdcall, 1, pe::BASE),
                (CallingConvention32::Stdcall, 0, 0),
                (CallingConvention32::Stdcall, 2, 0),
                (CallingConvention32::Cdecl, 1, 0),
            ] {
                if base.is_some()
                    && convention == CallingConvention32::Stdcall
                    && count == 1
                    && arg == 0
                {
                    continue;
                }
                let mut f = fixture(base);
                seed_error(&mut f, owner);
                let (token, _) = saved_call(&mut f, owner, MODULE, convention, count, arg);
                reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
                    complete(f, owner, token)
                });
                scalar(&mut f, owner, token, 0xfeed_beef).unwrap();
                last_error(&mut f, owner, LAST_ERROR);
            }
        }
    }
}

#[test]
fn saved_argument_return_and_private_image_ignore_transfer_and_live_stack_tampering() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(Some(0x0050_0000));
        seed_error(&mut f, owner);
        let (token, state) = saved_call(&mut f, owner, MODULE, CallingConvention32::Stdcall, 1, 0);
        f.engine.write32(ESP, 0xdead_beef).unwrap();
        f.engine.write32(ESP + 4, u32::MAX).unwrap();
        f.engine.arena_mut().unwrap()[TRANSFER_OFFSET..].fill(0xa5);
        returned(&mut f, owner, token, state, 0x0050_0000, 8);
        last_error(&mut f, owner, LAST_ERROR);
        let (token, _) = saved_call(
            &mut f,
            owner,
            MODULE,
            CallingConvention32::Stdcall,
            1,
            u32::MAX,
        );
        f.engine.write32(ESP + 4, 0).unwrap();
        f.engine.arena_mut().unwrap()[TRANSFER_OFFSET + 48..TRANSFER_OFFSET + 52].fill(0);
        reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
            complete(f, owner, token)
        });
    }
}

#[test]
fn owner_token_state_cancel_and_closed_priorities_precede_context_result() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(Some(pe::BASE));
        let (token, _) = saved_call(&mut f, owner, MODULE, CallingConvention32::Stdcall, 1, 0);
        reject(&mut f, HostError::InvalidArtifact, |f| match owner {
            Owner::Replacement => {
                f.engine
                    .complete_windows_call(KEY ^ (1 << 40), f.generation, token)
            }
            Owner::Resident => {
                f.engine
                    .complete_resident_windows_call(KEY ^ (1 << 40), f.resident, token)
            }
        });
        reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
            complete(f, owner, token + 1)
        });
        reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
            complete(
                f,
                match owner {
                    Owner::Replacement => Owner::Resident,
                    Owner::Resident => Owner::Replacement,
                },
                token,
            )
        });
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
            .copy_from_slice(&1_u32.to_le_bytes());
        reject(&mut f, HostError::Call(CallError::Cancelled), |f| {
            complete(f, owner, token)
        });
        f.engine.arena_mut().unwrap()[0] ^= 1;
        reject(&mut f, HostError::Call(CallError::StateChanged), |f| {
            complete(f, owner, token)
        });
        f.engine.close();
        let arena = f.engine.arena().to_vec();
        assert_eq!(complete(&mut f, owner, token), Err(HostError::Closed));
        assert_eq!(f.engine.arena(), arena);
    }
}

#[test]
fn image_identity_survives_header_unmap_or_protect_and_instances_keep_their_own_base() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for unmap in [false, true] {
            let mut first = fixture(Some(pe::BASE));
            let mut second = fixture(Some(0x0050_0000));
            seed_error(&mut first, owner);
            if unmap {
                first.engine.unmap(pe::BASE, 1).unwrap();
            } else {
                first.engine.protect(pe::BASE, 1, 2).unwrap();
            }
            for f in [&mut first, &mut second] {
                let base = f.base;
                let (token, state) =
                    saved_call(f, owner, MODULE, CallingConvention32::Stdcall, 1, 0);
                returned(f, owner, token, state, base, 8);
            }
            last_error(&mut first, owner, LAST_ERROR);
            last_error(&mut second, owner, 0);
        }
    }
}
