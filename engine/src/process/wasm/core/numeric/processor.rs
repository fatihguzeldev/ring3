use super::*;
use crate::cpu::x86::{
    ControlOperation, Register32, USER_MODE_ENVIRONMENT, capabilities::CPU_ID_LEAVES, ir::Operation,
};

impl NumericBank {
    pub(super) fn processor(
        &mut self,
        instance: &mut EngineInstance,
        context: &mut Context32,
        instruction: &NativeInstruction,
    ) -> NumericResult<bool> {
        let mut state = context.state();
        match instruction.operation {
            Operation::CpuId => {
                let leaf = state.registers[Register32::Eax.index()];
                let subleaf = state.registers[Register32::Ecx.index()];
                let values = CPU_ID_LEAVES
                    .iter()
                    .find(|candidate| {
                        candidate.function == leaf
                            && candidate.subfunction.is_none_or(|sub| sub == subleaf)
                    })
                    .map_or([0; 4], |leaf| leaf.registers);
                for (register, value) in [
                    Register32::Eax,
                    Register32::Ebx,
                    Register32::Ecx,
                    Register32::Edx,
                ]
                .into_iter()
                .zip(values)
                {
                    state.registers[register.index()] = value;
                }
            }
            Operation::ReadProcessorId { destination } => {
                state.registers[destination.index()] = USER_MODE_ENVIRONMENT.tsc_aux()
            }
            Operation::VirtualTimestamp | Operation::VirtualTimestampAux => {
                let timestamp = instance
                    .cpu_clock
                    .timestamp()
                    .map_err(|_| Failure::Infrastructure)?;
                state.registers[Register32::Eax.index()] = timestamp as u32;
                state.registers[Register32::Edx.index()] = (timestamp >> 32) as u32;
                if matches!(instruction.operation, Operation::VirtualTimestampAux) {
                    state.registers[Register32::Ecx.index()] = USER_MODE_ENVIRONMENT.tsc_aux();
                }
            }
            Operation::SoftwareInterrupt { vector } => {
                context.set_word(116, u32::from(vector));
                context.set_word(48, instruction.pc);
                context.set_word(52, 0);
                return Ok(true);
            }
            Operation::Control(ControlOperation::Sysenter) => {
                context.set_word(116, 0x100);
                context.set_word(48, instruction.pc);
                context.set_word(52, 0);
                return Ok(true);
            }
            _ => return Err(Failure::Infrastructure),
        }
        context.set_state(state);
        Ok(false)
    }
}
