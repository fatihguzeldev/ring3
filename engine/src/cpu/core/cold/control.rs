use super::{
    integer::{self, Operand, condition, location, reg, value, word},
    memory::{Failure, Port},
};
use crate::cpu::x86::{Register32, State32, ir::*};

fn push(state: &mut State32, port: &mut Port<'_>, width: u32, value: u32) -> Result<(), Failure> {
    let pointer = state.registers[4].wrapping_sub(width);
    port.stack_write(pointer, width, value)?;
    state.registers[4] = pointer;
    Ok(())
}

fn target(target: BranchTarget, state: &State32, port: &Port<'_>) -> Result<u32, Failure> {
    match target {
        BranchTarget::Direct(pc) => Ok(pc.0),
        BranchTarget::Indirect(operand) => integer::read(location(operand), state, port, 4, false),
    }
}

pub(super) fn execute(
    operation: Operation,
    next: u32,
    state: &mut State32,
    port: &mut Port<'_>,
) -> Result<bool, Failure> {
    match operation {
        Operation::Nop => {}
        Operation::Jump { target: branch } => state.eip = target(branch, state, port)?,
        Operation::ConditionalJump {
            condition: test,
            target,
        } => {
            if condition(test, state.eflags) {
                state.eip = target.0;
            }
        }
        Operation::CountBranch { kind, target } | Operation::CountBranchWord { kind, target } => {
            let width = if matches!(operation, Operation::CountBranchWord { .. }) {
                2
            } else {
                4
            };
            let mask = integer::mask(width);
            let count = if kind == CountBranchKind::EcxZero {
                state.registers[1] & mask
            } else {
                state.registers[1].wrapping_sub(1) & mask
            };
            if kind != CountBranchKind::EcxZero {
                state.registers[1] = (state.registers[1] & !mask) | count;
            }
            let taken = match kind {
                CountBranchKind::EcxZero => count == 0,
                CountBranchKind::Loop => count != 0,
                CountBranchKind::LoopEqual => count != 0 && state.eflags & 0x40 != 0,
                CountBranchKind::LoopNotEqual => count != 0 && state.eflags & 0x40 == 0,
            };
            if taken {
                state.eip = target.0;
            }
        }
        Operation::Call { target: branch } => {
            let destination = target(branch, state, port)?;
            push(state, port, 4, next)?;
            state.eip = destination;
        }
        Operation::CallWord { target } => {
            push(state, port, 2, next)?;
            state.eip = target.0 & 0xffff;
        }
        Operation::CallRegisterWord { source } => {
            let target = state.registers[source.index()] & 0xffff;
            push(state, port, 2, next)?;
            state.eip = target;
        }
        Operation::ReadCallWord { address } => {
            let target = port.read(address, state, 2, false)?;
            push(state, port, 2, next)?;
            state.eip = target;
        }
        Operation::JumpWord { source } => state.eip = state.registers[source.index()] & 0xffff,
        Operation::ReadJumpWord { address } => state.eip = port.read(address, state, 2, false)?,
        Operation::Return { stack_adjust } | Operation::ReturnWord { stack_adjust } => {
            let width = if matches!(operation, Operation::ReturnWord { .. }) {
                2
            } else {
                4
            };
            let pc = port.stack_read(state.registers[4], width)?;
            state.registers[4] = state.registers[4].wrapping_add(width + u32::from(stack_adjust));
            state.eip = pc;
        }
        Operation::Push { source } => {
            let value = integer::read(value(source), state, port, 4, false)?;
            push(state, port, 4, value)?;
        }
        Operation::PushWord { source } => {
            let value = integer::read(word(source), state, port, 2, false)?;
            push(state, port, 2, value)?;
        }
        Operation::PushMemoryWord { source } => {
            let value = port.read(source, state, 2, false)?;
            push(state, port, 2, value)?;
        }
        Operation::PushFlags | Operation::PushFlagsWord => {
            let width = if matches!(operation, Operation::PushFlagsWord) {
                2
            } else {
                4
            };
            let flags = state.eflags & !0x30000;
            push(state, port, width, flags)?;
        }
        Operation::Pop { destination } => {
            let value = port.stack_read(state.registers[4], 4)?;
            // esp-based destinations are evaluated after the architectural increment.
            state.registers[4] = state.registers[4].wrapping_add(4);
            integer::store(location(destination), state, port, 4, value)?;
        }
        Operation::PopWord { destination } => {
            let value = port.stack_read(state.registers[4], 2)?;
            state.registers[4] = state.registers[4].wrapping_add(2);
            integer::store(reg(destination), state, port, 2, value)?;
        }
        Operation::PopMemoryWord { destination } => {
            let value = port.stack_read(state.registers[4], 2)?;
            state.registers[4] = state.registers[4].wrapping_add(2);
            port.write(destination, state, 2, value)?;
        }
        Operation::Leave | Operation::LeaveWord => {
            let width = if matches!(operation, Operation::LeaveWord) {
                2
            } else {
                4
            };
            let frame = state.registers[5];
            let value = port.stack_read(frame, width)?;
            state.registers[4] = frame.wrapping_add(width);
            integer::store(reg(Register32::Ebp), state, port, width, value)?;
        }
        Operation::EnterZero | Operation::EnterZeroWord => {
            let width = if matches!(operation, Operation::EnterZeroWord) {
                2
            } else {
                4
            };
            let frame = state.registers[5];
            push(state, port, width, frame)?;
            let pointer = state.registers[4];
            integer::store(reg(Register32::Ebp), state, port, width, pointer)?;
        }
        Operation::PopAll | Operation::PopAllWord => {
            let width = if matches!(operation, Operation::PopAllWord) {
                2
            } else {
                4
            };
            let pointer = state.registers[4];
            for (slot, register) in [(0, 7), (1, 6), (2, 5), (4, 3), (5, 2), (6, 1), (7, 0)] {
                let value = port.stack_read(pointer.wrapping_add(slot * width), width)?;
                integer::store(Operand::Register(register, 0), state, port, width, value)?;
            }
            state.registers[4] = pointer.wrapping_add(8 * width);
        }
        _ => return Ok(false),
    }
    Ok(true)
}
