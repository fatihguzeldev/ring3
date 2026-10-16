use super::Array32;
use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
    windows::automation::{
        ArrayBound32, ArrayLayout32, AutomationError32 as E, Oleaut32Api32 as Api,
    },
};
impl EngineInstance {
    pub(super) fn create_automation_array(
        &mut self,
        api: Api,
        args: &[u32],
    ) -> Result<u32, HostError> {
        let vector = api == Api::SafeArrayCreateVector;
        let mut bounds = Vec::new();
        if vector {
            bounds.push(ArrayBound32 {
                lower: args[1] as i32,
                count: args[2],
            });
        } else {
            let dimensions = args[1] as usize;
            if args[2] == 0 || dimensions == 0 || dimensions > self.automation.limits.dimensions {
                return Err(HostError::Automation(E::InvalidParameter));
            }
            self.memory()?
                .check_span(GuestAddress(args[2]), dimensions * 8, Access::Read)
                .map_err(HostError::Memory)?;
            bounds
                .try_reserve_exact(dimensions)
                .map_err(|_| HostError::Automation(E::Allocation))?;
            for dimension in 0..dimensions {
                let mut bytes = [0; 8];
                self.memory()?
                    .read(GuestAddress(args[2] + dimension as u32 * 8), &mut bytes)
                    .map_err(HostError::Memory)?;
                bounds.push(ArrayBound32 {
                    count: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
                    lower: i32::from_le_bytes(bytes[4..].try_into().unwrap()),
                });
            }
        }
        if args[0] > u16::MAX as u32 {
            return Err(HostError::Automation(E::BadVariantType));
        }
        let layout = ArrayLayout32::new(args[0] as u16, &bounds, vector, self.automation.limits)
            .map_err(HostError::Automation)?;
        if layout.owns_elements() {
            return Err(HostError::Automation(E::NotImplemented));
        }
        self.allocate_automation_array(layout, None, None)
    }
    pub(super) fn allocate_automation_array(
        &mut self,
        layout: ArrayLayout32,
        source: Option<&[u8]>,
        output: Option<u32>,
    ) -> Result<u32, HostError> {
        let size = layout.allocation_bytes().map_err(HostError::Automation)?;
        if self.automation.strings.len() + self.automation.arrays.len()
            >= self.automation.limits.objects
            || self
                .automation
                .bytes
                .checked_add(size)
                .is_none_or(|n| n > self.automation.limits.bytes)
        {
            return Err(HostError::Automation(E::Capacity));
        }
        self.automation
            .arrays
            .try_reserve(1)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        if let Some(output) = output {
            self.memory()?
                .check_span(GuestAddress(output), 4, Access::Write)
                .map_err(HostError::Memory)?;
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        bytes.resize(size, 0);
        let prepared = self.prepare_windows_local_initialized(size as u32)?;
        let base = prepared.address();
        let descriptor = base + 4;
        let data = if layout.data_bytes == 0 {
            0
        } else {
            base + layout.data_offset() as u32
        };
        bytes[..4].copy_from_slice(&(layout.vartype as u32).to_le_bytes());
        layout
            .write_descriptor(&mut bytes[4..], data, 0)
            .map_err(HostError::Automation)?;
        if let Some(source) = source {
            if source.len() != layout.data_bytes {
                return Err(HostError::Automation(E::InvalidParameter));
            }
            let at = layout.data_offset();
            bytes[at..at + source.len()].copy_from_slice(source);
        }
        let pointer = descriptor.to_le_bytes();
        let write = [(GuestAddress(output.unwrap_or(0)), pointer.as_slice())];
        self.commit_windows_local_initialized(
            prepared,
            &bytes,
            if output.is_some() { &write } else { &[] },
        )?;
        self.automation.bytes += size;
        self.automation.arrays.push(Array32 {
            base,
            descriptor,
            data,
            allocation_bytes: size,
            separate_data: false,
            layout,
        });
        Ok(descriptor)
    }
    pub(in crate::process::windows::automation) fn automation_array_index(
        &self,
        pointer: u32,
    ) -> Result<(usize, u32), HostError> {
        let index = self
            .automation
            .arrays
            .iter()
            .position(|a| a.descriptor == pointer)
            .ok_or(HostError::Automation(E::InvalidParameter))?;
        let array = &self.automation.arrays[index];
        let mut header = [0; 16];
        self.memory()?
            .read(GuestAddress(pointer), &mut header)
            .map_err(HostError::Memory)?;
        if u16::from_le_bytes(header[..2].try_into().unwrap()) as usize != array.layout.bounds.len()
            || u16::from_le_bytes(header[2..4].try_into().unwrap()) != array.layout.features
            || u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize
                != array.layout.element_bytes
            || u32::from_le_bytes(header[12..16].try_into().unwrap()) != array.data
        {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        for (dimension, bound) in array.layout.bounds.iter().rev().enumerate() {
            let mut bytes = [0; 8];
            self.memory()?
                .read(
                    GuestAddress(pointer + 16 + dimension as u32 * 8),
                    &mut bytes,
                )
                .map_err(HostError::Memory)?;
            if u32::from_le_bytes(bytes[..4].try_into().unwrap()) != bound.count
                || i32::from_le_bytes(bytes[4..].try_into().unwrap()) != bound.lower
            {
                return Err(HostError::Automation(E::InvalidParameter));
            }
        }
        Ok((index, u32::from_le_bytes(header[8..12].try_into().unwrap())))
    }
}
