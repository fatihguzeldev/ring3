use crate::{
    abi::{
        arena::X87_OFFSET,
        x86::{X87_SIZE, decode_x87, encode_x87},
    },
    graphics::d3d8::api::{InterfaceKind, MethodGate},
    process::{CallError, EngineInstance, HostError},
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum FpuSetup32 {
    #[default]
    NotReached,
    Entered,
}

pub(super) struct PreparedFpu32 {
    bytes: [u8; X87_SIZE],
}

impl PreparedFpu32 {
    pub(super) fn prepare(
        instance: &EngineInstance,
        gate: Option<MethodGate>,
        arguments: &[u32],
    ) -> Result<Option<Self>, HostError> {
        if !gate.is_some_and(|gate| gate.interface == InterfaceKind::Direct3D && gate.slot == 15)
            || arguments[4] & 2 != 0
        {
            return Ok(None);
        }
        let original = &instance.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE];
        let pending = instance
            .pending_call
            .as_ref()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        if pending.extended.x87.as_slice() != original {
            return Err(HostError::Call(CallError::StateChanged));
        }
        let mut state = decode_x87(original).map_err(|_| HostError::Infrastructure)?;
        state.control = (state.control & !0x0f00) | 0x003f;
        state.status &= !0x8080;
        let mut bytes = [0; X87_SIZE];
        encode_x87(&state, &mut bytes).map_err(|_| HostError::Infrastructure)?;
        Ok(Some(Self { bytes }))
    }

    pub(super) fn commit(self, instance: &mut EngineInstance) {
        // the factory cannot replace the validated call while it owns only guest memory.
        instance.pending_call.as_mut().unwrap().extended.x87 = self.bytes;
        instance.arena.as_mut().get_mut()[X87_OFFSET..X87_OFFSET + X87_SIZE]
            .copy_from_slice(&self.bytes);
    }
}
