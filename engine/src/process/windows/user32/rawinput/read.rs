use super::{super::User32Context32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::user32::User32Error32 as E,
};
impl User32Context32 {
    pub(super) fn read_raw_input(
        &self,
        args: &[u32],
        mem: &mut AddressSpace,
        thread: u32,
    ) -> Result<u32, E> {
        if args[4] != 16 || !matches!(args[1], 0x1000_0003 | 0x1000_0005) {
            return Err(E::Win32(87));
        }
        let packet = self.raw_input.packet(args[0], thread)?;
        let required = if args[1] == 0x1000_0005 {
            16
        } else {
            packet.length
        };
        let available = memory::dword(mem, args[3])?;
        if args[2] == 0 {
            memory::size(mem, args[3], required as u32)?;
            return Ok(0);
        }
        if available < required as u32 {
            memory::size(mem, args[3], required as u32)?;
            return Err(E::Win32(122));
        }
        memory::output(mem, args[2], required)?;
        mem.write(GuestAddress(args[2]), &packet.bytes[..required])?;
        Ok(required as u32)
    }
    pub(super) fn read_raw_input_buffer(
        &mut self,
        args: &[u32],
        mem: &mut AddressSpace,
        thread: u32,
    ) -> Result<u32, E> {
        if args[2] != 16 {
            return Err(E::Win32(87));
        }
        let available = memory::dword(mem, args[1])? as usize;
        let first = self
            .raw_input
            .queued(thread)
            .next()
            .map_or(0, |packet| packet.length);
        if args[0] == 0 {
            memory::size(mem, args[1], first as u32)?;
            return Ok(0);
        }
        if available < first {
            memory::size(mem, args[1], first as u32)?;
            return Err(E::Win32(122));
        }
        let mut length = 0;
        let mut count = 0;
        for packet in self.raw_input.queued(thread) {
            if packet.length > available - length {
                break;
            }
            length += packet.length;
            count += 1;
        }
        memory::output(mem, args[0], length)?;
        let mut bytes = memory::vector(length)?;
        let mut ids = Vec::new();
        ids.try_reserve_exact(count).map_err(|_| E::Allocation)?;
        let mut offset = 0;
        for packet in self.raw_input.queued(thread).take(count) {
            bytes[offset..offset + packet.length].copy_from_slice(&packet.bytes[..packet.length]);
            offset += packet.length;
            ids.push(packet.id);
        }
        mem.write(GuestAddress(args[0]), &bytes)?;
        self.state.discard_raw_input(thread, &ids);
        self.raw_input.retire(&ids);
        Ok(count as u32)
    }
    pub(in crate::process::windows::user32) fn default_raw_input(
        &mut self,
        window: u32,
        thread: u32,
        id: u32,
    ) -> Result<(), E> {
        let packet = self.raw_input.packet(id, thread)?;
        if packet.window != window {
            return Err(E::Win32(6));
        }
        self.raw_input.retire(&[id]);
        Ok(())
    }
}
