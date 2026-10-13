use ring3_engine::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit, encode_state_v2},
    },
    cpu::{
        ExecutionExit, ExitReason,
        dbt::{CompileError, RegistryError},
        x86::{State32, decode::decode_one},
    },
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};
const KEY: u64 = 0x4341_5041_4349_5459;
const ENTRY: u32 = 0x3000;
fn upload(e: &mut EngineInstance, address: u32, bytes: &[u8]) {
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()].copy_from_slice(bytes);
    e.upload(address, bytes.len() as u32).unwrap();
}
fn describe(e: &mut EngineInstance, address: u32, length: usize, entries: bool) {
    let t = &mut e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    t[..4].copy_from_slice(&address.to_le_bytes());
    t[4..].copy_from_slice(&(if entries { 0 } else { length as u32 }).to_le_bytes());
}
fn ram(e: &EngineInstance) -> Vec<u8> {
    let mut output = vec![0; 3 * 4096];
    for (i, address) in [0x1000, ENTRY, 0x5000].into_iter().enumerate() {
        let target = &mut output[i * 4096..(i + 1) * 4096];
        if address == 0x5000 {
            e.memory()
                .unwrap()
                .read(GuestAddress(address), target)
                .unwrap();
        } else {
            e.memory()
                .unwrap()
                .fetch(GuestAddress(address), target)
                .unwrap();
        }
    }
    output
}
fn stop(e: &mut EngineInstance) {
    let a = e.arena_mut().unwrap();
    let mut registers = [0; 8];
    registers[3] = 0x5000;
    encode_state_v2(
        &State32 {
            registers,
            eip: ENTRY,
            eflags: 2,
        },
        &mut a[..STATE_SIZE],
    )
    .unwrap();
    encode_exit(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::NeedCode,
        },
        &mut a[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}
#[test]
fn automatic_prefixes_progress_and_explicit_units_keep_their_limits() {
    let mut rows = 0;
    let mut failures = 0;
    for (label, opcode, repeats, expected_error) in [
        ("word_read_short", &[0x66, 0x03, 0x03][..], 1, None),
        (
            "word_read_46",
            &[0x66, 0x03, 0x03][..],
            46,
            Some(CompileError::WasmLimit),
        ),
        ("nop_63", &[0x90][..], 63, None),
        (
            "nop_64",
            &[0x90][..],
            64,
            Some(CompileError::InstructionLimit),
        ),
    ] {
        let mut code = opcode.repeat(repeats);
        code.extend([0xeb, 0]);
        for mode in [
            "extent",
            "entry",
            "resident_extent",
            "resident_entry",
            "stopped",
            "resident_stopped",
        ] {
            let mut e = EngineInstance::new(3, KEY).unwrap();
            for address in [0x1000, ENTRY] {
                e.map(address, 1, 7).unwrap();
            }
            e.map(0x5000, 1, 3).unwrap();
            upload(&mut e, 0x1000, &[0xe9, 0xfb, 0x1f, 0, 0]);
            upload(&mut e, ENTRY, &code);
            upload(&mut e, 0x5000, &[7, 0]);
            for address in [0x1000, ENTRY] {
                e.protect(address, 1, 4).unwrap();
            }
            // each selected encoding and the real terminal are decoded independently.
            let mut offset = 0;
            for _ in 0..repeats + 1 {
                let decoded =
                    decode_one(e.memory().unwrap(), GuestAddress(ENTRY + offset)).unwrap();
                offset += u32::from(decoded.length());
            }
            assert_eq!(offset, code.len() as u32);
            describe(&mut e, 0x1000, 5, false);
            e.compile(1).unwrap();
            let keep = e.compile_resident(1).unwrap();
            let generation = e.generation();
            let replacement = e.artifact_bytes().unwrap().to_vec();
            let resident = e.resident_bytes(keep.get()).unwrap().to_vec();
            if mode.ends_with("stopped") {
                stop(&mut e);
            } else {
                describe(&mut e, ENTRY, code.len(), mode.ends_with("entry"));
            }
            let arena = e.arena().to_vec();
            let memory = ram(&e);
            let result = match mode {
                "extent" => e.compile(1).map(u64::from),
                "entry" => e.compile_entries(1, 0).map(u64::from),
                "resident_extent" => e.compile_resident(1).map(|id| id.get()),
                "resident_entry" => e.compile_resident_entries(1, 0).map(|id| id.get()),
                "stopped" => e.compile_stopped_entry(KEY, generation).map(u64::from),
                "resident_stopped" => e.compile_stopped_resident_entry(KEY).map(|id| id.get()),
                _ => unreachable!(),
            };
            assert_eq!(e.arena(), arena);
            assert_eq!(ram(&e), memory);
            let byte_length = if let Ok(id) = result {
                if mode.starts_with("resident") {
                    e.resident_bytes(id).unwrap().len()
                } else {
                    e.artifact_bytes().unwrap().len()
                }
            } else {
                0
            };
            if result.is_ok() {
                assert!(byte_length > 8 && byte_length <= 65_536);
            }
            let expected = expected_error
                .filter(|_| mode.ends_with("extent"))
                .map(|error| {
                    if mode.starts_with("resident") {
                        HostError::Resident(RegistryError::Compile(error))
                    } else {
                        HostError::Compile(error)
                    }
                });
            assert_eq!(result.as_ref().err().copied(), expected);
            if expected.is_some() {
                failures += 1;
                assert_eq!(e.generation(), generation);
                assert_eq!(e.artifact_bytes().unwrap(), replacement);
                assert_eq!(e.resident_bytes(keep.get()).unwrap(), resident);
                assert_eq!(e.lookup_resident(0x1000).unwrap(), keep);
                assert!(e.lookup_resident(ENTRY).is_err());
                e.guard(KEY, generation).unwrap();
                e.guard_resident(KEY, keep.get()).unwrap();
            }
            rows += 1;
            println!(
                "row|{}|{}|guest_bytes={}|instructions={}|result={:?}|emitted_bytes={}|arena_ram_neutral=true",
                label,
                mode,
                code.len(),
                repeats + 1,
                result,
                byte_length
            );
        }
    }
    println!(
        "summary|rows={}|observed_refusals={}|controls={}|bootstrap_compiles={}|guest_execution=none",
        rows,
        failures,
        rows - failures,
        2 * rows
    );
}
