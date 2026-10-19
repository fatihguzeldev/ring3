use super::*;
use crate::{
    cpu::{
        dbt::{CompileLimits, cold::prepare_embedded_entry_region},
        x86::{
            SegmentContext,
            decode::{CodeFetchContext, DecodeAuthority},
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn instructions(code: &[u8]) -> Vec<DecodedOperation> {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let bytes = [code, &[0xff, 0xe2]].concat();
    memory.write(GuestAddress(0x1000), &bytes).unwrap();
    let mut prepared = prepare_embedded_entry_region(
        &memory,
        &[GuestAddress(0x1000)],
        CompileLimits::default(),
        &[],
        CodeFetchContext::from_segments(&SegmentContext::default()),
        DecodeAuthority::default(),
    )
    .unwrap();
    prepared.blocks.remove(0).instructions
}

#[test]
fn groups_are_greedy_bounded_and_preserve_exact_instruction_descriptors() {
    let source = instructions(&[0xd9, 0xe8].repeat(13));
    let selected = x87_groups(&source).collect::<Vec<_>>();
    assert_eq!(
        selected,
        [
            X87GroupPlan32 {
                first: 0,
                length: 8
            },
            X87GroupPlan32 {
                first: 8,
                length: 5
            },
        ]
    );
    for plan in selected {
        let group = plan.descriptors(&source).unwrap();
        assert_eq!(group.len(), plan.length);
        for (step, instruction) in group.steps().iter().zip(&source[plan.first..]) {
            assert_eq!(step.pc, instruction.pc().0);
            assert_eq!(step.next_pc, instruction.next_pc().0);
            assert_eq!(step.length, instruction.length());
            assert_eq!(
                *step,
                X87PairStep32::from_instruction(instruction, false).unwrap()
            );
        }
    }
    for length in 0..X87_GROUP_MIN_STEPS {
        assert!(x87_groups(&source[..length]).next().is_none());
    }
    assert!(
        X87GroupPlan32 {
            first: usize::MAX,
            length: 4
        }
        .descriptors(&source)
        .is_none()
    );
    assert!(
        X87GroupPlan32 {
            first: 0,
            length: 9
        }
        .descriptors(&source)
        .is_none()
    );
}

#[test]
fn groups_stop_at_a_final_exact_float32_pop_store_and_reject_other_boundaries() {
    // fld1; fldz; fadd st0,st1; fstp fs:[ebx+4]; four computational operations.
    let code = [
        &[0xd9, 0xe8, 0xd9, 0xee, 0xd8, 0xc1, 0x64, 0xd9, 0x5b, 4][..],
        &[0xd9, 0xe0].repeat(4),
    ]
    .concat();
    let source = instructions(&code);
    let plans = x87_groups(&source).collect::<Vec<_>>();
    assert_eq!(
        plans,
        [
            X87GroupPlan32 {
                first: 0,
                length: 4
            },
            X87GroupPlan32 {
                first: 4,
                length: 4
            }
        ]
    );
    let group = plans[0].descriptors(&source).unwrap();
    assert!(
        group.steps()[..3]
            .iter()
            .all(|step| step.operation.write_size() == 0)
    );
    let last = group.steps()[3];
    assert_eq!(last.operation.write_size(), 4);
    assert_eq!(last.address.unwrap().displacement, 4);
    assert_eq!(
        last.control,
        1 | ((crate::cpu::x86::SegmentRegister::Fs.index() as u32 + 1) << 8)
    );

    for excluded in [
        &[0xd9, 0x13][..], // nonpop float32 store.
        &[0xdd, 0x1b],     // float64 pop store.
        &[0xdb, 0xf1],     // integer flags.
        &[0xdf, 0xe0],     // status to ax.
        &[0xd9, 0x2b],     // control word.
        &[0x90],           // gpr/non-x87 boundary.
    ] {
        let run = [0xd9, 0xe8].repeat(3);
        let code = [run.as_slice(), excluded, run.as_slice()].concat();
        assert!(
            x87_groups(&instructions(&code)).next().is_none(),
            "{excluded:02x?}"
        );
    }
    let mut gap = instructions(&[0xd9, 0xe8].repeat(7));
    gap.remove(3);
    assert!(x87_groups(&gap).next().is_none());
}

#[test]
fn group_progress_has_a_distinct_bounded_envelope() {
    for status in [0, 11, 23, u32::MAX] {
        for reported in 0..X87_GROUP_MAX_STEPS as u32 {
            for completed in [reported, reported + 1] {
                let encoded = X87GroupProgress32::encode(status, completed, reported);
                assert_eq!(encoded as u32, status);
                assert_eq!(
                    (encoded >> X87_GROUP_COMPLETED_SHIFT) & 15,
                    u64::from(completed)
                );
                assert_eq!(
                    (encoded >> X87_GROUP_REPORTED_SHIFT) & 7,
                    u64::from(reported)
                );
                assert_eq!(encoded >> 39, 0);
            }
        }
    }
    // existing pair bits do not change when the new import is introduced.
    assert_eq!(
        super::super::X87PairProgress32::encode(23, 2, 1),
        23 | (2 << 32) | (1 << 34)
    );
}
