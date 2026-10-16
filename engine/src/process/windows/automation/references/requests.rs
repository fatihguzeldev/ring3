use super::{
    borrowed::{BorrowedCall32, ObjectWitnesses32, OutputWitness32},
    context::{Action32, ReferenceWork32, Stage32},
    cursor::ReferenceCursor32,
};
use crate::{
    process::{
        CallError, EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::{
            automation::variants::{OwnedValuePlan32, VariantOperation32},
            callbacks::{GuestCallbackScratch32, WindowsProviderOutcome32},
        },
    },
    windows::{
        CallFrame32,
        automation::{AutomationError32 as E, Oleaut32Api32 as Api},
    },
};

impl EngineInstance {
    pub(in crate::process::windows::automation) fn automation_variant_outcome(
        &mut self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
        api: Api,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        if let Some(continuation) = self.automation_reference_for_call(owner, token, frame)? {
            return self.advance_automation_reference(continuation);
        }
        let operation = self.prepare_automation_variant_operation(api, frame.arguments())?;
        let (destination, mut plan, query) = match operation {
            VariantOperation32::Complete(result) => {
                return Ok(WindowsProviderOutcome32::Return(result));
            }
            VariantOperation32::Publish { destination, plan } => (destination, plan, None),
            VariantOperation32::Query {
                destination,
                plan,
                source,
                target,
            } => (destination, plan, Some((source, target))),
        };
        let mut objects = preflight_references(self, &plan)?;
        if objects.is_none() && query.is_none() {
            self.publish_retained_variant_plan(destination, &mut plan)?;
            return Ok(WindowsProviderOutcome32::Return(0));
        }
        let mut objects = objects.take().unwrap_or_default();
        if let Some((source, _)) = query {
            objects.capture(self.memory.as_mut().ok_or(HostError::Closed)?, source)?;
            objects.reserve_query()?;
        }
        self.check_windows_callback_capacity()?;
        self.preflight_retained_variant_plan(&plan)?;
        self.validate_new_windows_reply(owner, token)?;
        if self.automation.references.len() >= self.windows_callback_work_capacity() {
            return Err(HostError::Automation(E::Capacity));
        }
        self.automation
            .references
            .try_reserve(1)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        let continuation = self
            .automation
            .next_reference
            .checked_add(1)
            .ok_or(HostError::Automation(E::Capacity))?;
        let output = OutputWitness32::prepare(
            self.memory.as_mut().ok_or(HostError::Closed)?,
            destination,
            16,
        )?;
        let (scratch, stage) = if let Some((source, target)) = query {
            BorrowedCall32::prepare(self.memory.as_mut().ok_or(HostError::Closed)?, source, 0)?;
            let mut bytes = [0; 20];
            if target == 9 {
                bytes[..4].copy_from_slice(&0x0002_0400u32.to_le_bytes());
            }
            bytes[8] = 0xc0;
            bytes[15] = 0x46;
            let bounds = self.automation_scratch_bounds();
            let scratch =
                GuestCallbackScratch32::allocate(self.memory.as_mut().unwrap(), bounds, &bytes)
                    .map_err(HostError::Memory)?;
            (
                Some(scratch),
                Stage32::NeedCall {
                    action: Action32::Query(target),
                    object: source,
                },
            )
        } else {
            (None, Stage32::Acquire)
        };
        let work = ReferenceWork32 {
            owner,
            token,
            frame,
            thread: self.current_windows_thread_id()?,
            continuation,
            destination,
            output,
            plan,
            acquire: ReferenceCursor32::default(),
            release: ReferenceCursor32::default(),
            rollback: ReferenceCursor32::default(),
            acquired: 0,
            rollback_remaining: 0,
            query_owned: None,
            failure: None,
            objects,
            stage,
            scratch,
        };
        self.automation.next_reference = continuation;
        self.automation.references.push(work);
        self.advance_automation_reference(continuation)
    }
    pub(in crate::process::windows::automation) fn automation_reference_for_call(
        &self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
    ) -> Result<Option<u64>, HostError> {
        let thread = self.current_windows_thread_id()?;
        let Some(work) = self
            .automation
            .references
            .iter()
            .find(|work| work.token == token)
        else {
            return Ok(None);
        };
        if work.owner != owner || work.frame != frame || work.thread != thread {
            return Err(invalid_state());
        }
        Ok(Some(work.continuation))
    }
}

pub(super) fn preflight_references(
    engine: &mut EngineInstance,
    plan: &OwnedValuePlan32,
) -> Result<Option<ObjectWitnesses32>, HostError> {
    let mut found = false;
    let mut objects = ObjectWitnesses32::default();
    for acquire in [true, false] {
        let mut cursor = ReferenceCursor32::default();
        while let Some(object) = cursor.next(
            if acquire { plan.source() } else { plan.old() },
            engine.automation.limits.nesting,
            |pointer| {
                if acquire {
                    plan.source_array(pointer)
                } else {
                    plan.old_array(pointer)
                }
            },
        )? {
            BorrowedCall32::prepare(
                engine.memory.as_mut().ok_or(HostError::Closed)?,
                object,
                if acquire { 1 } else { 2 },
            )?;
            if acquire {
                BorrowedCall32::prepare(
                    engine.memory.as_mut().ok_or(HostError::Closed)?,
                    object,
                    2,
                )?;
            }
            objects.capture(engine.memory.as_mut().ok_or(HostError::Closed)?, object)?;
            found = true;
        }
    }
    Ok(found.then_some(objects))
}

pub(super) fn invalid_state() -> HostError {
    HostError::Call(CallError::StateChanged)
}
