use super::super::{
    DirectInputRuntime32,
    memory::{output, put, words},
};
use crate::{
    input::dinput::{DirectInputResult32 as H, InterfaceKind32},
    memory::{AddressSpace, GuestAddress},
};
impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn get_state(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        size: u32,
        address: u32,
    ) -> Result<u32, H> {
        if size as usize != self.device(pointer)?.native.state_size()? {
            return Err(H::INVALID_PARAM);
        }
        output(memory, address, size as usize)?;
        let device = self.device_mut(pointer)?;
        resize(&mut device.transfer, size as usize)?;
        device.native.read_state(&mut device.transfer)?;
        memory.write(GuestAddress(address), &device.transfer)?;
        device.native.commit_state_read();
        Ok(0)
    }
    pub(in crate::process::windows::dinput) fn get_data(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        args: &[u32],
        interface: InterfaceKind32,
    ) -> Result<u32, H> {
        let size = args[1] as usize;
        if !matches!(size, 16 | 20) || interface.generation() < 8 && size != 16 || args[4] & !1 != 0
        {
            return Err(H::INVALID_PARAM);
        }
        output(memory, args[3], 4)?;
        let [requested] = words(memory, args[3])?;
        let count = (requested as usize).min(self.device(pointer)?.native.buffered()?);
        if args[2] != 0 {
            output(memory, args[2], count * size)?;
        }
        let device = self.device_mut(pointer)?;
        let (_, result) = device.native.read_buffer(&mut [])?;
        let count_bytes = (count as u32).to_le_bytes();
        if args[2] != 0 && count != 0 {
            resize(&mut device.transfer, count * size)?;
            for (index, event) in device.native.buffered_events().take(count).enumerate() {
                for (offset, value) in [
                    event.offset,
                    event.value,
                    event.timestamp_ms,
                    event.sequence,
                    event.app_data,
                ]
                .into_iter()
                .take(size / 4)
                .enumerate()
                {
                    put(&mut device.transfer, index * size + offset * 4, value);
                }
            }
            memory.write_spans(&[
                (GuestAddress(args[2]), &device.transfer),
                (GuestAddress(args[3]), &count_bytes),
            ])?;
        } else {
            memory.write(GuestAddress(args[3]), &count_bytes)?;
        }
        if args[4] & 1 == 0 {
            device.native.consume_buffer(count)?;
        }
        Ok(result.0)
    }
}
fn resize(bytes: &mut Vec<u8>, size: usize) -> Result<(), H> {
    if size > bytes.len() {
        bytes
            .try_reserve(size - bytes.len())
            .map_err(|_| H::OUT_OF_MEMORY)?;
    }
    bytes.resize(size, 0);
    Ok(())
}
