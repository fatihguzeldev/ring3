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
fn pairs_keep_exact_nonoverlapping_entries_and_descriptor_provenance() {
    // fld1; fldz; fadd st0,st1; fchs; fabs.
    let instructions = instructions(&[0xd9, 0xe8, 0xd9, 0xee, 0xd8, 0xc1, 0xd9, 0xe0, 0xd9, 0xe1]);
    let pairs = x87_pairs(&instructions).collect::<Vec<_>>();
    assert_eq!(pairs.len(), 2);
    assert_eq!(
        pairs
            .iter()
            .map(|pair| [pair[0].pc, pair[1].pc])
            .collect::<Vec<_>>(),
        [[0x1000, 0x1002], [0x1004, 0x1006]]
    );
    for (pair, instruction_pair) in pairs.iter().zip(instructions.chunks_exact(2)) {
        for (step, instruction) in pair.iter().zip(instruction_pair) {
            assert_eq!(step.pc, instruction.pc().0);
            assert_eq!(step.next_pc, instruction.next_pc().0);
            assert_eq!(step.length, instruction.length());
            assert_eq!(step.control, 1);
            let Operation::X87 {
                operation, opcode, ..
            } = *instruction.operation()
            else {
                panic!("x87 fixture");
            };
            assert_eq!(step.operation, operation);
            assert_eq!(step.opcode, opcode);
        }
    }
}

#[test]
fn pairs_reject_guest_writes_flags_control_and_nonadjacent_material() {
    for excluded in [
        &[0xd9, 0x1b][..], // fstp dword[ebx].
        &[0xdb, 0xf1],     // fcomi st0,st1.
        &[0xdf, 0xe0],     // fnstsw ax.
        &[0xd9, 0x2b],     // fldcw word[ebx].
        &[0xd9, 0x23],     // fldenv[ebx].
        &[0x90],           // integer instruction.
    ] {
        let code = [&[0xd9, 0xe8][..], excluded, &[0xd9, 0xee]].concat();
        assert!(x87_pairs(&instructions(&code)).next().is_none());
    }
    let mut gapped = instructions(&[0xd9, 0xe8, 0x90, 0xd9, 0xee]);
    gapped.remove(1);
    assert!(x87_pairs(&gapped).next().is_none());

    let memory = instructions(&[0x64, 0xd9, 0x43, 4, 0xd8, 0x01]);
    let pair = x87_pairs(&memory).next().unwrap();
    assert_eq!(pair[0].operation.read_size(), 4);
    assert_eq!(
        pair[0].control,
        1 | ((crate::cpu::x86::SegmentRegister::Fs.index() as u32 + 1) << 8)
    );
    assert_eq!(pair[1].operation.read_size(), 4);
    assert_eq!(pair[0].address.unwrap().displacement, 4);
}

#[test]
fn progress_encoding_keeps_transport_and_prefix_fields_disjoint() {
    for status in [0, 1, 11, 16, 23, u32::MAX] {
        for (completed, reported_step) in [(0, 0), (1, 0), (1, 1), (2, 1)] {
            let encoded = X87PairProgress32::encode(status, completed, reported_step);
            assert_eq!(encoded as u32, status);
            assert_eq!(
                (encoded >> X87_PAIR_COMPLETED_SHIFT) & 3,
                u64::from(completed)
            );
            assert_eq!(
                (encoded >> X87_PAIR_REPORTED_SHIFT) & 1,
                u64::from(reported_step)
            );
            assert_eq!(encoded >> 35, 0);
        }
    }
}
