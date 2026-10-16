use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
    windows::automation::{
        ArrayBound32, ArrayLayout32, AutomationError32 as E, Oleaut32Api32 as Api,
    },
};
impl EngineInstance {
    pub(super) fn automation_array_element(
        &mut self,
        index: usize,
        api: Api,
        args: &[u32],
    ) -> Result<u32, HostError> {
        let array = &self.automation.arrays[index];
        if args[1] == 0
            || args[2] == 0 && (array.layout.vartype != 8 || api == Api::SafeArrayGetElement)
        {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        if array.layout.owns_elements() {
            return Err(HostError::Automation(E::NotImplemented));
        }
        let dimensions = array.layout.bounds.len();
        self.memory()?
            .check_span(GuestAddress(args[1]), dimensions * 4, Access::Read)
            .map_err(HostError::Memory)?;
        let mut indices = [0i32; 64];
        for (dimension, value) in indices[..dimensions].iter_mut().enumerate() {
            let mut bytes = [0; 4];
            self.memory()?
                .read(GuestAddress(args[1] + dimension as u32 * 4), &mut bytes)
                .map_err(HostError::Memory)?;
            *value = i32::from_le_bytes(bytes);
        }
        let offset = array
            .layout
            .offset(&indices[..dimensions])
            .map_err(HostError::Automation)?;
        let element = array
            .data
            .checked_add(offset as u32)
            .ok_or(HostError::Automation(E::Overflow))?;
        let mut value = [0; 16];
        let size = array.layout.element_bytes;
        let (from, to) = if api == Api::SafeArrayGetElement {
            (element, args[2])
        } else {
            (args[2], element)
        };
        self.memory()?
            .read(GuestAddress(from), &mut value[..size])
            .map_err(HostError::Memory)?;
        self.memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(to), &value[..size])
            .map_err(HostError::Memory)?;
        Ok(0)
    }
    pub(super) fn copy_automation_array(
        &mut self,
        index: usize,
        output: u32,
    ) -> Result<u32, HostError> {
        if output == 0 {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        self.memory()?
            .check_span(GuestAddress(output), 4, Access::Write)
            .map_err(HostError::Memory)?;
        let array = &self.automation.arrays[index];
        if array.layout.owns_elements() {
            return Err(HostError::Automation(E::NotImplemented));
        }
        let layout = ArrayLayout32::new(
            array.layout.vartype,
            &array.layout.bounds,
            array.layout.features & 0x2000 != 0,
            self.automation.limits,
        )
        .map_err(HostError::Automation)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(layout.data_bytes)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        bytes.resize(layout.data_bytes, 0);
        if !bytes.is_empty() {
            self.memory()?
                .read(GuestAddress(array.data), &mut bytes)
                .map_err(HostError::Memory)?;
        }
        self.allocate_automation_array(layout, Some(&bytes), Some(output))?;
        Ok(0)
    }
    pub(super) fn redim_automation_array(
        &mut self,
        index: usize,
        locks: u32,
        input: u32,
    ) -> Result<u32, HostError> {
        if input == 0 {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        if locks != 0 {
            return Err(HostError::Automation(E::ArrayLocked));
        }
        let mut bound = [0; 8];
        self.memory()?
            .read(GuestAddress(input), &mut bound)
            .map_err(HostError::Memory)?;
        let bound = ArrayBound32 {
            count: u32::from_le_bytes(bound[..4].try_into().unwrap()),
            lower: i32::from_le_bytes(bound[4..].try_into().unwrap()),
        };
        let array = &self.automation.arrays[index];
        if array.layout.owns_elements() {
            return Err(HostError::Automation(E::NotImplemented));
        }
        let mut bounds = Vec::new();
        bounds
            .try_reserve_exact(array.layout.bounds.len())
            .map_err(|_| HostError::Automation(E::Allocation))?;
        bounds.extend_from_slice(&array.layout.bounds);
        *bounds.last_mut().unwrap() = bound;
        let layout = ArrayLayout32::new(
            array.layout.vartype,
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
        if self
            .automation
            .bytes
            .checked_sub(old_extra)
            .and_then(|n| n.checked_add(layout.data_bytes))
            .is_none_or(|n| n > self.automation.limits.bytes)
        {
            return Err(HostError::Automation(E::Capacity));
        }
        self.memory()?
            .check_span(
                GuestAddress(array.descriptor),
                layout.descriptor_bytes(),
                Access::Write,
            )
            .map_err(HostError::Memory)?;
        if array.separate_data {
            self.check_windows_runtime_bytes(array.data, old_extra as u32)?;
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(layout.data_bytes)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        bytes.resize(layout.data_bytes, 0);
        let copy = layout.data_bytes.min(array.layout.data_bytes);
        if copy != 0 {
            self.memory()?
                .read(GuestAddress(array.data), &mut bytes[..copy])
                .map_err(HostError::Memory)?;
        }
        let descriptor = array.descriptor;
        let old_data = array.data;
        let old_separate = array.separate_data;
        let mut header = [0; 528];
        let data = if bytes.is_empty() {
            layout
                .write_descriptor(&mut header, 0, 0)
                .map_err(HostError::Automation)?;
            self.memory
                .as_mut()
                .unwrap()
                .write(
                    GuestAddress(descriptor),
                    &header[..layout.descriptor_bytes()],
                )
                .map_err(HostError::Memory)?;
            0
        } else {
            let prepared = self.prepare_windows_local_initialized(bytes.len() as u32)?;
            let data = prepared.address();
            layout
                .write_descriptor(&mut header, data, 0)
                .map_err(HostError::Automation)?;
            self.commit_windows_local_initialized(
                prepared,
                &bytes,
                &[(
                    GuestAddress(descriptor),
                    &header[..layout.descriptor_bytes()],
                )],
            )?;
            data
        };
        if old_separate {
            self.free_windows_runtime_bytes(old_data);
        }
        self.automation.bytes = self.automation.bytes - old_extra + layout.data_bytes;
        let array = &mut self.automation.arrays[index];
        array.data = data;
        array.separate_data = !bytes.is_empty();
        array.layout = layout;
        Ok(0)
    }
}
