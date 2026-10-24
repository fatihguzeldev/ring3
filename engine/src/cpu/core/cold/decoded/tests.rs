use super::*;
use crate::{
    cpu::x86::{SegmentCache, SegmentContext},
    memory::{PageRange, Permissions},
};

fn memory(code: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0x1000, 0x2000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory.write(GuestAddress(0x1000), code).unwrap();
    memory
}

fn fetch() -> CodeFetchContext {
    CodeFetchContext::from_segments(&SegmentContext::default()).unwrap()
}

fn operation(
    book: &mut DecodedBook,
    scratch: &mut DecodeScratch,
    memory: &AddressSpace,
    pc: u32,
    fetch: CodeFetchContext,
) -> (u8, u32, Operation) {
    let BoundDecode::Instruction(step) = book
        .decode(scratch, memory, GuestAddress(pc), fetch)
        .unwrap()
    else {
        panic!("successful instruction expected");
    };
    (step.length(), step.next_pc().0, *step.operation())
}

#[test]
fn unchanged_source_reuses_the_actual_decoded_operation() {
    let memory = memory(&[0x8b, 0x03]);
    let mut book = DecodedBook::default();
    let mut scratch = DecodeScratch::default();
    let first = operation(&mut book, &mut scratch, &memory, 0x1000, fetch());
    for _ in 0..10 {
        assert_eq!(
            operation(&mut book, &mut scratch, &memory, 0x1000, fetch()),
            first
        );
    }
    assert_eq!(scratch.decode_calls(), 1);
    assert_eq!(book.rows.iter().flatten().count(), 1);
}

#[test]
fn content_same_byte_replacement_and_memory_identity_require_fresh_decode() {
    let mut first = memory(&[0x40]);
    let second = memory(&[0x48]);
    let mut book = DecodedBook::default();
    let mut scratch = DecodeScratch::default();
    let inc = operation(&mut book, &mut scratch, &first, 0x1000, fetch());
    first.write(GuestAddress(0x1000), &[0x40]).unwrap();
    assert_eq!(
        operation(&mut book, &mut scratch, &first, 0x1000, fetch()),
        inc
    );
    assert_eq!(
        scratch.decode_calls(),
        2,
        "equal bytes are not equal source authority"
    );
    let dec = operation(&mut book, &mut scratch, &second, 0x1000, fetch());
    assert_ne!(dec.2, inc.2);
    assert_eq!(scratch.decode_calls(), 3);
    first.write(GuestAddress(0x1000), &[0x48]).unwrap();
    assert_eq!(
        operation(&mut book, &mut scratch, &first, 0x1000, fetch()),
        dec
    );
    assert_eq!(scratch.decode_calls(), 4);
}

#[test]
fn current_code_size_and_segment_limit_are_not_inherited_from_a_hit() {
    let memory = memory(&[0xb8, 0x78, 0x56, 0x34, 0x12]);
    let mut book = DecodedBook::default();
    let mut scratch = DecodeScratch::default();
    assert_eq!(
        operation(&mut book, &mut scratch, &memory, 0x1000, fetch()).0,
        5
    );
    let mut segments = SegmentContext::default();
    segments.caches[1] = SegmentCache::from_bits(segments.caches[1].bits() & !(1 << 25)).unwrap();
    let word = CodeFetchContext::from_segments(&segments).unwrap();
    assert_eq!(
        operation(&mut book, &mut scratch, &memory, 0x1000, word).0,
        3
    );
    segments = SegmentContext::default();
    segments.caches[1] = SegmentCache::from_bits(0xf2b0_1000).unwrap();
    let limited = CodeFetchContext::from_segments(&segments).unwrap();
    let BoundDecode::CodeFault(fault) = book
        .decode(&mut scratch, &memory, GuestAddress(0x1000), limited)
        .unwrap()
    else {
        panic!("the current code limit denies the second byte");
    };
    assert_eq!(
        fault.kind,
        crate::cpu::x86::decode::CapturedFetchFaultKind32::Segment
    );
    assert_eq!(
        (fault.attempted_length, fault.consumed_prefix_length),
        (2, 1)
    );
    assert!(memory.is_code_current(fault.prefix.as_ref().unwrap()));
    assert_eq!(scratch.decode_calls(), 3);
    assert_eq!(
        operation(&mut book, &mut scratch, &memory, 0x1000, fetch()).0,
        5
    );
    assert_eq!(
        scratch.decode_calls(),
        3,
        "a failed fetch probe never replaces the valid cached instruction with a fault"
    );
}

