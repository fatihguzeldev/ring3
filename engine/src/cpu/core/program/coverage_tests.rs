use super::*;
use crate::memory::{PageRange, Permissions};

fn memory(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}

fn limits(instructions: usize) -> CompileOptions {
    CompileOptions {
        instructions,
        blocks: 8,
        ..CompileOptions::default()
    }
}

#[test]
fn emitted_coverage_includes_linear_interior_and_native_instruction_starts() {
    let memory = memory(&[0xb8, 1, 0, 0, 0, 0x40, 0xd9, 0x03, 0x0f, 0x0b]);
    let code = compile(&memory, GuestAddress(0x1000), limits(64)).unwrap();
    assert_eq!(
        code.instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1005, 0x1006, 0x1008]
    );
    assert_eq!(code.instruction_count(), 4);
    assert_eq!(code.entrypoints(), [GuestAddress(0x1000)]);
    assert_eq!(code.native_instructions().len(), 1);
    assert_eq!(code.native_instructions()[0].pc, 0x1006);
}

#[test]
fn instruction_and_frontier_limits_exclude_unemitted_instruction_starts() {
    let memory = memory(&[0x40, 0x41, 0x42, 0x43, 0x0f, 0x0b]);
    let limited = compile(&memory, GuestAddress(0x1000), limits(2)).unwrap();
    let frontier = compile_with_frontier(
        &memory,
        GuestAddress(0x1000),
        limits(64),
        &[],
        &[GuestAddress(0x1002)],
    )
    .unwrap();
    for code in [limited, frontier] {
        assert_eq!(code.instruction_pcs().collect::<Vec<_>>(), [0x1000, 0x1001]);
        assert_eq!(code.instruction_count(), 2);
    }
}

#[test]
fn wasm_byte_shortening_retains_only_the_final_emitted_pc_prefix() {
    let mut bytes = [0x83, 0x03, 1, 0xd3, 0x23, 0x66, 0x83, 0x03, 1].repeat(24);
    bytes.extend_from_slice(&[0x0f, 0x0b]);
    let memory = memory(&bytes);
    let mut expected = Vec::new();
    for index in 0..24 {
        expected.extend([0x1000 + index * 9, 0x1003 + index * 9, 0x1005 + index * 9]);
    }
    expected.push(0x1000 + 24 * 9);
    let full = compile(&memory, GuestAddress(0x1000), limits(128)).unwrap();
    assert_eq!(full.instruction_pcs().collect::<Vec<_>>(), expected);
    let minimum = compile(&memory, GuestAddress(0x1000), limits(1)).unwrap();
    let quota = minimum.wasm().len() + 1024;
    assert!(full.wasm().len() > quota);
    let shortened = compile(
        &memory,
        GuestAddress(0x1000),
        CompileOptions {
            wasm_bytes: quota,
            ..limits(128)
        },
    )
    .unwrap();
    assert!(shortened.wasm().len() <= quota);
    assert!(shortened.instruction_count() > 0 && shortened.instruction_count() < expected.len());
    assert_eq!(
        shortened.instruction_pcs().collect::<Vec<_>>(),
        expected[..shortened.instruction_count()]
    );
}
