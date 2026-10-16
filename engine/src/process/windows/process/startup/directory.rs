use super::{ProcessStartup32, bytes::wide_bytes};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::process::ProcessError32,
};

impl ProcessStartup32 {
    pub(in crate::process) fn replace_current_directory(
        &mut self,
        memory: &mut AddressSpace,
        units: &[u16],
    ) -> Result<(), ProcessError32> {
        let bytes = wide_bytes(units)?;
        let mut arena = self.prepare_arena()?;
        let address = arena.allocate(bytes.len(), 2)?;
        arena.release(self.current_directory.0, self.current_directory.1)?;
        let mut descriptor = [0; 8];
        descriptor[..2].copy_from_slice(&((bytes.len() - 2) as u16).to_le_bytes());
        descriptor[2..4].copy_from_slice(&(bytes.len() as u16).to_le_bytes());
        descriptor[4..].copy_from_slice(&address.to_le_bytes());
        memory
            .write_spans(&[
                (GuestAddress(address), &bytes),
                (GuestAddress(self.peb + 0x124), &descriptor),
            ])
            .map_err(|_| ProcessError32::NoAccess)?;
        self.commit_arena(arena);
        self.current_directory = (address, bytes.len());
        Ok(())
    }
}
