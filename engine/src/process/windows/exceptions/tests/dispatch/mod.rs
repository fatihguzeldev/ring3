use super::*;

mod collided;
mod nested;

fn chain(engine: &mut EngineInstance) -> (ContextState32, [u32; 3]) {
    let mut original = engine.windows_context_state().unwrap();
    let frames = [0x7f003f00u32, 0x7f003f10, 0x7f003f20];
    let rows: Vec<_> = frames
        .iter()
        .enumerate()
        .map(|(index, &frame)| {
            let next = frames.get(index + 1).copied().unwrap_or(u32::MAX);
            (
                GuestAddress(frame),
                [
                    next.to_le_bytes(),
                    (0x401000u32 + index as u32).to_le_bytes(),
                ]
                .concat(),
            )
        })
        .collect();
    let mut writes = vec![(GuestAddress(0x7e000000), frames[0].to_le_bytes().to_vec())];
    writes.extend(rows);
    let spans: Vec<_> = writes
        .iter()
        .map(|(address, bytes)| (*address, bytes.as_slice()))
        .collect();
    engine.memory.as_mut().unwrap().write_spans(&spans).unwrap();
    original.state.registers[4] = frames[0] - 16;
    original.state.registers[0] = 0x13572468;
    engine.publish_windows_context(&original).unwrap();
    (original, frames)
}

fn word(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

fn dispatcher(engine: &mut EngineInstance, frame: u32) {
    let address = engine
        .exceptions
        .deliveries
        .last()
        .unwrap()
        .dispatcher_address;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), &frame.to_le_bytes())
        .unwrap();
}
