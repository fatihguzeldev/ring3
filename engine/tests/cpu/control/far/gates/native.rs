use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
        x86::{SEGMENTS_SIZE, decode_current_segment_context},
    },
    cpu::x86::{SegmentDescriptorRecord, SegmentRegister},
    memory::GuestAddress,
    process::{EngineInstance, HostError, StoreCompletion},
};

const KEY: u64 = 0x4341_4c4c_4741_5445;
const STACK: u32 = 0x7800;

struct Bound {
    engine: EngineInstance,
    id: Option<u64>,
}
fn upload(engine: &mut EngineInstance, at: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(at, bytes.len() as u32).unwrap();
}
fn bound(resident: bool) -> Bound {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x7000, 1, 3).unwrap();
    upload(&mut engine, 0x1000, &[0xeb, 0xfe]);
    upload(&mut engine, 0x7000, &[0x6b; 4096]);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    let id = if resident {
        Some(engine.compile_resident(1).unwrap().get())
    } else {
        engine.compile(1).unwrap();
        None
    };
    engine
        .configure_segment_descriptor_raw(
            0x4b,
            SegmentDescriptorRecord::from_words(0xffff, 0x00cffa00),
        )
        .unwrap();
    Bound { engine, id }
}
fn gate(
    kind: u32,
    selector: u16,
    offset: u32,
    dpl: u32,
    present: bool,
    count: u32,
) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        (u32::from(selector) << 16) | (offset & 0xffff),
        (if kind == 12 { offset & 0xffff0000 } else { 0 })
            | (kind << 8)
            | (dpl << 13)
            | (u32::from(present) << 15)
            | count,
    )
}
fn install(bound: &mut Bound, raw: SegmentDescriptorRecord) {
    bound
        .engine
        .configure_segment_descriptor_raw(0x43, raw)
        .unwrap();
}
fn jump(bound: &mut Bound, offset: u32, word: u32) -> Result<(), HostError> {
    match bound.id {
        Some(id) => bound.engine.far_jump_resident(KEY, id, offset, 0x43, word),
        None => bound.engine.far_jump(offset, 0x43, word),
    }
}
fn call(bound: &mut Bound, offset: u32, word: u32) -> Result<StoreCompletion, HostError> {
    match bound.id {
        Some(id) => bound
            .engine
            .far_call_resident(KEY, id, offset, 0x43, word, 0x12345678, STACK),
        None => bound.engine.far_call(offset, 0x43, word, 0x12345678, STACK),
    }
}
fn field(engine: &EngineInstance, at: usize) -> u32 {
    u32::from_le_bytes(
        engine.arena()[HELPER_OFFSET + at..HELPER_OFFSET + at + 4]
            .try_into()
            .unwrap(),
    )
}
fn ram(engine: &EngineInstance, page: u32) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(page), &mut bytes)
        .unwrap();
    bytes
}
fn context(engine: &EngineInstance) -> ring3_engine::cpu::x86::SegmentContext {
    decode_current_segment_context(
        &engine.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
        Some(&engine.arena()[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + 40]),
    )
    .unwrap()
}
fn ownership(before: &[u8], engine: &EngineInstance) {
    let mut want = before.to_vec();
    for (at, len) in [
        (HELPER_OFFSET, HELPER_SIZE),
        (SEGMENTS_OFFSET, SEGMENTS_SIZE),
        (SEGMENT_BASES_RECORD_OFFSET, 40),
    ] {
        want[at..at + len].copy_from_slice(&engine.arena()[at..at + len]);
    }
    assert_eq!(
        engine.arena(),
        want,
        "helper leaves state, exit, cancel, transfers and all other arena owners unchanged"
    );
}
fn gate_jump(kind: u32, instruction_word: u32, target: u32) {
    for resident in [false, true] {
        let mut b = bound(resident);
        install(&mut b, gate(kind, 0x48, target, 3, true, 31));
        let before = b.engine.arena().to_vec();
        let old = context(&b.engine);
        let code = ram(&b.engine, 0x1000);
        let stack = ram(&b.engine, 0x7000);
        assert_eq!(
            jump(&mut b, 0xdeadbeef, instruction_word),
            Ok(()),
            "configured same-CPL gate is admitted"
        );
        assert_eq!((field(&b.engine, 16), field(&b.engine, 20)), (0, target));
        let mut expected = old;
        expected.state.selectors[1] = 0x4b;
        assert_eq!(
            context(&b.engine),
            expected,
            "loaded CS names target code, not gate"
        );
        ownership(&before, &b.engine);
        assert_eq!(ram(&b.engine, 0x1000), code);
        assert_eq!(ram(&b.engine, 0x7000), stack);
    }
}
fn gate_call(kind: u32, instruction_word: u32, target: u32, slot: usize) {
    for resident in [false, true] {
        let mut b = bound(resident);
        install(&mut b, gate(kind, 0x48, target, 3, true, 31));
        let before = b.engine.arena().to_vec();
        let old = context(&b.engine);
        let code = ram(&b.engine, 0x1000);
        let mut stack = ram(&b.engine, 0x7000);
        assert_eq!(
            call(&mut b, 0xdeadbeef, instruction_word),
            Ok(StoreCompletion::Complete),
            "gate width owns frame regardless instruction66"
        );
        assert_eq!((field(&b.engine, 16), field(&b.engine, 20)), (0, target));
        let end = (STACK - 0x7000) as usize;
        stack[end - slot..end].copy_from_slice(&0x1b_u32.to_le_bytes()[..slot]);
        stack[end - 2 * slot..end - slot].copy_from_slice(&0x12345678_u32.to_le_bytes()[..slot]);
        assert_eq!(
            ram(&b.engine, 0x7000),
            stack,
            "exact frame and every other stack byte; parameter count31 makes no copies"
        );
        assert_eq!(ram(&b.engine, 0x1000), code);
        let mut expected = old;
        expected.state.selectors[1] = 0x4b;
        assert_eq!(context(&b.engine), expected);
        ownership(&before, &b.engine);
    }
}

