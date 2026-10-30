use super::*;

fn general(operand: SimdOperand, state: &State32) -> u32 {
    if let SimdOperand::Register(SimdRegister::Gpr(register)) = operand {
        state.registers[register.index()]
    } else {
        0
    }
}

impl NumericState {
    pub(super) fn simd(
        &mut self,
        memory: &mut AddressSpace,
        scalar: &mut State32,
        segments: &SegmentContext,
        operation: SimdOperation,
    ) -> Result<(), NumericError> {
        if matches!(operation.kind, SimdKind::Fence | SimdKind::Prefetch) {
            return Ok(());
        }
        if (operation.uses_mmx() || operation.kind == SimdKind::EmptyMmx)
            && self.x87.status & !self.x87.control & 63 != 0
        {
            self.x87.status |= 0x8080;
            return Err(NumericError::Cpu {
                vector: 16,
                detail: u32::from(self.x87.status),
            });
        }
        let address = match (operation.source, operation.destination) {
            (SimdOperand::Memory(address), _) | (_, SimdOperand::Memory(address)) => Some(address),
            _ => operation.implicit_address,
        };
        if matches!(
            operation.kind,
            SimdKind::SaveExtended | SimdKind::RestoreExtended
        ) {
            let save = operation.kind == SimdKind::SaveExtended;
            let linear = operand(
                segments,
                address.ok_or(NumericError::Infrastructure)?,
                scalar,
                512,
                if save {
                    SegmentAccess::Write
                } else {
                    SegmentAccess::Read
                },
            )?;
            if linear & 15 != 0 {
                return Err(NumericError::Cpu {
                    vector: 13,
                    detail: 0,
                });
            }
            let mut image = [0; 512];
            let mut port = memory::Port(memory);
            if save {
                port.check(GuestAddress(linear), 512, PagingAccess32::Write)
                    .map_err(|error| NumericError::memory(error, 512))?;
                simd_semantics::save_extended(&self.x87, &self.vectors.simd, &mut image);
                port.write(GuestAddress(linear), &image[..464])
                    .map_err(|error| NumericError::memory(error, 512))?;
            } else {
                port.read(GuestAddress(linear), &mut image)
                    .map_err(|error| NumericError::memory(error, 512))?;
                let (x87, simd) =
                    simd_semantics::restore_extended(&image).map_err(NumericError::simd)?;
                self.x87 = x87;
                self.vectors.simd = simd;
            }
            return Ok(());
        }
        let length = usize::from(operation.memory_bytes);
        if length > 16 {
            return Err(NumericError::Infrastructure);
        }
        let mut payload = [0; 16];
        let alignment = if !operation.aligned {
            1
        } else if operation.vector_bytes == 8 {
            8
        } else {
            16
        };
        if operation.reads_memory() {
            let linear = operand(
                segments,
                address.ok_or(NumericError::Infrastructure)?,
                scalar,
                length,
                SegmentAccess::Read,
            )?;
            if linear & (alignment - 1) != 0 {
                return Err(NumericError::Cpu {
                    vector: 13,
                    detail: 0,
                });
            }
            memory::Port(memory)
                .read(GuestAddress(linear), &mut payload[..length])
                .map_err(|error| NumericError::memory(error, length))?;
        } else if operation.writes_memory() && operation.kind != SimdKind::MaskedStore {
            operand(
                segments,
                address.ok_or(NumericError::Infrastructure)?,
                scalar,
                length,
                SegmentAccess::Write,
            )?;
        }
        let before = self.vectors.simd;
        let mut mmx =
            std::array::from_fn(|index| self.x87.registers[index][..8].try_into().unwrap());
        let result = simd_semantics::execute(
            operation,
            &mut self.vectors.simd,
            &mut mmx,
            &mut payload,
            general(operation.source, scalar),
            general(operation.destination, scalar),
            scalar.eflags,
        );
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if !matches!(error, SimdError::FloatingPoint(_)) {
                    self.vectors.simd = before;
                }
                return Err(NumericError::simd(error));
            }
        };
        if operation.writes_memory()
            && !(operation.kind == SimdKind::MaskedStore && result.gpr == 0)
        {
            let linear = match operand(
                segments,
                address.ok_or(NumericError::Infrastructure)?,
                scalar,
                length,
                SegmentAccess::Write,
            ) {
                Ok(linear) => linear,
                Err(error) => {
                    self.vectors.simd = before;
                    return Err(error);
                }
            };
            if linear & (alignment - 1) != 0 {
                self.vectors.simd = before;
                return Err(NumericError::Cpu {
                    vector: 13,
                    detail: 0,
                });
            }
            let mut port = memory::Port(memory);
            let store = if operation.kind == SimdKind::MaskedStore {
                port.write_masked(GuestAddress(linear), &payload[..length], result.gpr)
            } else {
                port.write(GuestAddress(linear), &payload[..length])
            };
            if let Err(error) = store {
                self.vectors.simd = before;
                return Err(NumericError::memory(error, length));
            }
        }
        if operation.uses_mmx() {
            if let SimdOperand::Register(SimdRegister::Mmx(index)) = operation.destination {
                self.x87.registers[usize::from(index)][..8]
                    .copy_from_slice(&mmx[usize::from(index)]);
                self.x87.registers[usize::from(index)][8..].fill(255);
            }
            self.x87.tag = 0;
            self.x87.status &= !0x3800;
        }
        if operation.kind == SimdKind::EmptyMmx {
            self.x87.tag = 0xffff;
        }
        if let SimdOperand::Register(SimdRegister::Gpr(register)) = operation.destination {
            scalar.registers[register.index()] = result.gpr;
        }
        scalar.eflags = result.flags;
        Ok(())
    }
}
