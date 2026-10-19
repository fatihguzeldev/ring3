use super::*;

mod faults;
mod publication;

const REGISTER_STORE: &[u8] = &[0xd9, 0xe0, 0xd9, 0x19, 0xff, 0xe2];
const READ_STORE: &[u8] = &[0xd9, 0x03, 0xd9, 0x19, 0xff, 0xe2];

fn one() -> X87State {
    let mut state = X87State {
        tag: 0xfffc,
        ..X87State::default()
    };
    state.registers[0] = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
    state
}

fn negative_prefix() -> X87State {
    let mut state = one();
    state.registers[0][9] = 0xbf;
    state.instruction_pointer = 0x1000;
    state.opcode = 0x1e0;
    state.code_selector = 0x1b;
    state
}

fn stored_negative(address: u32) -> X87State {
    let mut state = negative_prefix();
    state.status = 0x800;
    state.tag = 0xffff;
    state.instruction_pointer = 0x1002;
    state.opcode = 0x119;
    state.data_pointer = address;
    state.data_selector = 0x23;
    state
}

fn actual_fp(engine: &EngineInstance) -> X87State {
    decode_x87(&engine.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap()
}

fn ram(engine: &EngineInstance) -> [u8; 4096] {
    let mut bytes = [0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut bytes)
        .unwrap();
    bytes
}

fn assert_packet(engine: &EngineInstance, version: u32, fields: [u32; 6]) {
    let mut expected = [0; HELPER_SIZE];
    expected[..4].copy_from_slice(b"R3MH");
    expected[4..8].copy_from_slice(&(0x10000 | version).to_le_bytes());
    expected[8..12].copy_from_slice(&40_u32.to_le_bytes());
    for (index, value) in fields.into_iter().enumerate() {
        expected[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        &engine.arena()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        expected
    );
}

fn scalar_result(
    engine: &mut EngineInstance,
    id: u64,
    step: X87PairStep32,
    address: u32,
) -> Result<StoreCompletion, HostError> {
    engine.x87_resident_op(
        KEY,
        id,
        step.operation.encode(),
        address,
        FLAGS,
        step.pc,
        u32::from(step.opcode),
        step.control,
    )
}