#[test]
fn gate16_jmp_ignores_operand_offset_and_uses_gate_target() {
    gate_jump(4, 0, 0x2345);
}
#[test]
fn gate32_jmp_keeps_full_gate_offset_despite_word_instruction() {
    gate_jump(12, 1, 0x12345);
}
#[test]
fn gate16_call_uses_word_frame_despite_dword_instruction() {
    gate_call(4, 0, 0x2345, 2);
}
#[test]
fn gate32_call_uses_dword_frame_despite_word_instruction() {
    gate_call(12, 1, 0x12345, 4);
}

#[test]
fn gate_target_faults_attribute_embedded_selector_without_code_or_stack_publication() {
    for (selector, target, expected_status, detail) in [
        (0_u16, None, 3, 0),
        (0x53, None, 3, 0x50),
        (0x48, Some(0x00cff200_u32), 3, 0x48),
        (0x48, Some(0x00cf7a00), 6, 0x48),
    ] {
        let mut b = bound(false);
        if let Some(high) = target {
            b.engine
                .configure_segment_descriptor_raw(
                    0x48,
                    SegmentDescriptorRecord::from_words(0xffff, high),
                )
                .unwrap();
        }
        install(&mut b, gate(12, selector, 0x2000, 3, true, 0));
        let before = b.engine.arena().to_vec();
        let stack = ram(&b.engine, 0x7000);
        assert_eq!(
            jump(&mut b, 0, 0),
            Ok(()),
            "inner descriptor faults are architectural rather than unresolved profile"
        );
        assert_eq!(
            (field(&b.engine, 16), field(&b.engine, 24)),
            (expected_status, detail)
        );
        let mut want = before;
        want[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
            .copy_from_slice(&b.engine.arena()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]);
        assert_eq!(b.engine.arena(), want);
        assert_eq!(ram(&b.engine, 0x7000), stack);
    }
}
#[test]
fn gate_jump_to_more_privileged_nonconforming_code_is_gp_target() {
    let mut b = bound(false);
    b.engine
        .configure_segment_descriptor_raw(
            0x48,
            SegmentDescriptorRecord::from_words(0xffff, 0x00cf9a00),
        )
        .unwrap();
    install(&mut b, gate(12, 0x48, 0x2000, 3, true, 0));
    assert_eq!(jump(&mut b, 0, 0), Ok(()));
    assert_eq!((field(&b.engine, 16), field(&b.engine, 24)), (3, 0x48));
}

#[test]
fn outer_gate_privilege_and_presence_controls_keep_existing_v7_fault_contract() {
    for (dpl, present, error) in [
        (2, false, CpuMemoryError::GeneralProtection(0x40)),
        (3, false, CpuMemoryError::SegmentNotPresent(0x40)),
    ] {
        let mut b = bound(false);
        install(&mut b, gate(12, 0, 0, dpl, present, 0));
        let before = b.engine.arena().to_vec();
        let stack = ram(&b.engine, 0x7000);
        assert_eq!(jump(&mut b, 0, 0), Ok(()));
        let mut packet = [0; HELPER_SIZE];
        encode_cpu_result(0, 0, Err(error), &mut packet).unwrap();
        let mut want = before;
        want[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&packet);
        assert_eq!(b.engine.arena(), want);
        assert_eq!(ram(&b.engine, 0x7000), stack);
    }
}
#[test]
fn direct_code_control_retains_operand_width_and_v7_result() {
    let mut b = bound(false);
    b.engine
        .configure_segment_descriptor_raw(
            0x43,
            SegmentDescriptorRecord::from_words(0xffff, 0x00cffa00),
        )
        .unwrap();
    assert_eq!(jump(&mut b, 0xabcd2345, 1), Ok(()));
    assert_eq!(field(&b.engine, 20), 0x2345);
    assert_eq!(
        u16::from_le_bytes(
            b.engine.arena()[HELPER_OFFSET + 4..HELPER_OFFSET + 6]
                .try_into()
                .unwrap()
        ),
        7
    );
    assert_eq!(
        context(&b.engine).state.selectors[SegmentRegister::Cs.index()],
        0x43
    );
}
#[test]
fn privilege_changing_gate_call_remains_neutral_missing_tss_boundary() {
    let mut b = bound(false);
    b.engine
        .configure_segment_descriptor_raw(
            0x48,
            SegmentDescriptorRecord::from_words(0xffff, 0x00cf9a00),
        )
        .unwrap();
    install(&mut b, gate(12, 0x48, 0x2000, 3, true, 31));
    let before = b.engine.arena().to_vec();
    let stack = ram(&b.engine, 0x7000);
    crate::support_fetch::assert_host_contract(
        (call(&mut b, 0, 0)).expect_err("compilation must refuse this fixture"),
        HostError::InvalidRequest,
    );
    assert_eq!(b.engine.arena(), before);
    assert_eq!(ram(&b.engine, 0x7000), stack);
}
#[test]
fn task_gate_outer_admission_remains_neutral_and_separate() {
    let mut b = bound(false);
    install(&mut b, gate(5, 0x48, 0, 3, true, 0));
    let before = b.engine.arena().to_vec();
    let stack = ram(&b.engine, 0x7000);
    crate::support_fetch::assert_host_contract(
        (jump(&mut b, 0, 0)).expect_err("compilation must refuse this fixture"),
        HostError::InvalidRequest,
    );
    assert_eq!(b.engine.arena(), before);
    assert_eq!(ram(&b.engine, 0x7000), stack);
}
