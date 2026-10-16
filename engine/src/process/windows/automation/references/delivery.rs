use super::{
    borrowed::BorrowedCall32,
    context::{Action32, ReferenceWork32, Stage32},
    requests::invalid_state,
};
use crate::{
    memory::GuestAddress,
    process::windows::callbacks::{
        WindowsCallback32, WindowsCallbackProvider32, WindowsProviderOutcome32,
    },
    process::{EngineInstance, HostError},
    windows::automation::{AutomationError32 as E, variants::Variant32},
};

impl EngineInstance {
    pub(in crate::process) fn resume_automation_callback(
        &mut self,
        continuation: u64,
        result: u32,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        let thread = self.current_windows_thread_id()?;
        let work = self
            .automation
            .references
            .iter_mut()
            .find(|work| work.continuation == continuation && work.thread == thread)
            .ok_or(HostError::InvalidRequest)?;
        if let Stage32::Calling { action, .. } = work.stage {
            work.stage = Stage32::Received { action, result };
        }
        self.advance_automation_reference(continuation)
    }

    pub(super) fn advance_automation_reference(
        &mut self,
        continuation: u64,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        let thread = self.current_windows_thread_id()?;
        let index = self
            .automation
            .references
            .iter()
            .position(|work| work.continuation == continuation && work.thread == thread)
            .ok_or(HostError::InvalidRequest)?;
        let mut work = self.automation.references.remove(index);
        let outcome = self.advance_automation_reference_work(&mut work);
        if !matches!(outcome, Ok(WindowsProviderOutcome32::Return(_))) {
            self.automation.references.insert(index, work);
        }
        outcome
    }

