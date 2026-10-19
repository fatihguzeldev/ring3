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
fn wide_plans_are_additive_bounded_and_borrow_exact_final_instructions() {
    for length in [8, 9, 16, 31, 32, 33, 41] {
        let source = instructions(&[0xd9, 0xe8].repeat(length));
        let plans = x87_wide_groups(&source).collect::<Vec<_>>();
        let expected = match length {
            8 => vec![],
            9..=32 => vec![(0, length)],
            33 => vec![(0, 32)],
            41 => vec![(0, 32), (32, 9)],
            _ => unreachable!(),
        };
        assert_eq!(
            plans
                .iter()
                .map(|plan| (plan.first, plan.length))
                .collect::<Vec<_>>(),
            expected
        );
        for plan in plans {
            let view = plan.view(&source).unwrap();
            assert_eq!(view.len(), plan.length);
            for index in 0..view.len() {
                let instruction = &source[plan.first + index];
                let descriptor = view.descriptor(index).unwrap();
                assert_eq!(
                    descriptor,
                    step(instruction, index + 1 == view.len()).unwrap()
                );
                assert_eq!(descriptor.pc, 0x1000 + (plan.first + index) as u32 * 2);
            }
            assert!(view.descriptor(view.len()).is_none());
        }
        // adding a wider catalog does not change the existing greedy v1 plan.
        let legacy = super::super::x87_groups(&source).collect::<Vec<_>>();
        assert_eq!(legacy.first().map(|plan| plan.length), Some(8));
    }
    let source = instructions(&[0xd9, 0xe8].repeat(10));
    for (first, length) in [(usize::MAX, 9), (0, 8), (0, 33), (4, 9)] {
        assert!(X87WideGroupPlan32 { first, length }.view(&source).is_none());
    }
    let mut gap = source;
    gap.remove(4);
    let view = X87WideGroupPlan32 {
        first: 0,
        length: 9,
    }
    .view(&gap)
    .unwrap();
    assert!(view.descriptor(4).is_none());
}

#[test]
fn wide_descriptors_allow_only_a_final_exact_store_and_keep_boundaries() {
    let prefix = [0xd9, 0xe8].repeat(8);
    let code = [prefix.as_slice(), &[0x64, 0xd9, 0x5b, 4]].concat();
    let source = instructions(&code);
    let plan = x87_wide_groups(&source).next().unwrap();
    let view = plan.view(&source).unwrap();
    assert_eq!(view.len(), 9);
    let last = view.descriptor(8).unwrap();
    assert_eq!(last.operation.write_size(), 4);
    assert_eq!(last.address.unwrap().displacement, 4);
    assert_eq!(
        last.control,
        1 | ((crate::cpu::x86::SegmentRegister::Fs.index() as u32 + 1) << 8)
    );

    for excluded in [
        &[0xd9, 0x13][..],
        &[0xdd, 0x1b],
        &[0xdb, 0xf1],
        &[0xdf, 0xe0],
        &[0xd9, 0x2b],
        &[0x90],
    ] {
        let short = [0xd9, 0xe8].repeat(4);
        let code = [short.as_slice(), excluded, short.as_slice()].concat();
        assert!(
            x87_wide_groups(&instructions(&code)).next().is_none(),
            "{excluded:02x?}"
        );
    }
    let short = [0xd9, 0xe8].repeat(4);
    let source = instructions(&[short.as_slice(), &[0xd9, 0x1b], short.as_slice()].concat());
    let view = X87WideGroupPlan32 {
        first: 0,
        length: 9,
    }
    .view(&source)
    .unwrap();
    assert!(
        view.descriptor(4).is_none(),
        "an internal store is never a wide descriptor"
    );
}

#[test]
fn wide_progress_is_separate_from_the_unchanged_v1_envelope() {
    for status in [0, 11, 23, u32::MAX] {
        for reported in 0..32 {
            for completed in [reported, reported + 1] {
                let encoded = X87WideGroupProgress32::encode(status, completed, reported);
                assert_eq!(encoded as u32, status);
                assert_eq!((encoded >> 32) & 63, u64::from(completed));
                assert_eq!((encoded >> 38) & 31, u64::from(reported));
                assert_eq!(encoded >> 43, 0);
            }
        }
    }
    assert_eq!(
        super::super::X87GroupProgress32::encode(23, 8, 7),
        23 | (8 << 32) | (7 << 36)
    );
    assert_eq!(
        X87WideGroupProgress32::encode(0, 32, 31),
        (32 << 32) | (31 << 38)
    );
}
