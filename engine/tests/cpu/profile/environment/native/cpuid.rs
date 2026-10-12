use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        x86::{
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn decode(bytes: &[u8]) -> Result<Operation, DecodeError> {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let instruction = decode_one(&memory, GuestAddress(0x1000))?;
    assert_eq!(usize::from(instruction.length()), bytes.len());
    Ok(*instruction.operation())
}

#[test]
fn cpuid_keeps_full_register_operation_and_explicit_prefix_refusals() {
    for bytes in [
        &[0x0f, 0xa2][..],
        &[0x66, 0x0f, 0xa2][..],
        &[0x64, 0x67, 0x0f, 0xa2][..],
    ] {
        assert_eq!(decode(bytes), Ok(Operation::CpuId), "{bytes:02x?}");
    }
    assert_eq!(
        decode(&[0xf0, 0x0f, 0xa2]),
        Err(DecodeError::InvalidEncoding)
    );
    for bytes in [&[0x66, 0x66, 0x0f, 0xa2][..], &[0xf3, 0x0f, 0xa2][..]] {
        assert_eq!(
            decode(bytes),
            Err(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn native_cpuid_modules_report_only_the_proven_cpu_profile() {
    let mut modules = Vec::new();
    for resident in [false, true] {
        for entries in [false, true] {
            for instruction in [&[0x0f, 0xa2][..], &[0x66, 0x0f, 0xa2][..]] {
                let mut code = instruction.to_vec();
                code.extend([0xeb, 0]);
                let mut engine = EngineInstance::new(1, 0x4350_5549).unwrap();
                engine.map(0x1000, 1, 7).unwrap();
                engine.arena_mut().unwrap()[140..140 + code.len()].copy_from_slice(&code);
                engine.upload(0x1000, code.len() as u32).unwrap();
                engine.protect(0x1000, 1, 4).unwrap();
                engine.arena_mut().unwrap()[140..148].copy_from_slice(
                    &[0x1000_u32.to_le_bytes(), (code.len() as u32).to_le_bytes()].concat(),
                );
                let id = if resident {
                    Some(if entries {
                        engine.compile_resident_entries(1, 0).unwrap()
                    } else {
                        engine.compile_resident(1).unwrap()
                    })
                } else {
                    if entries {
                        engine.compile_entries(1, 0).unwrap();
                    } else {
                        engine.compile(1).unwrap();
                    }
                    None
                };
                let bytes = match id {
                    Some(id) => engine.resident_bytes(id.get()).unwrap(),
                    None => engine.artifact_bytes().unwrap(),
                };
                modules.extend([
                    u8::from(resident),
                    u8::from(entries),
                    instruction.len() as u8,
                ]);
                modules.extend((bytes.len() as u32).to_le_bytes());
                modules.extend(bytes);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut child = Command::new("node")
        .arg(root.join("engine/tests/cpu/profile/environment/fixtures/p2-cpuid/runtime/native.mjs"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&modules).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "CPUID codegen failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
