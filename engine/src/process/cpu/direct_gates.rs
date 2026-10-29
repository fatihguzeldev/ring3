use crate::{
    abi::{
        arena::*,
        header,
        x86::{STATE_SIZE, decode_state},
    },
    process::{CallError, EngineInstance, HostError},
};

impl EngineInstance {
    pub(in crate::process) fn try_direct_external_cpu_gate_mask(
        &mut self,
        pc: u32,
        esp: u32,
        eax: u32,
        accepted_mask: u32,
    ) -> Result<bool, HostError> {
        if accepted_mask == 0 || accepted_mask & !0x1fff != 0 {
            return Ok(false);
        }
        if self.exceptions.has_deliveries() || self.guard_owned_host_mutation().is_err() {
            return Ok(false);
        }
        let Some((id, true)) = self.external_cpu_gate_match(pc) else {
            return Ok(false);
        };
        let bit = match id {
            0x10001 => 1,
            0x10002 => 1 << 1,
            0xf000b => 1 << 2,
            0x40225 => 1 << 3,
            0x4022a => 1 << 4,
            0x4022e => 1 << 5,
            0x40232 => 1 << 6,
            0x4023f => 1 << 7,
            0x4023d => 1 << 8,
            0x40253 => 1 << 9,
            0x40255 => 1 << 10,
            0x40228 => 1 << 11,
            0x4022c => 1 << 12,
            _ => 0,
        };
        if accepted_mask & bit == 0 {
            return Ok(false);
        }
        let Ok(mut state) = decode_state(&self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
        else {
            return Ok(false);
        };
        state.eip = pc;
        state.registers[4] = esp;
        state.registers[0] = eax;
        let Some(frame) = self.external_cpu_deferred_frame(pc, id, Some(state)) else {
            return Ok(false);
        };

        // after dispatch, failures are terminal; the caller must never replay this gate.
        let value = self.execute_external_direct_windows_gate(id, frame)?;
        self.guard_core_entry()?;
        if self.callback.is_some() || self.pending_windows_callback().is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let returned = frame.complete(value);
        let mut packet = [0; 32];
        header::write(&mut packet, *b"R3IF");
        for (offset, value) in [
            (16, id),
            (20, value),
            (24, returned.eip),
            (28, returned.registers[4]),
        ] {
            header::write_u32(&mut packet, offset, value);
        }
        self.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + packet.len()]
            .copy_from_slice(&packet);
        Ok(true)
    }

    pub(in crate::process) fn try_direct_external_cpu_gate(
        &mut self,
        pc: u32,
        esp: u32,
        eax: u32,
    ) -> Result<bool, HostError> {
        self.try_direct_external_cpu_gate_mask(pc, esp, eax, 0xff)
    }
}
