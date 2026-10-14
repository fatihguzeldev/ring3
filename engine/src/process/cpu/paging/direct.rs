use crate::{
    abi::{
        arena::{HELPER_OFFSET, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        memory_helper::HELPER_SIZE,
        x86::{SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, encode_current_segment_context},
    },
    cpu::x86::{
        CpuPagingError32, CpuReadPort32, PagingAccess32, PagingDomain32, SegmentCache,
        SegmentDescriptorRecord, StackFrameKind,
    },
    memory::GuestAddress,
    process::{EngineInstance, StoreCompletion},
};

const KEY: u64 = 0x5046_4449_5245_4354;
const PAGES: [u32; 5] = [0, 0x1000, 0x5000, 0x7000, 0xffff_f000];

fn record(base: u32, limit: u32, access: u8) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        (base << 16) | (limit & 0xffff),
        (base & 0xff00_0000) | ((base >> 16) & 0xff) | (u32::from(access) << 8) | (limit & 0xf0000),
    )
}

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(5, KEY).unwrap();

    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
    }

    engine
        .configure_segment_descriptor_raw(
            0x43,
            SegmentDescriptorRecord::from_words(0xffff, 0x0040_f800),
        )
        .unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    engine
}

fn ram(engine: &EngineInstance) -> [Vec<u8>; 5] {
    PAGES.map(|page| {
        let mut bytes = vec![0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(page), &mut bytes)
            .unwrap();
        bytes
    })
}

fn input<const N: usize>(engine: &mut EngineInstance, words: [u32; N]) {
    for (index, word) in words.into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + 4 * index..TRANSFER_OFFSET + 4 * index + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
}

fn only_helper_changed(engine: &EngineInstance, before: &[u8]) {
    // the packet codec owns its byte oracle; every other arena byte stays unchanged.
    assert_eq!(&engine.arena()[..HELPER_OFFSET], &before[..HELPER_OFFSET]);
    assert_eq!(
        &engine.arena()[HELPER_OFFSET + HELPER_SIZE..],
        &before[HELPER_OFFSET + HELPER_SIZE..]
    );
}

fn assert_fault(
    error: CpuPagingError32,
    address: u32,
    code: u32,
    access: PagingAccess32,
    domain: PagingDomain32,
) {
    let CpuPagingError32::PageFault(fault) = error else {
        panic!("expected certified PF: {error:?}")
    };
    assert_eq!(
        (
            fault.linear(),
            fault.error_code(),
            fault.access(),
            fault.domain()
        ),
        (GuestAddress(address), code, access, domain)
    );
}

#[test]
fn integer_rmw_and_no_adjustment_arpl_check_user_write_before_payload_or_ram_publication() {
    for descriptor in [0x400, 0x10000, 0x30000] {
        let mut engine = engine();
        engine.write32(0x5010, 0xaabb_1123).unwrap();
        engine.protect(0x5000, 1, 1).unwrap();
        input(
            &mut engine,
            [0, 0x244ed7, 0x5010, 3, 0xaabb_ccdd, 0x1122_3344],
        );
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        let version = engine.memory().unwrap().code_validation_version();
        let width = if descriptor == 0x10000 {
            8
        } else if descriptor == 0x30000 {
            2
        } else {
            4
        };
        let mut bytes = [0xa5; 8];
        assert_fault(
            engine
                .inspect_cpu_memory_port32()
                .read_modify(GuestAddress(0x5010), &mut bytes[..width])
                .unwrap_err(),
            0x5010,
            7,
            PagingAccess32::Write,
            PagingDomain32::User,
        );
        assert_eq!(bytes, [0xa5; 8]);
        assert_eq!(engine.cr2(), None);
        assert_eq!(
            engine.cpu_integer(descriptor, 0x5010),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.cr2(), Some(GuestAddress(0x5010)));
        only_helper_changed(&engine, &before);
        assert_eq!(ram(&engine), before_ram);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    }
}

#[test]
fn far_pointer_read_fault_precedes_descriptor_fault_without_cache_or_accessed_publication() {
    let mut engine = engine();
    engine
        .configure_segment_descriptor_raw(0x43, record(0, 0xffff, 0x72))
        .unwrap();
    let before = engine.arena().to_vec();
    let before_ram = ram(&engine);
    let descriptors = engine.segment_descriptors.clone();
    let version = engine.memory().unwrap().code_validation_version();
    let mut bytes = [0xa5; 6];
    assert_fault(
        engine
            .inspect_cpu_memory_port32()
            .read(GuestAddress(0x5fff), &mut bytes)
            .unwrap_err(),
        0x6000,
        4,
        PagingAccess32::Read,
        PagingDomain32::User,
    );
    assert_eq!(bytes, [0xa5; 6]);
    assert_eq!(engine.segment_load_far(3, 0x5fff, 0), Ok(()));
    assert_eq!(engine.cr2(), Some(GuestAddress(0x6000)));
    only_helper_changed(&engine, &before);
    assert_eq!(engine.segment_descriptors, descriptors);
    assert_eq!(ram(&engine), before_ram);
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
}

#[test]
fn compound_stack_late_write_and_enter_allocation_faults_publish_no_staged_registers_or_pushes() {
    for (kind, esp, allocation, address) in [
        (StackFrameKind::PushAll32, 0x7010, 0, 0x6ffc),
        (StackFrameKind::Enter32, 0x7004, 8, 0x6ff8),
    ] {
        let mut engine = engine();
        input(&mut engine, [1, 2, 3, 4, esp, 0x7004, 7, 8]);
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        let version = engine.memory().unwrap().code_validation_version();
        assert_eq!(
            engine.stack_execute(kind.control(allocation, 0)),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.cr2(), Some(GuestAddress(address)));
        only_helper_changed(&engine, &before);
        assert_eq!(ram(&engine), before_ram);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    }
}

#[test]
fn cached_stack_and_far_target_faults_stay_before_paging_and_late_call_failure_is_atomic() {
    let mut engine = engine();
    let mut context = engine.segment_context().unwrap();
    context.caches[2] = SegmentCache::from_bits(0xf230_7003).unwrap();
    let mut segments = [0; SEGMENTS_SIZE];
    let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
    encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
    engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
        .copy_from_slice(&segments);
    engine.arena_mut().unwrap()
        [SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
        .copy_from_slice(&bases);
    input(&mut engine, [1, 2, 3, 4, 0x7010, 6, 7, 8]);
    let before = engine.arena().to_vec();
    engine
        .stack_execute(StackFrameKind::PushAll32.control(0, 0))
        .unwrap();
    assert_eq!(engine.cr2(), None);
    only_helper_changed(&engine, &before);
    let mut engine = self::engine();
    let before_ram = ram(&engine);
    let descriptors = engine.segment_descriptors.clone();
    let version = engine.memory().unwrap().code_validation_version();
    let before = engine.arena().to_vec();
    engine.far_call(0x10000, 0x43, 0, 0x1007, 0x7004).unwrap();
    assert_eq!(
        engine.cr2(),
        None,
        "target limit must precede the absent second frame page"
    );
    only_helper_changed(&engine, &before);
    let before = engine.arena().to_vec();
    engine.far_call(0x2000, 0x43, 0, 0x1007, 0x7004).unwrap();
    assert_eq!(engine.cr2(), Some(GuestAddress(0x6ffc)));
    only_helper_changed(&engine, &before);
    assert_eq!(engine.segment_descriptors, descriptors);
    assert_eq!(ram(&engine), before_ram);
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
}
