use super::Array32;
use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError, windows::automation::context::Bstr32},
    windows::automation::{ArrayLayout32, AutomationError32 as E, bstr_bytes},
};

pub(super) struct StringPayload32 {
    pub bytes: Vec<u8>,
}
impl EngineInstance {
    fn bstr_array_slots(&self, index: usize) -> Result<Vec<u32>, HostError> {
        let array = &self.automation.arrays[index];
        if array.layout.data_bytes != 0 {
            self.memory()?
                .check_span(
                    GuestAddress(array.data),
                    array.layout.data_bytes,
                    Access::Read,
                )
                .map_err(HostError::Memory)?;
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(array.layout.data_bytes)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        bytes.resize(array.layout.data_bytes, 0);
        if !bytes.is_empty() {
            self.memory()?
                .read(GuestAddress(array.data), &mut bytes)
                .map_err(HostError::Memory)?;
        }
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(bytes.len() / 4)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        slots.extend(
            bytes
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap())),
        );
        Ok(slots)
    }
    fn bstr_retirement(&self, slots: &[u32]) -> Result<(Vec<u32>, usize), HostError> {
        let mut pointers = Vec::new();
        let count = slots.iter().filter(|&&pointer| pointer != 0).count();
        if count > self.automation.strings.len() {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        pointers
            .try_reserve_exact(count)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        pointers.extend(slots.iter().copied().filter(|&p| p != 0));
        pointers.sort_unstable();
        if pointers.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        let mut freed = 0usize;
        for pointer in &mut pointers {
            let row = self
                .automation
                .strings
                .iter()
                .find(|row| row.pointer == *pointer)
                .ok_or(HostError::Automation(E::InvalidParameter))?;
            self.check_windows_runtime_bytes(row.pointer - 4, row.bytes as u32)?;
            freed = freed
                .checked_add(row.bytes)
                .ok_or(HostError::Automation(E::Capacity))?;
            *pointer -= 4;
        }
        Ok((pointers, freed))
    }
    fn commit_bstr_retirement(&mut self, pointers: &[u32], freed: usize) {
        self.automation
            .strings
            .retain(|row| pointers.binary_search(&(row.pointer - 4)).is_err());
        self.automation.bytes -= freed;
    }
    fn prepare_bstr_payload(&self, pointer: u32) -> Result<Option<StringPayload32>, HostError> {
        if pointer == 0 {
            return Ok(None);
        }
        let payload = self.read_automation_bstr(pointer)?;
        Ok(Some(StringPayload32 {
            bytes: bstr_bytes(&payload, self.automation.limits.bytes)
                .map_err(HostError::Automation)?,
        }))
    }
    fn check_bstr_group_capacity(
        &mut self,
        bytes: usize,
        strings: usize,
        retired: &[u32],
        freed: usize,
        new_array: bool,
    ) -> Result<(), HostError> {
        if self
            .automation
            .bytes
            .checked_sub(freed)
            .and_then(|n| n.checked_add(bytes))
            .is_none_or(|n| n > self.automation.limits.bytes)
            || self.automation.strings.len() + self.automation.arrays.len() - retired.len()
                + strings
                + usize::from(new_array)
                > self.automation.limits.objects
        {
            return Err(HostError::Automation(E::Capacity));
        }
        self.automation
            .strings
            .try_reserve(strings)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        self.automation
            .arrays
            .try_reserve(usize::from(new_array))
            .map_err(|_| HostError::Automation(E::Allocation))?;
        Ok(())
    }
    pub(super) fn bstr_array_element(
        &mut self,
        index: usize,
        offset: usize,
        input: u32,
        output: bool,
    ) -> Result<u32, HostError> {
        let element = self.automation.arrays[index]
            .data
            .checked_add(offset as u32)
            .ok_or(HostError::Automation(E::Overflow))?;
        let mut old = [0; 4];
        self.memory()?
            .read(GuestAddress(element), &mut old)
            .map_err(HostError::Memory)?;
        let old = u32::from_le_bytes(old);
        if output && input == 0 {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        let destination = if output { input } else { element };
        self.memory()?
            .check_span(GuestAddress(destination), 4, Access::Write)
            .map_err(HostError::Memory)?;
        let payload = if output && old == 0 {
            Some(StringPayload32 {
                bytes: bstr_bytes(&[], self.automation.limits.bytes)
                    .map_err(HostError::Automation)?,
            })
        } else {
            self.prepare_bstr_payload(if output { old } else { input })?
        };
        let (retired, freed) = self.bstr_retirement(if output {
            &[]
        } else {
            std::slice::from_ref(&old)
        })?;
        let bytes = payload.as_ref().map_or(0, |payload| payload.bytes.len());
        self.check_bstr_group_capacity(
            bytes,
            usize::from(payload.is_some()),
            &retired,
            freed,
            false,
        )?;
        let sizes = if payload.is_some() {
            vec![bytes as u32]
        } else {
            Vec::new()
        };
        let prepared = self.prepare_windows_local_initialized_group(&sizes, &retired)?;
        let pointer = prepared
            .addresses()
            .first()
            .map_or(0, |address| address + 4);
        let encoded = pointer.to_le_bytes();
        let payloads = payload
            .as_ref()
            .map(|payload| vec![payload.bytes.as_slice()])
            .unwrap_or_default();
        self.commit_windows_local_initialized_group(
            prepared,
            &payloads,
            &[(GuestAddress(destination), &encoded)],
        )?;
        self.commit_bstr_retirement(&retired, freed);
        if payload.is_some() {
            self.automation.strings.push(Bstr32 { pointer, bytes });
            self.automation.bytes += bytes;
        }
        Ok(0)
    }
    pub(super) fn copy_bstr_array(&mut self, index: usize, output: u32) -> Result<u32, HostError> {
        if output == 0 {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        self.memory()?
            .check_span(GuestAddress(output), 4, Access::Write)
            .map_err(HostError::Memory)?;
        let array = &self.automation.arrays[index];
        let layout = ArrayLayout32::new(
            8,
            &array.layout.bounds,
            array.layout.features & 0x2000 != 0,
            self.automation.limits,
        )
        .map_err(HostError::Automation)?;
        let slots = self.bstr_array_slots(index)?;
        let count = slots.iter().filter(|&&pointer| pointer != 0).count();
        let size = layout.allocation_bytes().map_err(HostError::Automation)?;
        self.check_bstr_group_capacity(size, count, &[], 0, true)?;
        let mut total = size;
        for &pointer in slots.iter().filter(|&&pointer| pointer != 0) {
            total = total
                .checked_add(
                    self.automation_bstr_size(pointer)?
                        .checked_add(6)
                        .ok_or(HostError::Automation(E::Capacity))?,
                )
                .ok_or(HostError::Automation(E::Capacity))?;
            if self
                .automation
                .bytes
                .checked_add(total)
                .is_none_or(|bytes| bytes > self.automation.limits.bytes)
            {
                return Err(HostError::Automation(E::Capacity));
            }
        }
        let mut strings = Vec::new();
        strings
            .try_reserve_exact(count)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        for (index, pointer) in slots
            .into_iter()
            .enumerate()
            .filter(|(_, pointer)| *pointer != 0)
        {
            strings.push((index, self.prepare_bstr_payload(pointer)?.unwrap()));
        }
        let mut sizes = Vec::new();
        sizes
            .try_reserve_exact(count + 1)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        sizes.push(size as u32);
        sizes.extend(strings.iter().map(|(_, string)| string.bytes.len() as u32));
        let prepared = self.prepare_windows_local_initialized_group(&sizes, &[])?;
        let base = prepared.addresses()[0];
        let descriptor = base + 4;
        let data = if layout.data_bytes == 0 {
            0
        } else {
            base + layout.data_offset() as u32
        };
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        bytes.resize(size, 0);
        bytes[..4].copy_from_slice(&8u32.to_le_bytes());
        layout
            .write_descriptor(&mut bytes[4..], data, 0)
            .map_err(HostError::Automation)?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(count)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        for (position, (index, string)) in strings.iter().enumerate() {
            let pointer = prepared.addresses()[position + 1] + 4;
            rows.push(Bstr32 {
                pointer,
                bytes: string.bytes.len(),
            });
            let at = layout.data_offset() + index * 4;
            bytes[at..at + 4].copy_from_slice(&pointer.to_le_bytes());
        }
        let mut payloads = Vec::new();
        payloads
            .try_reserve_exact(count + 1)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        payloads.push(bytes.as_slice());
        payloads.extend(strings.iter().map(|(_, string)| string.bytes.as_slice()));
        self.commit_windows_local_initialized_group(
            prepared,
            &payloads,
            &[(GuestAddress(output), &descriptor.to_le_bytes())],
        )?;
        self.automation.strings.extend(rows);
        self.automation.bytes += total;
        self.automation.arrays.push(Array32 {
            base,
            descriptor,
            data,
            allocation_bytes: size,
            separate_data: false,
            borrows: 0,
            exclusive: false,
            layout,
        });
        Ok(0)
    }
    pub(super) fn destroy_bstr_array(&mut self, index: usize) -> Result<u32, HostError> {
        let slots = self.bstr_array_slots(index)?;
        let (mut retired, freed) = self.bstr_retirement(&slots)?;
        let array = &self.automation.arrays[index];
        let size = array.allocation_bytes
            + if array.separate_data {
                array.layout.data_bytes
            } else {
                0
            };
        retired
            .try_reserve(2)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        retired.push(array.base);
        if array.separate_data {
            retired.push(array.data);
        }
        retired.sort_unstable();
        let prepared = self.prepare_windows_local_initialized_group(&[], &retired)?;
        self.commit_windows_local_initialized_group(prepared, &[], &[])?;
        self.commit_bstr_retirement(&retired, freed);
        self.automation.arrays.remove(index);
        self.automation.bytes -= size;
        Ok(0)
    }
}

impl EngineInstance {
    pub(super) fn redim_bstr_array(&mut self, index: usize, input: u32) -> Result<u32, HostError> {
        if input == 0 {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        let mut bound = [0; 8];
        self.memory()?
            .read(GuestAddress(input), &mut bound)
            .map_err(HostError::Memory)?;
        let bound = crate::windows::automation::ArrayBound32 {
            count: u32::from_le_bytes(bound[..4].try_into().unwrap()),
            lower: i32::from_le_bytes(bound[4..].try_into().unwrap()),
        };
        let array = &self.automation.arrays[index];
        let mut bounds = Vec::new();
        bounds
            .try_reserve_exact(array.layout.bounds.len())
            .map_err(|_| HostError::Automation(E::Allocation))?;
        bounds.extend_from_slice(&array.layout.bounds);
        *bounds.last_mut().unwrap() = bound;
        let layout = ArrayLayout32::new(
            8,
            &bounds,
            array.layout.features & 0x2000 != 0,
            self.automation.limits,
        )
        .map_err(HostError::Automation)?;
        let old_extra = if array.separate_data {
            array.layout.data_bytes
        } else {
            0
        };
        let old_data = array.data;
        let descriptor = array.descriptor;
        self.memory()?
            .check_span(
                GuestAddress(descriptor),
                layout.descriptor_bytes(),
                Access::Write,
            )
            .map_err(HostError::Memory)?;
        let slots = self.bstr_array_slots(index)?;
        let retained = (layout.data_bytes / 4).min(slots.len());
        let (mut retired, freed) = self.bstr_retirement(&slots[retained..])?;
        if slots[..retained]
            .iter()
            .copied()
            .filter(|&p| p != 0)
            .any(|p| retired.binary_search(&(p - 4)).is_ok())
        {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        self.check_bstr_group_capacity(layout.data_bytes, 0, &retired, freed + old_extra, false)?;
        if old_extra != 0 {
            retired
                .try_reserve(1)
                .map_err(|_| HostError::Automation(E::Allocation))?;
            retired.push(old_data);
        }
        retired.sort_unstable();
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(layout.data_bytes)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        bytes.resize(layout.data_bytes, 0);
        for (index, &pointer) in slots[..retained].iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&pointer.to_le_bytes());
        }
        let sizes = if bytes.is_empty() {
            Vec::new()
        } else {
            vec![bytes.len() as u32]
        };
        let prepared = self.prepare_windows_local_initialized_group(&sizes, &retired)?;
        let data = prepared.addresses().first().copied().unwrap_or(0);
        let mut header = [0; 528];
        layout
            .write_descriptor(&mut header, data, 0)
            .map_err(HostError::Automation)?;
        let payloads = [bytes.as_slice()];
        self.commit_windows_local_initialized_group(
            prepared,
            if bytes.is_empty() { &[] } else { &payloads },
            &[(
                GuestAddress(descriptor),
                &header[..layout.descriptor_bytes()],
            )],
        )?;
        self.commit_bstr_retirement(&retired, freed);
        self.automation.bytes = self.automation.bytes - old_extra + bytes.len();
        let array = &mut self.automation.arrays[index];
        array.data = data;
        array.separate_data = !bytes.is_empty();
        array.layout = layout;
        Ok(0)
    }
}
