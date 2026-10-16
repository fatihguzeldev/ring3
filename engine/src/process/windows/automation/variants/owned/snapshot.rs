use super::{ArrayCopy32, OwnedValuePlan32, Place32};
use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
    windows::automation::{ArrayLayout32, AutomationError32 as E, bstr_bytes, variants::Variant32},
};

impl EngineInstance {
    pub(in crate::process::windows::automation) fn plan_owned_string(
        &self,
        payload: &[u8],
        old: Variant32,
    ) -> Result<OwnedValuePlan32, HostError> {
        let mut plan =
            OwnedValuePlan32::new(&Variant32::new(8, &[]).0).map_err(HostError::Automation)?;
        plan.source = Variant32::EMPTY;
        plan.old = old;
        self.retire_variant_tree(&mut plan, old, 0)?;
        let size = payload
            .len()
            .checked_add(6)
            .ok_or(HostError::Automation(E::Capacity))?;
        let block = plan
            .reserve_block(size, self.automation.limits)
            .map_err(HostError::Automation)?;
        self.check_owned_plan_capacity(&plan)?;
        plan.blocks[block] =
            bstr_bytes(payload, self.automation.limits.bytes).map_err(HostError::Automation)?;
        plan.strings
            .try_reserve(1)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        plan.strings.push(block);
        plan.fixup(Place32::Root(8), block, 4)
            .map_err(HostError::Automation)?;
        plan.retire.sort_unstable();
        plan.retire_identities
            .try_reserve_exact(plan.retire.len())
            .map_err(|_| HostError::Automation(E::Allocation))?;
        for &pointer in &plan.retire {
            plan.retire_identities
                .push((pointer, self.windows_runtime_allocation_identity(pointer)?));
        }
        Ok(plan)
    }

    pub(in crate::process::windows::automation) fn plan_owned_variant(
        &self,
        value: Variant32,
        old: Variant32,
        clear: bool,
    ) -> Result<OwnedValuePlan32, HostError> {
        let mut plan = OwnedValuePlan32::new(&value.0).map_err(HostError::Automation)?;
        plan.old = old;
        if clear {
            plan.source = Variant32::EMPTY;
        }
        self.retire_variant_tree(&mut plan, old, 0)?;
        if !clear {
            self.clone_variant_tree(&mut plan, value, Place32::Root(8), &mut [0; 128], 0)?;
        }
        self.check_owned_plan_capacity(&plan)?;
        plan.retire.sort_unstable();
        plan.retire_identities
            .try_reserve_exact(plan.retire.len())
            .map_err(|_| HostError::Automation(E::Allocation))?;
        for &pointer in &plan.retire {
            plan.retire_identities
                .push((pointer, self.windows_runtime_allocation_identity(pointer)?));
        }
        Ok(plan)
    }

    fn clone_variant_tree(
        &self,
        plan: &mut OwnedValuePlan32,
        value: Variant32,
        place: Place32,
        ancestors: &mut [u32; 128],
        depth: usize,
    ) -> Result<(), HostError> {
        value.validate().map_err(HostError::Automation)?;
        if value.by_ref() {
            return Ok(());
        }
        if value.array() {
            if value.pointer() == 0 {
                return Ok(());
            }
            self.clone_array_tree(plan, value.pointer(), place, ancestors, depth)?;
        } else if value.base() == 8 {
            let size = self
                .automation_bstr_size(value.pointer())?
                .checked_add(6)
                .ok_or(HostError::Automation(E::Capacity))?;
            let block = plan
                .reserve_block(size, self.automation.limits)
                .map_err(HostError::Automation)?;
            self.check_owned_plan_capacity(plan)?;
            let payload = self.read_automation_bstr(value.pointer())?;
            plan.blocks[block] = bstr_bytes(&payload, self.automation.limits.bytes)
                .map_err(HostError::Automation)?;
            plan.strings
                .try_reserve(1)
                .map_err(|_| HostError::Automation(E::Allocation))?;
            plan.strings.push(block);
            plan.fixup(place, block, 4).map_err(HostError::Automation)?;
        }
        Ok(())
    }

