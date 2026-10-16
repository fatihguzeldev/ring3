use super::{OwnedValuePlan32, Place32};
use crate::{
    memory::{Access, GuestAddress},
    process::{
        EngineInstance, HostError,
        windows::automation::{arrays::Array32, context::Bstr32},
    },
    windows::automation::AutomationError32 as E,
};

impl EngineInstance {
    pub(in crate::process::windows::automation) fn preflight_retained_variant_plan(
        &mut self,
        plan: &OwnedValuePlan32,
    ) -> Result<(), HostError> {
        self.check_owned_plan_capacity(plan)?;
        self.check_retained_variant_allocations(plan)?;
        if !plan.blocks.is_empty() || !plan.retire.is_empty() {
            let mut sizes = Vec::new();
            sizes
                .try_reserve_exact(plan.blocks.len())
                .map_err(|_| HostError::Automation(E::Allocation))?;
            sizes.extend(plan.blocks.iter().map(|block| block.len() as u32));
            self.prepare_windows_local_initialized_group(&sizes, &plan.retire)?;
        }
        Ok(())
    }
    pub(in crate::process::windows::automation) fn publish_retained_variant_plan(
        &mut self,
        destination: u32,
        plan: &mut OwnedValuePlan32,
    ) -> Result<(), HostError> {
        self.publish_owned_variant_plan(Some(destination), plan)
    }
    fn publish_owned_variant_plan(
        &mut self,
        destination: Option<u32>,
        plan: &mut OwnedValuePlan32,
    ) -> Result<(), HostError> {
        self.check_owned_plan_capacity(plan)?;
        self.check_retained_variant_allocations(plan)?;
        if let Some(destination) = destination {
            self.memory()?
                .check_span(GuestAddress(destination), plan.root.len(), Access::Write)
                .map_err(HostError::Memory)?;
        }
        if plan.blocks.is_empty() && plan.retire.is_empty() {
            return if let Some(destination) = destination {
                self.memory
                    .as_mut()
                    .unwrap()
                    .write(GuestAddress(destination), &plan.root)
                    .map_err(HostError::Memory)
            } else {
                Ok(())
            };
        }
        self.automation
            .strings
            .try_reserve(plan.strings.len())
            .map_err(|_| HostError::Automation(E::Allocation))?;
        self.automation
            .arrays
            .try_reserve(plan.arrays.len())
            .map_err(|_| HostError::Automation(E::Allocation))?;
        let mut sizes = Vec::new();
        sizes
            .try_reserve_exact(plan.blocks.len())
            .map_err(|_| HostError::Automation(E::Allocation))?;
        sizes.extend(plan.blocks.iter().map(|block| block.len() as u32));
        let mut strings = Vec::new();
        strings
            .try_reserve_exact(plan.strings.len())
            .map_err(|_| HostError::Automation(E::Allocation))?;
        let mut arrays = Vec::new();
        arrays
            .try_reserve_exact(plan.arrays.len())
            .map_err(|_| HostError::Automation(E::Allocation))?;
        let mut payloads = Vec::new();
        payloads
            .try_reserve_exact(plan.blocks.len())
            .map_err(|_| HostError::Automation(E::Allocation))?;
        let prepared = self.prepare_windows_local_initialized_group(&sizes, &plan.retire)?;
        let addresses = prepared.addresses();
        for &block in &plan.strings {
            strings.push(Bstr32 {
                pointer: addresses[block] + 4,
                bytes: plan.blocks[block].len(),
            });
        }
        for array in &plan.arrays {
            let base = addresses[array.block];
            let size = plan.blocks[array.block].len();
            let data = if array.layout.data_bytes == 0 {
                0
            } else {
                base + array.layout.data_offset() as u32
            };
            arrays.push(Array32 {
                base,
                descriptor: base + 4,
                data,
                allocation_bytes: size,
                separate_data: false,
                layout: crate::windows::automation::ArrayLayout32::new(
                    array.layout.vartype,
                    &array.layout.bounds,
                    array.layout.features & 0x2000 != 0,
                    self.automation.limits,
                )
                .map_err(HostError::Automation)?,
            });
        }
        for fixup in &plan.fixups {
            addresses[fixup.block]
                .checked_add(fixup.addend)
                .ok_or(HostError::Automation(E::Overflow))?;
        }
        for fixup in &plan.fixups {
            let pointer = addresses[fixup.block]
                .checked_add(fixup.addend)
                .ok_or(HostError::Automation(E::Overflow))?;
            let (bytes, offset) = match fixup.place {
                Place32::Root(offset) => (&mut plan.root, offset),
                Place32::Block { index, offset } => (&mut plan.blocks[index], offset),
            };
            bytes[offset..offset + 4].copy_from_slice(&pointer.to_le_bytes());
        }
        payloads.extend(plan.blocks.iter().map(Vec::as_slice));
        let output =
            destination.map(|destination| [(GuestAddress(destination), plan.root.as_slice())]);
        let result = self.commit_windows_local_initialized_group(
            prepared,
            &payloads,
            output.as_ref().map_or(&[], |output| output.as_slice()),
        );
        drop(payloads);
        if let Err(error) = result {
            // retained plans must keep original source pointers for callback retry and compensation.
            for fixup in &plan.fixups {
                let (bytes, offset) = match fixup.place {
                    Place32::Root(offset) => (&mut plan.root, offset),
                    Place32::Block { index, offset } => (&mut plan.blocks[index], offset),
                };
                bytes[offset..offset + 4].copy_from_slice(&fixup.original);
            }
            return Err(error);
        }
        self.automation
            .strings
            .retain(|row| plan.retire.binary_search(&(row.pointer - 4)).is_err());
        self.automation
            .arrays
            .retain(|row| plan.retire.binary_search(&row.base).is_err());
        self.automation.strings.extend(strings);
        self.automation.arrays.extend(arrays);
        self.automation.bytes = self.automation.bytes - plan.freed + plan.bytes;
        Ok(())
    }
    pub(in crate::process::windows::automation) fn check_retained_variant_allocations(
        &self,
        plan: &OwnedValuePlan32,
    ) -> Result<(), HostError> {
        for &(pointer, identity) in &plan.retire_identities {
            if self.windows_runtime_allocation_identity(pointer).ok() != Some(identity) {
                return Err(HostError::Call(crate::process::CallError::StateChanged));
            }
        }
        Ok(())
    }
}
