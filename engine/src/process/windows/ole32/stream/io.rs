use super::context::stream_error;
use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    process::windows::ole32::Ole32Runtime32,
    windows::ole32::{HResult32, stream::StreamId32},
};
pub(super) const INVALID_POINTER: HResult32 = HResult32(0x8003_0009);
pub(super) fn check_output(
    memory: &AddressSpace,
    address: u32,
    size: usize,
) -> Result<(), HResult32> {
    if address == 0 {
        return Err(INVALID_POINTER);
    }
    memory
        .check_span(GuestAddress(address), size, Access::Write)
        .map_err(|_| INVALID_POINTER)
}
pub(super) fn check_optional_output(
    memory: &AddressSpace,
    address: u32,
    size: usize,
) -> Result<(), HResult32> {
    if address == 0 {
        Ok(())
    } else {
        check_output(memory, address, size)
    }
}
pub(super) fn large(lo: u32, hi: u32) -> u64 {
    u64::from(lo) | (u64::from(hi) << 32)
}
impl Ole32Runtime32 {
    pub(super) fn read_stream(
        &mut self,
        memory: &mut AddressSpace,
        id: StreamId32,
        args: &[u32],
    ) -> Result<u32, HResult32> {
        check_optional_output(memory, args[3], 4)?;
        let prepared = self
            .streams
            .store
            .prepare_read(id, args[2] as usize)
            .map_err(stream_error)?;
        let bytes = self
            .streams
            .store
            .read_bytes(&prepared)
            .map_err(stream_error)?;
        if args[1] == 0 && args[2] != 0 {
            return Err(INVALID_POINTER);
        }
        let length = prepared.length();
        let count = (length as u32).to_le_bytes();
        let mut spans = [
            (GuestAddress(args[1]), bytes),
            (GuestAddress(args[3]), &count[..]),
        ];
        let count = if args[3] == 0 { 1 } else { 2 };
        if args[1] == 0 {
            spans[0].1 = &[];
        }
        memory
            .write_spans(&spans[..count])
            .map_err(|_| INVALID_POINTER)?;
        self.streams.store.commit_read(prepared);
        Ok(u32::from(length < args[2] as usize))
    }
    pub(super) fn write_stream(
        &mut self,
        memory: &mut AddressSpace,
        id: StreamId32,
        args: &[u32],
    ) -> Result<u32, HResult32> {
        check_optional_output(memory, args[3], 4)?;
        if args[1] == 0 && args[2] != 0 {
            return Err(INVALID_POINTER);
        }
        memory
            .check_span(GuestAddress(args[1]), args[2] as usize, Access::Read)
            .map_err(|_| INVALID_POINTER)?;
        let prepared = self
            .streams
            .store
            .prepare_write_region(id, args[2] as usize)
            .map_err(stream_error)?;
        memory
            .ensure_transaction_versions(u64::from(args[3] != 0))
            .map_err(|_| HResult32::UNEXPECTED)?;
        self.streams.store.commit_write_region(prepared, |bytes| {
            memory
                .read(GuestAddress(args[1]), bytes)
                .expect("guest input span was preflighted before stream publication")
        });
        if args[3] != 0 {
            memory
                .write(GuestAddress(args[3]), &args[2].to_le_bytes())
                .expect("guest output and version were preflighted before stream publication");
        }
        Ok(0)
    }
    pub(super) fn seek_stream(
        &mut self,
        memory: &mut AddressSpace,
        id: StreamId32,
        args: &[u32],
    ) -> Result<u32, HResult32> {
        check_optional_output(memory, args[4], 8)?;
        let prepared = self
            .streams
            .store
            .prepare_seek(id, large(args[1], args[2]), args[3])
            .map_err(stream_error)?;
        if args[4] != 0 {
            memory
                .write(GuestAddress(args[4]), &prepared.position().to_le_bytes())
                .map_err(|_| INVALID_POINTER)?;
        }
        self.streams.store.commit_seek(prepared);
        Ok(0)
    }
    pub(super) fn copy_stream(
        &mut self,
        memory: &mut AddressSpace,
        id: StreamId32,
        args: &[u32],
    ) -> Result<u32, HResult32> {
        check_optional_output(memory, args[4], 8)?;
        check_optional_output(memory, args[5], 8)?;
        let target = self.stream_id(memory, args[1])?;
        let prepared = self
            .streams
            .store
            .prepare_copy(id, target, large(args[2], args[3]))
            .map_err(stream_error)?;
        let count = (prepared.length() as u64).to_le_bytes();
        let spans = [
            (
                GuestAddress(args[4]),
                if args[4] == 0 { &[][..] } else { &count[..] },
            ),
            (
                GuestAddress(args[5]),
                if args[5] == 0 { &[][..] } else { &count[..] },
            ),
        ];
        memory.write_spans(&spans).map_err(|_| INVALID_POINTER)?;
        self.streams.store.commit_copy(prepared);
        Ok(0)
    }
    pub(super) fn stat_stream(
        &self,
        memory: &mut AddressSpace,
        id: StreamId32,
        args: &[u32],
    ) -> Result<u32, HResult32> {
        if args[2] > 1 {
            return Err(HResult32(0x8003_0057));
        }
        check_output(memory, args[1], 72)?;
        let mut stat = [0u8; 72];
        stat[4..8].copy_from_slice(&2u32.to_le_bytes());
        stat[8..16].copy_from_slice(
            &self
                .streams
                .store
                .size(id)
                .map_err(stream_error)?
                .to_le_bytes(),
        );
        stat[40..44].copy_from_slice(&2u32.to_le_bytes());
        memory
            .write(GuestAddress(args[1]), &stat)
            .map_err(|_| INVALID_POINTER)?;
        Ok(0)
    }
}
