use crate::{cpu::ExitReason, memory::Access, process::CpuEvent32};

pub const EXCEPTION_RECORD_SIZE32: usize = 80;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExceptionRecord32 {
    pub code: u32,
    pub flags: u32,
    pub nested: u32,
    pub address: u32,
    pub count: u32,
    pub information: [u32; 15],
}
impl ExceptionRecord32 {
    pub fn new(
        code: u32,
        flags: u32,
        address: u32,
        information: &[u32],
    ) -> Result<Self, super::ExceptionError32> {
        if information.len() > 15 || flags & !0x7f != 0 {
            return Err(super::ExceptionError32::InvalidParameter);
        }
        let mut values = [0; 15];
        values[..information.len()].copy_from_slice(information);
        Ok(Self {
            code,
            flags,
            nested: 0,
            address,
            count: information.len() as u32,
            information: values,
        })
    }
    pub fn bytes(self) -> [u8; EXCEPTION_RECORD_SIZE32] {
        let mut bytes = [0; EXCEPTION_RECORD_SIZE32];
        for (index, word) in [self.code, self.flags, self.nested, self.address, self.count]
            .into_iter()
            .chain(self.information)
            .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
    pub fn from_cpu_event(event: &CpuEvent32) -> Option<Self> {
        let mut information = [0; 15];
        let mut count = 0;
        let code = match event.exit.reason {
            ExitReason::DivideError => 0xc0000094,
            ExitReason::DebugTrap => 0x80000004,
            ExitReason::Breakpoint => 0x80000003,
            ExitReason::Overflow => 0xc0000095,
            ExitReason::BoundRange => 0xc000008c,
            ExitReason::InvalidOpcode => 0xc000001d,
            ExitReason::MemoryFault { fault, .. } => {
                count = 2;
                information[0] = access(fault.access);
                information[1] = fault.address.0;
                0xc0000005
            }
            ExitReason::PageFault {
                linear,
                access: kind,
                ..
            } => {
                count = 2;
                information[0] = access(kind);
                information[1] = linear.0;
                0xc0000005
            }
            ExitReason::FloatingPoint { status } => floating(
                status as u32,
                u32::from(event.x87.control),
                status & 0x40 != 0,
            ),
            ExitReason::SimdFloatingPoint { mxcsr } => floating(mxcsr, (mxcsr >> 7) & 0x3f, false),
            ExitReason::GeneralProtection { .. } => 0xc0000005,
            ExitReason::DeviceNotAvailable
            | ExitReason::InvalidTss { .. }
            | ExitReason::SegmentNotPresent { .. } => 0xc0000096,
            ExitReason::StackSegment { .. } => 0xc00000fd,
            _ => return None,
        };
        Some(Self {
            code,
            flags: 0,
            nested: 0,
            address: event
                .instruction
                .map_or(event.state.eip, |instruction| instruction.eip),
            count,
            information,
        })
    }
}
fn access(access: Access) -> u32 {
    match access {
        Access::Read => 0,
        Access::Write => 1,
        Access::Execute => 8,
    }
}
fn floating(flags: u32, mask: u32, stack: bool) -> u32 {
    let raised = flags & !mask & 0x3f;
    for (bit, code) in [
        (1, if stack { 0xc0000092 } else { 0xc0000090 }),
        (2, 0xc000008d),
        (4, 0xc000008e),
        (8, 0xc0000091),
        (16, 0xc0000093),
        (32, 0xc000008f),
    ] {
        if raised & bit != 0 {
            return code;
        }
    }
    0xc0000090
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exception_record_is_literal_pe32_and_limits_arguments() {
        let record = ExceptionRecord32::new(0xc0000005, 1, 0x11223344, &[8, 0xdeadbeef]).unwrap();
        let bytes = record.bytes();
        assert_eq!(
            &bytes[..28],
            &[
                5, 0, 0, 0xc0, 1, 0, 0, 0, 0, 0, 0, 0, 0x44, 0x33, 0x22, 0x11, 2, 0, 0, 0, 8, 0, 0,
                0, 0xef, 0xbe, 0xad, 0xde
            ]
        );
        assert_eq!(&bytes[28..], &[0; 52]);
        assert!(ExceptionRecord32::new(1, 0, 0, &[0; 16]).is_err());
    }
    #[test]
    fn floating_status_honors_masks_and_stack_priority() {
        assert_eq!(floating(4, 0x3b, false), 0xc000008e);
        assert_eq!(floating(0x41, 0x3e, true), 0xc0000092);
        assert_eq!(floating(0x30, 0xf, false), 0xc0000093);
    }
}
