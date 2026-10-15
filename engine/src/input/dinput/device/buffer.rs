use super::DirectInputDevice32;
use crate::input::{
    dinput::{DirectInputResult32, format::DeviceObject32},
    source::SourceEvent32,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceEvent32 {
    pub offset: u32,
    pub value: u32,
    pub timestamp_ms: u32,
    pub sequence: u32,
    pub app_data: u32,
}
impl DirectInputDevice32 {
    pub fn buffered(&self) -> Result<usize, DirectInputResult32> {
        self.check_acquired()?;
        if self.requested_buffer == 0 {
            Err(DirectInputResult32::NOT_BUFFERED)
        } else {
            Ok(self.buffer.len())
        }
    }
    pub fn buffered_events(&self) -> impl Iterator<Item = DeviceEvent32> + '_ {
        self.buffer.iter().copied()
    }
    pub fn read_buffer(
        &self,
        output: &mut [DeviceEvent32],
    ) -> Result<(usize, DirectInputResult32), DirectInputResult32> {
        let count = self.buffered()?.min(output.len());
        for (dst, event) in output[..count].iter_mut().zip(&self.buffer) {
            *dst = *event;
        }
        Ok((
            count,
            if self.overflow {
                DirectInputResult32::BUFFER_OVERFLOW
            } else {
                DirectInputResult32::OK
            },
        ))
    }
    pub fn consume_buffer(&mut self, count: usize) -> Result<(), DirectInputResult32> {
        if count > self.buffered()? {
            return Err(DirectInputResult32::INVALID_PARAM);
        }
        self.buffer.drain(..count);
        self.overflow = false;
        Ok(())
    }
    pub(super) fn emit(
        &mut self,
        object: DeviceObject32,
        value: u32,
        event: SourceEvent32,
        origin: u64,
    ) {
        if self.requested_buffer == 0 {
            return;
        }
        let capacity = (self.requested_buffer as usize)
            .min(self.buffer_limit)
            .saturating_sub(1);
        let Some(format) = self.format.as_ref() else {
            return;
        };
        for binding in format
            .bindings()
            .iter()
            .filter(|binding| binding.object == Some(object))
        {
            if self.buffer.len() == capacity {
                self.overflow = true;
                return;
            }
            self.buffer.push_back(DeviceEvent32 {
                offset: binding.offset,
                value,
                timestamp_ms: (event.timestamp_ns.saturating_sub(origin) / 1_000_000) as u32,
                sequence: event.input_sequence,
                app_data: binding.app_data,
            });
        }
    }
}