#[test]
fn cross_page_execute_failure_keeps_progressive_first_denied_byte() {
    let mut memory = memory(&[0x90]);
    memory
        .write(GuestAddress(0x1ffd), &[0xb8, 0x78, 0x56, 0x34, 0x12])
        .unwrap();
    let mut book = DecodedBook::default();
    let mut scratch = DecodeScratch::default();
    assert_eq!(
        operation(&mut book, &mut scratch, &memory, 0x1ffd, fetch()).0,
        5
    );
    memory
        .protect(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    for expected_calls in [2, 3] {
        let BoundDecode::CodeFault(fault) = book
            .decode(&mut scratch, &memory, GuestAddress(0x1ffd), fetch())
            .unwrap()
        else {
            panic!("the current execute permission denies the fourth byte");
        };
        assert_eq!(
            fault.kind,
            crate::cpu::x86::decode::CapturedFetchFaultKind32::Paging
        );
        assert_eq!(
            (fault.attempted_length, fault.consumed_prefix_length),
            (4, 3)
        );
        assert!(memory.is_code_current(fault.prefix.as_ref().unwrap()));
        assert_eq!(
            scratch.decode_calls(),
            expected_calls,
            "fetch faults are not cached"
        );
    }
    memory
        .protect(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    assert_eq!(
        operation(&mut book, &mut scratch, &memory, 0x1ffd, fetch()).0,
        5
    );
    assert_eq!(scratch.decode_calls(), 4);
    assert_eq!(
        operation(&mut book, &mut scratch, &memory, 0x1ffd, fetch()).0,
        5
    );
    assert_eq!(scratch.decode_calls(), 4);
}

#[test]
fn wrapping_linear_fetch_retains_exactly_two_source_pages() {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0xffff_f000, 0] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
        .write(GuestAddress(0xffff_fffe), &[0xb8, 0x44])
        .unwrap();
    memory.write(GuestAddress(0), &[0x33, 0x22, 0x11]).unwrap();
    let mut segments = SegmentContext::default();
    segments.state.bases[1] = 0xffff_effe;
    let fetch = CodeFetchContext::from_segments(&segments).unwrap();
    let mut book = DecodedBook::default();
    let mut scratch = DecodeScratch::default();
    let first = operation(&mut book, &mut scratch, &memory, 0x1000, fetch);
    assert_eq!((first.0, first.1), (5, 0x1005));
    assert_eq!(
        operation(&mut book, &mut scratch, &memory, 0x1000, fetch),
        first
    );
    assert_eq!(scratch.decode_calls(), 1);
    let entry = book.rows.iter().flatten().next().unwrap();
    assert_eq!(entry.source.versions.len(), 2);
    assert!(memory.is_code_current(&entry.source));
    memory.write(GuestAddress(0), &[0x34]).unwrap();
    assert_ne!(
        operation(&mut book, &mut scratch, &memory, 0x1000, fetch).2,
        first.2
    );
    assert_eq!(scratch.decode_calls(), 2);
}

#[test]
fn fixed_four_way_replacement_and_all_owned_snapshot_storage_fit_the_reservation() {
    let mut memory = memory(&[0x90]);
    memory.write(GuestAddress(0x1000), &[0x90; 4096]).unwrap();
    memory.write(GuestAddress(0x2000), &[0x90; 4096]).unwrap();
    let fetch = fetch();
    let bucket = DecodedBook::first(GuestAddress(0x1000), fetch.decode_key());
    let collisions: Vec<_> = (0x1000..0x3000)
        .filter(|pc| DecodedBook::first(GuestAddress(*pc), fetch.decode_key()) == bucket)
        .take(5)
        .collect();
    assert_eq!(collisions.len(), 5);
    let mut book = DecodedBook::default();
    let mut scratch = DecodeScratch::default();
    for pc in &collisions {
        assert_eq!(
            operation(&mut book, &mut scratch, &memory, *pc, fetch).2,
            Operation::Nop
        );
    }
    assert_eq!(book.rows.len(), ROWS);
    assert_eq!(book.rows.iter().flatten().count(), WAYS);
    let allocated = size_of::<DecodedBook>()
        + book.rows.len() * size_of::<Option<Entry>>()
        + book
            .rows
            .iter()
            .flatten()
            .map(|entry| entry.source.versions.len() * size_of::<PageVersion>())
            .sum::<usize>();
    assert!(
        allocated
            + size_of::<DecodedOperation>()
            + size_of::<CodeSnapshot>()
            + 2 * size_of::<PageVersion>()
            <= RESERVATION
    );
    assert_eq!(scratch.decode_calls(), 5);
    assert_eq!(
        operation(&mut book, &mut scratch, &memory, collisions[0], fetch).2,
        Operation::Nop
    );
    assert_eq!(
        scratch.decode_calls(),
        6,
        "replacement re-decodes without losing exact execution"
    );
}

#[test]
fn optional_snapshot_failure_preserves_successful_fresh_decoding() {
    let memory = memory(&[0x40]);
    let mut book = DecodedBook {
        skip_snapshot: true,
        ..Default::default()
    };
    let mut scratch = DecodeScratch::default();
    for _ in 0..2 {
        let BoundDecode::Instruction(step) = book
            .decode(&mut scratch, &memory, GuestAddress(0x1000), fetch())
            .unwrap()
        else {
            panic!("optional retention cannot manufacture a fetch fault");
        };
        assert!(matches!(step, BookStep::Fresh(_)));
        assert_eq!((step.pc().0, step.next_pc().0), (0x1000, 0x1001));
    }
    assert!(book.rows.iter().all(Option::is_none));
    assert_eq!(scratch.decode_calls(), 2);
}

#[test]
fn invalid_instruction_outcomes_never_become_cached_execution_authority() {
    let memory = memory(&[0x0f, 0x0b]);
    let mut book = DecodedBook::default();
    let mut scratch = DecodeScratch::default();
    for _ in 0..2 {
        assert_eq!(
            operation(&mut book, &mut scratch, &memory, 0x1000, fetch()).2,
            Operation::Control(ControlOperation::InvalidOpcode)
        );
    }
    assert!(book.rows.iter().all(Option::is_none));
    assert_eq!(scratch.decode_calls(), 2);
}
