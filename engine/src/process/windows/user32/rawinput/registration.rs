use super::{super::User32Context32, memory};
use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    windows::{rawinput::RawRegistration32, user32::User32Error32 as E, window::WindowHandle32},
};
impl User32Context32 {
    pub(super) fn register_raw_input(
        &mut self,
        args: &[u32],
        mem: &AddressSpace,
    ) -> Result<u32, E> {
        if args[2] != 12 || args[1] == 0 || args[0] == 0 {
            return Err(E::Win32(87));
        }
        memory::aligned(args[0])?;
        if args[1] as usize > self.raw_input.registration_limit() {
            return Err(E::Capacity);
        }
        let length = args[1] as usize * 12;
        mem.check_span(GuestAddress(args[0]), length, Access::Read)?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(args[1] as usize)
            .map_err(|_| E::Allocation)?;
        for index in 0..args[1] {
            let mut bytes = [0; 12];
            mem.read(GuestAddress(args[0] + index * 12), &mut bytes)?;
            let row = RawRegistration32 {
                page: u16::from_le_bytes(bytes[..2].try_into().unwrap()),
                usage: u16::from_le_bytes(bytes[2..4].try_into().unwrap()),
                flags: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
                window: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            };
            if row.window != 0 {
                let handle = WindowHandle32::try_from(row.window)?;
                if self.state.record(handle)?.destroying {
                    return Err(E::Win32(1400));
                }
                self.windows().get(handle)?;
            }
            rows.push(row);
        }
        let prepared = self.raw_input.prepare_registrations(&rows)?;
        self.raw_input.commit_registrations(prepared);
        Ok(1)
    }
    pub(super) fn registered_raw_input(
        &self,
        args: &[u32],
        mem: &mut AddressSpace,
    ) -> Result<u32, E> {
        if args[2] != 12 {
            return Err(E::Win32(87));
        }
        let rows = self.raw_input.registrations();
        let count = rows.len() as u32;
        let available = memory::dword(mem, args[1])?;
        if args[0] == 0 || available < count {
            memory::size(mem, args[1], count)?;
            return Err(E::Win32(122));
        }
        memory::output(mem, args[0], rows.len() * 12)?;
        let mut bytes = memory::vector(rows.len() * 12)?;
        for (row, output) in rows.iter().zip(bytes.chunks_exact_mut(12)) {
            output[..2].copy_from_slice(&row.page.to_le_bytes());
            output[2..4].copy_from_slice(&row.usage.to_le_bytes());
            output[4..8].copy_from_slice(&row.flags.to_le_bytes());
            output[8..12].copy_from_slice(&row.window.to_le_bytes());
        }
        mem.write(GuestAddress(args[0]), &bytes)?;
        Ok(count)
    }
}
