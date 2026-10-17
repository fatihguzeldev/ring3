use super::{
    context::{Action32, ReferenceWork32, Stage32},
    cursor::ReferenceCursor32,
    requests::{invalid_state, preflight_references},
};
use crate::{
    process::windows::automation::variants::{Coerced32, InvokeParameters32, OwnedValuePlan32},
    process::{EngineInstance, HostError},
    windows::automation::{AutomationError32 as E, variants::Variant32},
};

pub(super) struct InvokeState32 {
    pub parameters: InvokeParameters32,
    pub temporaries: Vec<OwnedValuePlan32>,
    pub cleanup: Option<OwnedValuePlan32>,
    pub release: ReferenceCursor32,
    pub release_index: usize,
    pub depth: usize,
    pub result: u32,
}
impl InvokeState32 {
    pub(super) fn prepare(
        engine: &EngineInstance,
        parameters: InvokeParameters32,
    ) -> Result<Self, HostError> {
        let mut temporaries = Vec::new();
        temporaries
            .try_reserve_exact(engine.automation.limits.nesting)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        Ok(Self {
            parameters,
            temporaries,
            cleanup: Some(engine.plan_owned_variant(Variant32::EMPTY, Variant32::EMPTY, true)?),
            release: ReferenceCursor32::default(),
            release_index: 0,
            depth: 1,
            result: 0,
        })
    }
    pub(super) fn forget_temporaries(&mut self) {
        self.cleanup = None;
        self.temporaries.clear();
    }
}

impl EngineInstance {
    pub(super) fn accept_automation_invoke_result(
        &mut self,
        work: &mut ReferenceWork32,
        status: u32,
        returned: Variant32,
    ) -> Result<(), HostError> {
        let temporary = self.plan_owned_variant(Variant32::EMPTY, returned, true)?;
        self.preflight_retained_variant_plan(&temporary)?;
        self.capture_automation_temporary_objects(work, &temporary)?;
        let invoke = work.invoke.as_mut().ok_or_else(invalid_state)?;
        invoke
            .cleanup
            .as_mut()
            .ok_or_else(invalid_state)?
            .absorb_retirement(&temporary, self.automation.limits)
            .map_err(HostError::Automation)?;
        invoke.temporaries.push(temporary);
        work.stage = Stage32::InvokeConvert(status);
        Ok(())
    }

    pub(super) fn convert_automation_invoke_result(
        &mut self,
        work: &mut ReferenceWork32,
        status: u32,
    ) -> Result<(), HostError> {
        let invoke = work.invoke.as_ref().ok_or_else(invalid_state)?;
        let parameters = invoke.parameters;
        let returned = invoke.temporaries.last().ok_or_else(invalid_state)?.old();
        if status & 0x8000_0000 != 0 {
            work.invoke.as_mut().unwrap().result = E::TypeMismatch.hresult();
            work.stage = Stage32::ReleaseTemporaries;
            return Ok(());
        }
        let scratch = work.scratch.ok_or_else(invalid_state)?;
        scratch.check(self.memory()?).map_err(HostError::Memory)?;
        let at = scratch.address.0 + 32;
        if self.read_automation_variant(at, false)? != returned {
            return Err(invalid_state());
        }
        let source = self.read_automation_variant(at, true)?;
        if source.vartype() == 9
            && source.pointer() != 0
            && !matches!(parameters.target, 9 | 13)
            && parameters.flags & 1 == 0
        {
            if invoke.depth >= self.automation.limits.nesting {
                work.invoke.as_mut().unwrap().result = E::Capacity.hresult();
                work.stage = Stage32::ReleaseTemporaries;
                return Ok(());
            }
            let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
            work.objects.capture(memory, source.pointer())?;
            super::borrowed::BorrowedCall32::prepare(memory, source.pointer(), 6)?;
            memory
                .write(scratch.address, &[0; 48])
                .map_err(HostError::Memory)?;
            work.invoke.as_mut().unwrap().depth += 1;
            work.stage = Stage32::NeedCall {
                action: Action32::Invoke,
                object: source.pointer(),
            };
            return Ok(());
        }
        let converted = match self.coerce_automation_variant(
            source,
            parameters.locale,
            parameters.flags,
            parameters.target,
        ) {
            Ok(value) => value,
            Err(HostError::Automation(error))
                if !matches!(error, E::Memory(_) | E::Allocation | E::Capacity) =>
            {
                work.invoke.as_mut().unwrap().result = error.hresult();
                work.stage = Stage32::ReleaseTemporaries;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let mut plan = match converted {
            Coerced32::Value(value) => self.plan_owned_variant(value, work.plan.old(), false)?,
            Coerced32::String(bytes) => self.plan_owned_string(&bytes, work.plan.old())?,
        };
        plan.absorb_retirement(
            work.invoke
                .as_ref()
                .unwrap()
                .cleanup
                .as_ref()
                .ok_or_else(invalid_state)?,
            self.automation.limits,
        )
        .map_err(HostError::Automation)?;
        if let Some(objects) = preflight_references(self, &plan)? {
            work.objects.extend(self.memory()?, objects)?;
        }
        self.preflight_retained_variant_plan(&plan)?;
        work.plan = plan;
        work.stage = Stage32::Acquire;
        Ok(())
    }

    fn capture_automation_temporary_objects(
        &mut self,
        work: &mut ReferenceWork32,
        plan: &OwnedValuePlan32,
    ) -> Result<(), HostError> {
        let mut cursor = ReferenceCursor32::default();
        while let Some(object) =
            cursor.next(plan.old(), self.automation.limits.nesting, |pointer| {
                plan.old_array(pointer)
            })?
        {
            let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
            super::borrowed::BorrowedCall32::prepare(memory, object, 2)?;
            work.objects.capture(memory, object)?;
        }
        Ok(())
    }

    pub(super) fn next_automation_temporary_release(
        &mut self,
        work: &mut ReferenceWork32,
    ) -> Result<Option<u32>, HostError> {
        let invoke = work.invoke.as_mut().ok_or_else(invalid_state)?;
        while let Some(plan) = invoke.temporaries.get(invoke.release_index) {
            if let Some(object) =
                invoke
                    .release
                    .next(plan.old(), self.automation.limits.nesting, |pointer| {
                        plan.old_array(pointer)
                    })?
            {
                return Ok(Some(object));
            }
            invoke.release_index += 1;
            invoke.release = ReferenceCursor32::default();
        }
        Ok(None)
    }

    pub(super) fn retire_automation_temporary_values(
        &mut self,
        work: &mut ReferenceWork32,
    ) -> Result<(), HostError> {
        let invoke = work.invoke.as_mut().ok_or_else(invalid_state)?;
        if let Some(plan) = invoke.cleanup.as_mut() {
            self.retire_retained_variant_plan(plan)?;
            invoke.forget_temporaries();
        }
        Ok(())
    }
}
