use crate::{
    memory::GuestAddress,
    process::{
        EngineInstance, HostError, calls::call::PendingOwner,
        windows::callbacks::WindowsProviderOutcome32,
    },
    windows::{
        CallFrame32,
        automation::{AutomationError32 as E, Oleaut32Api32 as Api, variants::Variant32},
    },
};

impl EngineInstance {
    pub(in crate::process::windows::automation) fn automation_owned_array_outcome(
        &mut self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
        api: Api,
    ) -> Result<Option<WindowsProviderOutcome32>, HostError> {
        if let Some(continuation) = self.automation_reference_for_call(owner, token, frame)? {
            return self.advance_automation_reference(continuation).map(Some);
        }
        if !matches!(
            api,
            Api::SafeArrayGetElement
                | Api::SafeArrayPutElement
                | Api::SafeArrayCopy
                | Api::SafeArrayDestroy
                | Api::SafeArrayRedim
        ) || frame.arguments()[0] == 0
        {
            return Ok(None);
        }
        let args = frame.arguments();
        let (index, locks) = self.automation_array_index(args[0])?;
        let array = &self.automation.arrays[index];
        let kind = array.layout.vartype;
        if !matches!(kind, 9 | 12 | 13) {
            return Ok(None);
        }
        if array.exclusive
            || matches!(api, Api::SafeArrayDestroy | Api::SafeArrayRedim) && locks != 0
        {
            return Err(HostError::Automation(E::ArrayLocked));
        }
        let loan = self.prepare_automation_array_loan(
            args[0],
            !matches!(api, Api::SafeArrayDestroy | Api::SafeArrayRedim),
        )?;
        let array_value = Variant32::new(0x2000 | kind, &args[0].to_le_bytes());
        let (destination, plan) = match api {
            Api::SafeArrayGetElement | Api::SafeArrayPutElement => {
                if args[1] == 0 || args[2] == 0 && (kind == 12 || api == Api::SafeArrayGetElement) {
                    return Err(HostError::Automation(E::InvalidParameter));
                }
                let element = self.automation_array_element_address(index, args[1])?;
                let current = self.read_automation_array_value(kind, element)?;
                if api == Api::SafeArrayGetElement {
                    let mut plan = self.plan_owned_variant(current, Variant32::EMPTY, false)?;
                    if kind != 12 {
                        plan.pointer_output();
                    }
                    (Some(args[2]), plan)
                } else {
                    let value = if kind == 12 {
                        self.read_automation_variant(args[2], false)?
                    } else {
                        Variant32::new(kind, &args[2].to_le_bytes())
                    };
                    let mut plan = self.plan_owned_variant(value, current, false)?;
                    if kind != 12 {
                        plan.pointer_output();
                    }
                    (Some(element), plan)
                }
            }
            Api::SafeArrayCopy => {
                if args[1] == 0 {
                    return Err(HostError::Automation(E::InvalidParameter));
                }
                let mut plan = self.plan_owned_variant(array_value, Variant32::EMPTY, false)?;
                plan.pointer_output();
                (Some(args[1]), plan)
            }
            Api::SafeArrayDestroy => (
                None,
                self.plan_owned_variant(Variant32::EMPTY, array_value, true)?,
            ),
            Api::SafeArrayRedim => (
                Some(args[0]),
                self.plan_automation_array_redim(index, args[1])?,
            ),
            _ => unreachable!(),
        };
        // the source tree is frozen before the first guest reference method can run.
        self.preflight_retained_variant_plan(&plan)?;
        self.start_automation_array_reference(owner, token, frame, destination, plan, loan)
            .map(Some)
    }
    fn read_automation_array_value(&self, kind: u16, address: u32) -> Result<Variant32, HostError> {
        if kind == 12 {
            return self.read_automation_variant(address, false);
        }
        let mut bytes = [0; 4];
        self.memory()?
            .read(GuestAddress(address), &mut bytes)
            .map_err(HostError::Memory)?;
        Ok(Variant32::new(kind, &bytes))
    }
    pub(super) fn automation_array_element_address(
        &self,
        index: usize,
        indices: u32,
    ) -> Result<u32, HostError> {
        let array = &self.automation.arrays[index];
        let mut values = [0i32; 64];
        for (dimension, value) in values[..array.layout.bounds.len()].iter_mut().enumerate() {
            let mut bytes = [0; 4];
            self.memory()?
                .read(
                    GuestAddress(
                        indices
                            .checked_add(dimension as u32 * 4)
                            .ok_or(HostError::Automation(E::Overflow))?,
                    ),
                    &mut bytes,
                )
                .map_err(HostError::Memory)?;
            *value = i32::from_le_bytes(bytes);
        }
        let offset = array
            .layout
            .offset(&values[..array.layout.bounds.len()])
            .map_err(HostError::Automation)?;
        array
            .data
            .checked_add(offset as u32)
            .ok_or(HostError::Automation(E::Overflow))
    }
}
