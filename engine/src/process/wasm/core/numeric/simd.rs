use super::*;
use crate::cpu::x86::{
    CpuMemoryPort32, CpuReadPort32, PagingAccess32, SegmentAccess,
    simd::{self, SimdError, SimdKind, SimdOperand, SimdOperation, SimdRegister},
};

fn general(operand: SimdOperand, state: &crate::cpu::x86::State32) -> u32 {
    if let SimdOperand::Register(SimdRegister::Gpr(register)) = operand {
        state.registers[register.index()]
    } else {
        0
    }
}

pub(super) fn failure(error: SimdError) -> Failure {
    match error {
        SimdError::GeneralProtection(code) => Failure::Cpu(13, code),
        SimdError::FloatingPoint(mxcsr) => Failure::Cpu(19, mxcsr),
        SimdError::X87FloatingPoint(status) => Failure::Cpu(16, u32::from(status)),
        SimdError::InvalidDescriptor | SimdError::InvalidState => Failure::Infrastructure,
    }
}

impl NumericBank {
    pub(super) fn simd(
        &mut self,
        instance: &mut EngineInstance,
        context: &mut Context32,
        op: SimdOperation,
    ) -> NumericResult<()> {
        if matches!(op.kind, SimdKind::Fence | SimdKind::Prefetch) {
            return Ok(());
        }
        if op.uses_mmx() || op.kind == SimdKind::EmptyMmx {
            if self.x87.status & !self.x87.control & 63 != 0 {
                self.x87.status |= 0x8080;
                return Err(Failure::Cpu(16, u32::from(self.x87.status)));
            }
        }
        let mut state = context.state();
        let memory_operand = match (op.source, op.destination) {
            (SimdOperand::Memory(address), _) | (_, SimdOperand::Memory(address)) => Some(address),
            _ => op.implicit_address,
        };
        if matches!(op.kind, SimdKind::SaveExtended | SimdKind::RestoreExtended) {
            let address = memory_operand.ok_or(Failure::Infrastructure)?;
            let save = op.kind == SimdKind::SaveExtended;
            let linear = self.operand(
                address,
                &state,
                512,
                if save {
                    SegmentAccess::Write
                } else {
                    SegmentAccess::Read
                },
            )?;
            if linear & 15 != 0 {
                return Err(Failure::Cpu(13, 0));
            }
            let mut image = [0; 512];
            let mut port = memory::Port(instance.memory.as_mut().ok_or(Failure::Infrastructure)?);
            if save {
                port.check(GuestAddress(linear), 512, PagingAccess32::Write)
                    .map_err(|e| Failure::paging(e, 512))?;
                simd::save_extended(&self.x87, &self.vectors.simd, &mut image);
                port.write(GuestAddress(linear), &image[..464])
                    .map_err(|e| Failure::paging(e, 512))?;
            } else {
                port.read(GuestAddress(linear), &mut image)
                    .map_err(|e| Failure::paging(e, 512))?;
                let (x87, simd) = simd::restore_extended(&image).map_err(failure)?;
                self.x87 = x87;
                self.vectors.simd = simd;
            }
            return Ok(());
        }
        let length = usize::from(op.memory_bytes);
        if length > 16 {
            return Err(Failure::Infrastructure);
        }
        let mut payload = [0; 16];
        if op.reads_memory() {
            let address = memory_operand.ok_or(Failure::Infrastructure)?;
            let linear = self.operand(address, &state, length, SegmentAccess::Read)?;
            let alignment = if !op.aligned {
                1
            } else if op.vector_bytes == 8 {
                8
            } else {
                16
            };
            if linear & (alignment - 1) != 0 {
                return Err(Failure::Cpu(13, 0));
            }
            memory::Port(instance.memory.as_mut().ok_or(Failure::Infrastructure)?)
                .read(GuestAddress(linear), &mut payload[..length])
                .map_err(|e| Failure::paging(e, length))?;
        } else if op.writes_memory() && op.kind != SimdKind::MaskedStore {
            let address = memory_operand.ok_or(Failure::Infrastructure)?;
            self.operand(address, &state, length, SegmentAccess::Write)?;
        }
        let before = self.vectors.simd;
        let mut mmx = std::array::from_fn(|r| self.x87.registers[r][..8].try_into().unwrap());
        let outcome = simd::execute_validated(
            op,
            &mut self.vectors.simd,
            &mut mmx,
            &mut payload,
            general(op.source, &state),
            general(op.destination, &state),
            state.eflags,
        );
        let result = match outcome {
            Ok(result) => result,
            Err(error) => {
                if !matches!(error, SimdError::FloatingPoint(_)) {
                    self.vectors.simd = before;
                }
                return Err(failure(error));
            }
        };
        if op.writes_memory() {
            let store = if op.kind == SimdKind::MaskedStore && result.gpr == 0 {
                Ok(())
            } else {
                let address = memory_operand.ok_or(Failure::Infrastructure)?;
                let linear = self.operand(address, &state, length, SegmentAccess::Write)?;
                let alignment = if !op.aligned {
                    1
                } else if op.vector_bytes == 8 {
                    8
                } else {
                    16
                };
                if linear & (alignment - 1) != 0 {
                    self.vectors.simd = before;
                    return Err(Failure::Cpu(13, 0));
                }
                let mut port =
                    memory::Port(instance.memory.as_mut().ok_or(Failure::Infrastructure)?);
                if op.kind == SimdKind::MaskedStore {
                    port.write_masked(GuestAddress(linear), &payload[..length], result.gpr)
                } else {
                    port.write(GuestAddress(linear), &payload[..length])
                }
            };
            if let Err(error) = store {
                self.vectors.simd = before;
                return Err(Failure::paging(error, length));
            }
        }
        if op.uses_mmx() {
            if let SimdOperand::Register(SimdRegister::Mmx(index)) = op.destination {
                self.x87.registers[usize::from(index)][..8]
                    .copy_from_slice(&mmx[usize::from(index)]);
                self.x87.registers[usize::from(index)][8..].fill(255);
            }
            self.x87.tag = 0;
            self.x87.status &= !0x3800;
        }
        if op.kind == SimdKind::EmptyMmx {
            self.x87.tag = 0xffff;
        }
        if let SimdOperand::Register(SimdRegister::Gpr(register)) = op.destination {
            state.registers[register.index()] = result.gpr;
        }
        state.eflags = result.flags;
        context.set_state(state);
        Ok(())
    }
}
