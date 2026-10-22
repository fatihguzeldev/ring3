use super::*;
use crate::memory::{PageRange, Permissions};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(
            GuestAddress(0x1000),
            &[0xe8, 0x3b, 0, 0, 0, 0x49, 0x75, 0xf8, 0x0f, 0x0b],
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1040), &[0xe8, 0x3b, 0, 0, 0, 0xc3])
        .unwrap();
    memory.write(GuestAddress(0x1080), &[0x40, 0xc3]).unwrap();
    memory
}

fn options() -> CompileOptions {
    CompileOptions {
        instructions: 64,
        blocks: 8,
        ..CompileOptions::default()
    }
}

#[test]
fn repeated_formation_preserves_both_owners_original_roots() {
    let memory = memory();
    let a = compile(&memory, GuestAddress(0x1000), options()).unwrap();
    let b = compile(&memory, GuestAddress(0x1040), options()).unwrap();
    let c = compile(&memory, GuestAddress(0x1080), options()).unwrap();
    assert_eq!(a.formation_roots(), &[GuestAddress(0x1000)]);
    assert_eq!(b.formation_roots(), &[GuestAddress(0x1040)]);
    let roots: Vec<_> = a
        .formation_roots()
        .iter()
        .chain(b.formation_roots())
        .copied()
        .collect();
    let ab = CompiledCode::compile_transition(
        &memory,
        GuestAddress(0x1040),
        options(),
        &[],
        &[],
        &roots,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        ab.formation_roots(),
        &[GuestAddress(0x1000), GuestAddress(0x1040)]
    );
    let roots: Vec<_> = ab
        .formation_roots()
        .iter()
        .chain(c.formation_roots())
        .copied()
        .collect();
    let abc = CompiledCode::compile_transition(
        &memory,
        GuestAddress(0x1080),
        options(),
        &[],
        &[],
        &roots,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        abc.formation_roots(),
        &[
            GuestAddress(0x1000),
            GuestAddress(0x1040),
            GuestAddress(0x1080),
        ]
    );
    for entry in [0x1000, 0x1005, 0x1040, 0x1045, 0x1080] {
        assert!(abc.entrypoints().contains(&GuestAddress(entry)));
    }
    assert!(abc.is_current(&memory));
    assert!(a.is_current(&memory));
    assert!(b.is_current(&memory));
    assert!(c.is_current(&memory));
}

#[test]
fn an_actual_interior_target_is_kept_in_addition_to_original_roots() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1040), &[0x85, 0xc0, 0x74, 0, 0x40, 0xc3])
        .unwrap();
    let code = CompiledCode::compile_transition(
        &memory,
        GuestAddress(0x1044),
        options(),
        &[],
        &[],
        &[
            GuestAddress(0x1040),
            GuestAddress(0x1000),
            GuestAddress(0x1040),
        ],
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        code.formation_roots(),
        &[
            GuestAddress(0x1000),
            GuestAddress(0x1040),
            GuestAddress(0x1044),
        ]
    );
    assert!(code.entrypoints().contains(&GuestAddress(0x1044)));
}

#[test]
fn formation_retains_more_than_eight_sorted_roots_within_the_existing_limits() {
    let mut memory = memory();
    let expected: Vec<_> = (0..12)
        .map(|index| GuestAddress(0x1100 + index * 0x10))
        .collect();
    for &pc in &expected {
        memory.write(pc, &[0x40, 0xc3]).unwrap();
    }
    let mut roots: Vec<_> = expected.iter().rev().copied().collect();
    roots.extend_from_slice(&[expected[5], expected[0], expected[11]]);
    let limits = CompileOptions {
        blocks: 16,
        ..options()
    };
    let code = CompiledCode::compile_transition(&memory, expected[11], limits, &[], &[], &roots)
        .unwrap()
        .expect("all twelve roots fit the existing block and instruction limits");
    assert_eq!(code.formation_roots(), expected);
    assert_eq!(code.entrypoints(), expected);
    assert_eq!(code.instruction_count(), 24);
    assert!(code.wasm().len() <= limits.wasm_bytes);
    assert!(code.is_current(&memory));
    memory.write(expected[11], &[0x48]).unwrap();
    assert!(!code.is_current(&memory));
}

#[test]
fn exceeding_the_block_limit_or_losing_a_root_to_shortening_declines() {
    let mut memory = memory();
    for index in 0..9 {
        memory
            .write(GuestAddress(0x1100 + index * 0x10), &[0x40, 0xc3])
            .unwrap();
    }
    let roots: Vec<_> = (0..8)
        .map(|index| GuestAddress(0x1100 + index * 0x10))
        .collect();
    assert!(
        CompiledCode::compile_transition(
            &memory,
            GuestAddress(0x1180),
            options(),
            &[],
            &[],
            &roots,
        )
        .unwrap()
        .is_none()
    );
    assert!(
        CompiledCode::compile_transition(
            &memory,
            GuestAddress(0x1040),
            CompileOptions {
                instructions: 1,
                ..options()
            },
            &[],
            &[],
            &[GuestAddress(0x1000), GuestAddress(0x1040)],
        )
        .unwrap()
        .is_none()
    );
    let normal = compile(&memory, GuestAddress(0x1040), options()).unwrap();
    assert_eq!(normal.formation_roots(), &[GuestAddress(0x1040)]);
    assert!(normal.is_current(&memory));

    let roots = [
        GuestAddress(0x1000),
        GuestAddress(0x1040),
        GuestAddress(0x1080),
    ];
    let full = CompiledCode::compile_transition(
        &memory,
        GuestAddress(0x1080),
        options(),
        &[],
        &[],
        &roots,
    )
    .unwrap()
    .unwrap();
    assert!(
        CompiledCode::compile_transition(
            &memory,
            GuestAddress(0x1080),
            CompileOptions {
                wasm_bytes: full.wasm().len() - 1,
                ..options()
            },
            &[],
            &[],
            &roots,
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn another_owners_frontier_and_reached_gates_are_not_bypassed() {
    let mut memory = memory();
    assert!(
        CompiledCode::compile_transition(
            &memory,
            GuestAddress(0x1040),
            options(),
            &[],
            &[GuestAddress(0x1000)],
            &[GuestAddress(0x1000), GuestAddress(0x1040)],
        )
        .unwrap()
        .is_none()
    );
    memory.write(GuestAddress(0x1040), &[0x0f, 0x0b]).unwrap();
    assert!(matches!(
        CompiledCode::compile_transition(
            &memory,
            GuestAddress(0x1040),
            options(),
            &[(GuestAddress(0x1040), 0)],
            &[],
            &[GuestAddress(0x1000), GuestAddress(0x1040)],
        ),
        Err(CompileError::InvalidLimits)
    ));
    let gated = CompiledCode::compile_transition(
        &memory,
        GuestAddress(0x1040),
        options(),
        &[(GuestAddress(0x1040), 77)],
        &[],
        &[GuestAddress(0x1000), GuestAddress(0x1040)],
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        gated.formation_roots(),
        &[GuestAddress(0x1000), GuestAddress(0x1040)]
    );
    assert!(gated.native_instructions().is_empty());
    assert!(gated.is_current(&memory));
}
