use super::{OwnedValuePlan32, Place32, Redim32, RetiredArray32};
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::automation::{
        ArrayBound32, ArrayLayout32, AutomationError32 as E, variants::Variant32,
    },
};

impl EngineInstance {
    pub(in crate::process::windows::automation) fn plan_automation_array_redim(
        &self,
        index: usize,
        input: u32,
    ) -> Result<OwnedValuePlan32, HostError> {
        if input == 0 {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        let mut bytes = [0; 8];
        self.memory()?
            .read(GuestAddress(input), &mut bytes)
            .map_err(HostError::Memory)?;
        let bound = ArrayBound32 {
            count: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
            lower: i32::from_le_bytes(bytes[4..].try_into().unwrap()),
        };
        let array = &self.automation.arrays[index];
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
        let mut plan = OwnedValuePlan32::new(&Variant32::EMPTY.0).map_err(HostError::Automation)?;
        let copy = array.layout.data_bytes.min(layout.data_bytes);
        let removed = array.layout.data_bytes - copy;
        let mut suffix = Vec::new();
        suffix
            .try_reserve_exact(removed)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        suffix.resize(removed, 0);
        if removed != 0 {
            self.memory()?
                .read(GuestAddress(array.data + copy as u32), &mut suffix)
                .map_err(HostError::Memory)?;
            for bytes in suffix.chunks_exact(array.layout.element_bytes) {
                let value = if array.layout.vartype == 12 {
                    Variant32(bytes.try_into().unwrap())
                } else {
                    Variant32::new(array.layout.vartype, bytes)
                };
                self.retire_variant_tree(&mut plan, value, 0)?;
            }
            plan.old = Variant32::new(
                0x2000 | array.layout.vartype,
                &array.descriptor.to_le_bytes(),
            );
            plan.retired_payloads
                .try_reserve(1)
                .map_err(|_| HostError::Automation(E::Allocation))?;
            plan.retired_payload_bytes = plan
                .retired_payload_bytes
                .checked_add(removed)
                .filter(|&bytes| bytes <= self.automation.limits.bytes)
                .ok_or(HostError::Automation(E::Capacity))?;
            plan.retired_payloads.push(RetiredArray32 {
                pointer: array.descriptor,
                vartype: array.layout.vartype,
                bytes: suffix,
            });
        }
        if array.separate_data {
            self.check_windows_runtime_bytes(array.data, array.layout.data_bytes as u32)?;
            plan.retire_block(array.data, array.layout.data_bytes, self.automation.limits)
                .map_err(HostError::Automation)?;
        }
        let block = if layout.data_bytes != 0 {
            let block = plan
                .reserve_block(layout.data_bytes, self.automation.limits)
                .map_err(HostError::Automation)?;
            if copy != 0 {
                self.memory()?
                    .read(GuestAddress(array.data), &mut plan.blocks[block][..copy])
                    .map_err(HostError::Memory)?;
            }
            Some(block)
        } else {
            None
        };
        plan.root
            .try_reserve(layout.descriptor_bytes().saturating_sub(plan.root.len()))
            .map_err(|_| HostError::Automation(E::Allocation))?;
        plan.root.resize(layout.descriptor_bytes(), 0);
        layout
            .write_descriptor(&mut plan.root, 0, 0)
            .map_err(HostError::Automation)?;
        plan.output = 0..plan.root.len();
        if let Some(block) = block {
            plan.fixup(Place32::Root(12), block, 0)
                .map_err(HostError::Automation)?;
        }
        plan.redim = Some(Redim32 {
            descriptor: array.descriptor,
            layout,
            block,
        });
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
}