    fn advance_automation_reference_work(
        &mut self,
        work: &mut ReferenceWork32,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        loop {
            match work.stage {
                Stage32::Acquire | Stage32::Release => {
                    let acquire = matches!(work.stage, Stage32::Acquire);
                    let cursor = if acquire {
                        &mut work.acquire
                    } else {
                        &mut work.release
                    };
                    let object = cursor.next(
                        if acquire {
                            work.plan.source()
                        } else {
                            work.plan.old()
                        },
                        self.automation.limits.nesting,
                        |pointer| {
                            if acquire {
                                work.plan.source_array(pointer)
                            } else {
                                work.plan.old_array(pointer)
                            }
                        },
                    )?;
                    work.stage = if let Some(object) = object {
                        Stage32::NeedCall {
                            action: if acquire {
                                Action32::Acquire
                            } else {
                                Action32::Release
                            },
                            object,
                        }
                    } else if acquire {
                        Stage32::Release
                    } else {
                        Stage32::Publish
                    };
                }
                Stage32::NeedCall { action, object } => {
                    if !matches!(action, Action32::Compensate) && !check_output(self, work)? {
                        continue;
                    }
                    if matches!(action, Action32::Query(_)) {
                        work.scratch
                            .ok_or_else(invalid_state)?
                            .check(self.memory()?)
                            .map_err(HostError::Memory)?;
                    }
                    work.objects.check(self.memory()?, object)?;
                    let slot = match action {
                        Action32::Acquire => 1,
                        Action32::Release | Action32::Compensate => 2,
                        Action32::Query(_) => 0,
                    };
                    let call = BorrowedCall32::prepare(
                        self.memory.as_mut().ok_or(HostError::Closed)?,
                        object,
                        slot,
                    )?;
                    work.stage = Stage32::Calling { action, call };
                }
                Stage32::Calling { action, call } => {
                    let mut arguments = [0; 16];
                    arguments[0] = call.object;
                    let count = if let Action32::Query(_) = action {
                        let scratch = work.scratch.ok_or_else(invalid_state)?;
                        arguments[1] = scratch.address.0;
                        arguments[2] = scratch.address.0 + 16;
                        3
                    } else {
                        1
                    };
                    return Ok(WindowsProviderOutcome32::Callback(WindowsCallback32 {
                        provider: WindowsCallbackProvider32::Automation,
                        continuation: work.continuation,
                        procedure: GuestAddress(call.procedure),
                        arguments,
                        count,
                    }));
                }
                Stage32::Received {
                    action: Action32::Acquire,
                    ..
                } => {
                    work.acquired += 1;
                    work.stage = Stage32::Acquire;
                }
                Stage32::Received {
                    action: Action32::Release,
                    ..
                } => work.stage = Stage32::Release,
                Stage32::Received {
                    action: Action32::Compensate,
                    ..
                } => {
                    if work.rollback_remaining != 0 {
                        work.rollback_remaining -= 1;
                    } else {
                        work.query_owned = None;
                    }
                    work.stage = Stage32::Rollback;
                }
                Stage32::Received {
                    action: Action32::Query(target),
                    result,
                } => {
                    if result & 0x8000_0000 != 0 {
                        work.stage = Stage32::Complete(result);
                    } else {
                        let scratch = work.scratch.ok_or_else(invalid_state)?;
                        scratch.check(self.memory()?).map_err(HostError::Memory)?;
                        let mut pointer = [0; 4];
                        self.memory()?
                            .read(GuestAddress(scratch.address.0 + 16), &mut pointer)
                            .map_err(HostError::Memory)?;
                        let pointer = u32::from_le_bytes(pointer);
                        if pointer == 0 {
                            work.stage = Stage32::Complete(E::Unexpected.hresult());
                        } else {
                            work.query_owned = Some(pointer);
                            work.stage = Stage32::QueryOwned(target);
                        }
                    }
                }
                Stage32::QueryOwned(target) => {
                    let pointer = work.query_owned.ok_or_else(invalid_state)?;
                    let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
                    work.objects.capture(memory, pointer)?;
                    BorrowedCall32::prepare(memory, pointer, 2)?;
                    work.plan
                        .acquired_root(Variant32::new(target, &pointer.to_le_bytes()));
                    work.stage = Stage32::Acquire;
                }
                Stage32::Rollback => {
                    let object = if work.rollback_remaining != 0 {
                        work.rollback
                            .next(
                                work.plan.source(),
                                self.automation.limits.nesting,
                                |pointer| work.plan.source_array(pointer),
                            )?
                            .ok_or_else(invalid_state)?
                    } else if let Some(object) = work.query_owned {
                        object
                    } else {
                        retire_scratch(self, work)?;
                        work.stage = Stage32::Failed(work.failure.ok_or_else(invalid_state)?);

                        continue;
                    };
                    work.stage = Stage32::NeedCall {
                        action: Action32::Compensate,
                        object,
                    };
                }
                Stage32::Publish => {
                    if !check_output(self, work)? {
                        continue;
                    }
                    retire_scratch(self, work)?;
                    self.publish_retained_variant_plan(work.destination, &mut work.plan)?;
                    work.stage = Stage32::Complete(0);
                }
                Stage32::Failed(error) => return Err(error),
                Stage32::Complete(result) => {
                    retire_scratch(self, work)?;
                    return Ok(WindowsProviderOutcome32::Return(result));
                }
            }
        }
    }
}

fn retire_scratch(
    engine: &mut EngineInstance,
    work: &mut ReferenceWork32,
) -> Result<(), HostError> {
    if let Some(scratch) = work.scratch {
        scratch
            .release(engine.memory.as_mut().ok_or(HostError::Closed)?)
            .map_err(HostError::Memory)?;
        work.scratch = None;
    }
    Ok(())
}

fn check_output(engine: &EngineInstance, work: &mut ReferenceWork32) -> Result<bool, HostError> {
    match work
        .output
        .check(engine.memory()?)
        .and_then(|()| engine.check_retained_variant_allocations(&work.plan))
    {
        Ok(()) => Ok(true),
        Err(error @ HostError::Call(crate::process::CallError::StateChanged)) => {
            work.failure = Some(error);
            work.rollback_remaining = work.acquired;
            work.rollback = super::cursor::ReferenceCursor32::default();
            work.stage = Stage32::Rollback;
            Ok(false)
        }
        Err(error) => Err(error),
    }
}
