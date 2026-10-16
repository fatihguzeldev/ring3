mod callbacks;
mod lifetime;
mod publication;
use super::*;
use crate::{
    cpu::x86::State32,
    memory::{GuestAddress, PageRange},
    process::{
        EngineInstance, calls::call::PendingOwner, windows::callbacks::GuestCallbackScratch32,
    },
    windows::{CallFrame32, CallingConvention32, com::ServerThreadingModel32, ole32::Ole32Api32},
};

fn pending(api: Ole32Api32) -> (EngineInstance, u32) {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine.map(0x10000, 2, 3).unwrap();
    engine.map(0x20000, 1, 5).unwrap();
    engine
        .configure_ole32(crate::process::Ole32Config32 {
            objects: PageRange::new(GuestAddress(0x30000000), 16).unwrap(),
            max_objects: 8,
            max_threads: 8,
        })
        .unwrap();
    let mut state = State32 {
        eip: 0x20000,
        ..Default::default()
    };
    state.registers[4] = 0x10800;
    engine.write32(0x10800, 0x20010).unwrap();
    let frame = CallFrame32::capture(
        engine.memory().unwrap(),
        state,
        CallingConvention32::Stdcall,
        5,
    )
    .unwrap();
    let mut bytes = [0u8; 64];
    bytes[32..48].copy_from_slice(&[0x55; 16]);
    let scratch = GuestCallbackScratch32::allocate(
        engine.memory.as_mut().unwrap(),
        ActivationRuntime32::scratch_reservation(),
        &bytes,
    )
    .unwrap();
    let address = scratch.address.0;
    engine
        .ole32
        .as_mut()
        .unwrap()
        .activation
        .pending
        .push(PendingActivation32 {
            continuation: 1,
            thread: 1,
            owner: PendingOwner::Replacement(1),
            token: 1,
            frame,
            api,
            out: 0x10300,
            outer: 0xabcd,
            path: "native.dll".into(),
            scratch,
            phase: Phase32::Factory,
            threading: ServerThreadingModel32::Both,
            server_threads: vec![1],
            server: None,
        });
    (engine, address)
}
mod sources;
