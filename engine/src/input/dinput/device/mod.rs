mod acquisition;
mod buffer;
mod properties;
mod state;
mod synchronization;
#[cfg(test)]
mod tests;

use super::{
    DirectInputResult32,
    format::{DataFormat32, DeviceDescription32},
};
use crate::input::{
    RelativeTotals,
    source::{InputCursor32, SourceSnapshot32},
};
pub use acquisition::{Acquisition32, CooperativeLevel32};
pub use buffer::DeviceEvent32;
pub use properties::AxisProperties32;
use std::collections::VecDeque;

pub struct DirectInputDevice32 {
    pub description: DeviceDescription32,
    pub(super) format: Option<DataFormat32>,
    pub(super) cooperative: Option<CooperativeLevel32>,
    pub(super) acquisition: Acquisition32,
    pub(super) cursor: InputCursor32,
    pub(super) snapshot: SourceSnapshot32,
    pub(super) baseline: RelativeTotals,
    pub(super) controller_baseline: [i32; 32],
    pub(super) axes: [AxisProperties32; 32],
    pub(super) buffer: VecDeque<DeviceEvent32>,
    pub(super) buffer_limit: usize,
    pub(super) requested_buffer: u32,
    pub(super) overflow: bool,
    pub(super) gain: u32,
    pub(super) axis_relative: Option<bool>,
}
impl DirectInputDevice32 {
    pub fn new(
        description: DeviceDescription32,
        maximum_buffer: usize,
    ) -> Result<Self, DirectInputResult32> {
        description.validate()?;
        if maximum_buffer == 0 || maximum_buffer > 65536 {
            return Err(DirectInputResult32::INVALID_PARAM);
        }
        let mut buffer = VecDeque::new();
        buffer
            .try_reserve_exact(maximum_buffer)
            .map_err(|_| DirectInputResult32::OUT_OF_MEMORY)?;
        Ok(Self {
            description,
            format: None,
            cooperative: None,
            acquisition: Acquisition32::Unacquired,
            cursor: InputCursor32 {
                epoch: 0,
                sequence: 0,
            },
            snapshot: SourceSnapshot32::default(),
            baseline: RelativeTotals::default(),
            controller_baseline: [0; 32],
            axes: [AxisProperties32::default(); 32],
            buffer,
            buffer_limit: maximum_buffer,
            requested_buffer: 0,
            overflow: false,
            gain: 10000,
            axis_relative: None,
        })
    }
    pub fn format(&self) -> Option<&DataFormat32> {
        self.format.as_ref()
    }
    pub fn format_mut(&mut self) -> Result<&mut DataFormat32, DirectInputResult32> {
        self.check_unacquired()?;
        self.format
            .as_mut()
            .ok_or(DirectInputResult32::NOT_INITIALIZED)
    }
    pub fn set_format(&mut self, format: DataFormat32) -> Result<(), DirectInputResult32> {
        self.check_unacquired()?;
        self.format = Some(format);
        self.buffer.clear();
        self.overflow = false;
        self.axis_relative = None;
        Ok(())
    }
    pub fn relative(&self) -> bool {
        self.axis_relative
            .unwrap_or_else(|| self.format.as_ref().is_some_and(|format| format.relative))
    }
    pub fn epoch_changed(&mut self) {
        if self.acquisition == Acquisition32::Acquired {
            self.acquisition = Acquisition32::Lost;
        }
        self.buffer.clear();
        self.overflow = false;
        self.cursor = InputCursor32 {
            epoch: 0,
            sequence: 0,
        };
        self.snapshot = SourceSnapshot32::default();
        self.baseline = RelativeTotals::default();
        self.controller_baseline = [0; 32];
    }
}