    fn clone_array_tree(
        &self,
        plan: &mut OwnedValuePlan32,
        pointer: u32,
        place: Place32,
        ancestors: &mut [u32; 128],
        depth: usize,
    ) -> Result<(), HostError> {
        if depth >= self.automation.limits.nesting || ancestors[..depth].contains(&pointer) {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        ancestors[depth] = pointer;
        let (index, _) = self.automation_array_index(pointer)?;
        let source = &self.automation.arrays[index];
        let layout = ArrayLayout32::new(
            source.layout.vartype,
            &source.layout.bounds,
            source.layout.features & 0x2000 != 0,
            self.automation.limits,
        )
        .map_err(HostError::Automation)?;
        let size = layout.allocation_bytes().map_err(HostError::Automation)?;
        self.memory()?
            .check_span(GuestAddress(source.data), layout.data_bytes, Access::Read)
            .map_err(HostError::Memory)?;
        let block = plan
            .reserve_block(size, self.automation.limits)
            .map_err(HostError::Automation)?;
        self.check_owned_plan_capacity(plan)?;
        plan.blocks[block][..4].copy_from_slice(&(u32::from(layout.vartype)).to_le_bytes());
        layout
            .write_descriptor(&mut plan.blocks[block][4..], 0, 0)
            .map_err(HostError::Automation)?;
        let start = layout.data_offset();
        if layout.data_bytes != 0 {
            self.memory()?
                .read(
                    GuestAddress(source.data),
                    &mut plan.blocks[block][start..start + layout.data_bytes],
                )
                .map_err(HostError::Memory)?;
            plan.fixup(
                Place32::Block {
                    index: block,
                    offset: 16,
                },
                block,
                start as u32,
            )
            .map_err(HostError::Automation)?;
        }
        plan.fixup(place, block, 4).map_err(HostError::Automation)?;
        if layout.owns_elements() {
            for offset in (0..layout.data_bytes).step_by(layout.element_bytes) {
                let at = start + offset;
                let (value, pointer_offset) = if layout.vartype == 12 {
                    (
                        Variant32(plan.blocks[block][at..at + 16].try_into().unwrap()),
                        8,
                    )
                } else {
                    (
                        Variant32::new(layout.vartype, &plan.blocks[block][at..at + 4]),
                        0,
                    )
                };
                self.clone_variant_tree(
                    plan,
                    value,
                    Place32::Block {
                        index: block,
                        offset: at + pointer_offset,
                    },
                    ancestors,
                    depth + 1,
                )?;
            }
        }
        plan.arrays
            .try_reserve(1)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        plan.arrays.push(ArrayCopy32 {
            source: pointer,
            block,
            layout,
        });
        Ok(())
    }

    fn retire_variant_tree(
        &self,
        plan: &mut OwnedValuePlan32,
        value: Variant32,
        depth: usize,
    ) -> Result<(), HostError> {
        value.validate().map_err(HostError::Automation)?;
        if value.by_ref() || value.pointer() == 0 {
            return Ok(());
        }
        if value.array() {
            if depth >= self.automation.limits.nesting {
                return Err(HostError::Automation(E::InvalidParameter));
            }
            let (index, locks) = self.automation_array_index(value.pointer())?;
            if locks != 0 {
                return Err(HostError::Automation(E::ArrayLocked));
            }
            let array = &self.automation.arrays[index];
            self.check_windows_runtime_bytes(array.base, array.allocation_bytes as u32)?;
            plan.retire_block(array.base, array.allocation_bytes, self.automation.limits)
                .map_err(HostError::Automation)?;
            if array.separate_data {
                self.check_windows_runtime_bytes(array.data, array.layout.data_bytes as u32)?;
                plan.retire_block(array.data, array.layout.data_bytes, self.automation.limits)
                    .map_err(HostError::Automation)?;
            }
            plan.retired_arrays += 1;
            let mut frozen = Vec::new();
            if matches!(array.layout.vartype, 9 | 12 | 13) {
                plan.retired_payload_bytes = plan
                    .retired_payload_bytes
                    .checked_add(array.layout.data_bytes)
                    .filter(|&bytes| bytes <= self.automation.limits.bytes)
                    .ok_or(HostError::Automation(E::Capacity))?;
                frozen
                    .try_reserve_exact(array.layout.data_bytes)
                    .map_err(|_| HostError::Automation(E::Allocation))?;
                frozen.resize(array.layout.data_bytes, 0);
                if !frozen.is_empty() {
                    self.memory()?
                        .read(GuestAddress(array.data), &mut frozen)
                        .map_err(HostError::Memory)?;
                }
            }
            if array.layout.owns_elements() {
                for offset in (0..array.layout.data_bytes).step_by(array.layout.element_bytes) {
                    let mut bytes = [0; 16];
                    if !frozen.is_empty() {
                        bytes[..array.layout.element_bytes]
                            .copy_from_slice(&frozen[offset..offset + array.layout.element_bytes]);
                    } else {
                        self.memory()?
                            .read(
                                GuestAddress(array.data + offset as u32),
                                &mut bytes[..array.layout.element_bytes],
                            )
                            .map_err(HostError::Memory)?;
                    }
                    let value = if array.layout.vartype == 12 {
                        Variant32(bytes)
                    } else {
                        Variant32::new(array.layout.vartype, &bytes[..4])
                    };
                    self.retire_variant_tree(plan, value, depth + 1)?;
                }
            }
            plan.retired_payloads
                .try_reserve(1)
                .map_err(|_| HostError::Automation(E::Allocation))?;
            plan.retired_payloads.push(super::RetiredArray32 {
                pointer: value.pointer(),
                vartype: array.layout.vartype,
                bytes: frozen,
            });
        } else if value.base() == 8 {
            let string = self
                .automation
                .strings
                .iter()
                .find(|row| row.pointer == value.pointer())
                .ok_or(HostError::Automation(E::InvalidParameter))?;
            self.check_windows_runtime_bytes(string.pointer - 4, string.bytes as u32)?;
            plan.retire_block(string.pointer - 4, string.bytes, self.automation.limits)
                .map_err(HostError::Automation)?;
            plan.retired_strings += 1;
        }
        Ok(())
    }

    pub(in crate::process::windows::automation) fn check_owned_plan_capacity(
        &self,
        plan: &OwnedValuePlan32,
    ) -> Result<(), HostError> {
        if self
            .automation
            .bytes
            .checked_sub(plan.freed)
            .and_then(|n| n.checked_add(plan.bytes))
            .is_none_or(|n| n > self.automation.limits.bytes)
            || self
                .automation
                .strings
                .len()
                .checked_add(self.automation.arrays.len())
                .and_then(|count| count.checked_sub(plan.retired_strings))
                .and_then(|count| count.checked_sub(plan.retired_arrays))
                .and_then(|count| count.checked_add(plan.blocks.len()))
                .is_none_or(|count| count > self.automation.limits.objects)
        {
            return Err(HostError::Automation(E::Capacity));
        }
        Ok(())
    }
}
