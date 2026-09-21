use iced_x86::Code;

use super::operands::{Width, check_span, read_value, write_value};
use super::{Cpu32, GuestMemory, Register32, StopReason};
use crate::execution::Access;

const SLOTS: [Register32; 8] = [
    Register32::Edi,
    Register32::Esi,
    Register32::Ebp,
    Register32::Esp,
    Register32::Ebx,
    Register32::Edx,
    Register32::Ecx,
    Register32::Eax,
];

impl Cpu32 {
    pub(super) fn register_stack(
        &mut self,
        code: Code,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        let width = if matches!(code, Code::Pushaw | Code::Popaw) {
            Width::Word
        } else {
            Width::Dword
        };
        let stack = self.register(Register32::Esp);
        if matches!(code, Code::Pushaw | Code::Pushad) {
            let base = stack.wrapping_sub(width as u32 * 8);
            let addresses: [u32; 8] = std::array::from_fn(|slot| {
                base.wrapping_add(u32::try_from(slot).unwrap() * width as u32)
            });
            for address in addresses.iter().rev() {
                check_span(*address, width)?;
                memory
                    .check_access(u64::from(*address), width as usize, Access::Write)
                    .map_err(StopReason::MemoryFault)?;
            }
            for (register, address) in SLOTS.into_iter().zip(addresses).rev() {
                write_value(memory, address, width, self.register(register))?;
            }
            self.set_register(Register32::Esp, base);
        } else {
            let mut values = [0; 8];
            for (slot, value) in values.iter_mut().enumerate() {
                if SLOTS[slot] != Register32::Esp {
                    let address = stack.wrapping_add(u32::try_from(slot).unwrap() * width as u32);
                    *value = read_value(memory, address, width)?;
                }
            }
            for (register, value) in SLOTS.into_iter().zip(values) {
                if register != Register32::Esp {
                    self.set_register(register, (self.register(register) & !width.mask()) | value);
                }
            }
            self.set_register(Register32::Esp, stack.wrapping_add(width as u32 * 8));
        }
        Ok(())
    }
}
