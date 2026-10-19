#[cfg(any(target_arch = "wasm32", test))]
use super::installation::ResidentInstallation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionFrameStart32 {
    pub token: u64,
    pub start_epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionReceipt32 {
    pub epoch: u64,
    pub owner: u64,
    pub slot: u32,
    pub total_retired: u32,
    pub child_retired: u32,
    pub reason: u32,
    pub kind: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Copy)]
pub(in crate::process) struct ExecutionFrame32 {
    pub(super) token: u64,
    start_epoch: u64,
    budget: u32,
    dispatch: bool,
    expected: Option<ResidentInstallation>,
    pub(super) last: Option<(u64, ResidentInstallation)>,
    sealed_retired: u32,
    sealed_work: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
mod control {
    use super::*;
    use crate::{
        abi::{
            arena::{CANCEL_OFFSET, EXIT_OFFSET},
            header,
            x86::{EXIT_SIZE, decode_exit},
        },
        cpu::ExitReason,
        process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    };

    impl EngineInstance {
        /// owns one synchronous invocation with direct engine helpers and pure clock imports.
        /// callers must not mutate raw architectural memory or invoke opaque callbacks inside it.
        pub fn begin_owned_execution_receipt(
            &mut self,
            key: u64,
            budget: u32,
            unit: u64,
            dispatch: bool,
        ) -> Result<ExecutionFrameStart32, HostError> {
            let frame = self.begin_execution_receipt(key, budget, unit, dispatch)?;
            self.enable_owned_execution(frame.token);
            Ok(frame)
        }

        /// admits one synchronous invocation whose generated children use only
        /// direct engine imports and pure clocks. raw context mutation, opaque
        /// callbacks and reentrant host processing are forbidden until finish.
        /// unsupported public transitions while an internal light seal is held
        /// return busy; callers must finish the receipt before servicing them.
        pub fn begin_compiled_execution_receipt(
            &mut self,
            key: u64,
            budget: u32,
            unit: u64,
            dispatch: bool,
        ) -> Result<ExecutionFrameStart32, HostError> {
            let frame = self.begin_owned_execution_receipt(key, budget, unit, dispatch)?;
            self.enable_compiled_execution(frame.token);
            Ok(frame)
        }

        pub fn begin_execution_receipt(
            &mut self,
            key: u64,
            budget: u32,
            unit: u64,
            dispatch: bool,
        ) -> Result<ExecutionFrameStart32, HostError> {
            self.guard_dispatch_entry(key)?;
            self.guard_detection_material_change()?;
            if self.execution_frame.is_some() {
                return Err(HostError::Call(CallError::Busy));
            }
            let installed = self
                .resident_installation_by_id(unit)
                .ok_or(HostError::InvalidArtifact)?;
            let token = self
                .execution_frame_issuer
                .checked_add(1)
                .ok_or(HostError::GenerationExhausted)?;
            let start_epoch = self.detection_epoch;
            self.execution_frame = Some(ExecutionFrame32 {
                token,
                start_epoch,
                budget,
                dispatch,
                expected: (!dispatch).then_some(installed),
                last: None,
                sealed_retired: 0,
                sealed_work: 0,
            });
            self.execution_frame_issuer = token;
            if !dispatch {
                // the dispatcher does this before each child; direct entries need the same work boundary.
                header::write_u32(
                    self.arena.as_mut().get_mut(),
                    crate::abi::arena::WORK_CONSUMED_OFFSET,
                    0,
                );
            }
            Ok(ExecutionFrameStart32 { token, start_epoch })
        }

        pub(in crate::process) fn expect_execution_lookup(
            &mut self,
            installed: Option<ResidentInstallation>,
        ) {
            if let Some(frame) = self.execution_frame.as_mut()
                && frame.dispatch
            {
                frame.expected = installed;
            }
        }

        pub(in crate::process) fn authorize_execution_child(
            &self,
            owner: PendingOwner,
        ) -> Result<Option<ResidentInstallation>, HostError> {
            let Some(frame) = &self.execution_frame else {
                return Ok(None);
            };
            let expected = frame.expected.ok_or(HostError::InvalidArtifact)?;
            let id = match owner {
                PendingOwner::Resident(id) | PendingOwner::ResidentCallback { unit_id: id, .. } => {
                    id
                }
                PendingOwner::Replacement(_) => return Err(HostError::InvalidArtifact),
            };
            if expected.unit_id != id
                || self
                    .resident_installations
                    .get(expected.slot as usize)
                    .copied()
                    .flatten()
                    != Some(expected)
            {
                return Err(HostError::InvalidArtifact);
            }
            Ok(Some(expected))
        }

        pub(in crate::process) fn entered_execution_child(
            &mut self,
            epoch: u64,
            installed: Option<ResidentInstallation>,
        ) {
            if let Some(frame) = self.execution_frame.as_mut() {
                frame.expected = None;
                frame.last = Some((epoch, installed.expect("authorized execution child")));
            }
            #[cfg(any(feature = "observability", test))]
            self.entered_execution_census_child(epoch);
        }

        pub(in crate::process) fn prepare_execution_progress(
            &self,
        ) -> Result<Option<(u32, u32)>, HostError> {
            let Some(frame) = self.execution_frame else {
                return Ok(None);
            };
            let retired = header::read_u32(self.arena(), EXIT_OFFSET + 20);
            let work = header::read_u32(self.arena(), crate::abi::arena::WORK_CONSUMED_OFFSET)
                .max(retired);
            let retired = frame
                .sealed_retired
                .checked_add(retired)
                .ok_or(HostError::Call(CallError::InvalidStop))?;
            let work = frame
                .sealed_work
                .checked_add(work)
                .filter(|work| *work <= frame.budget)
                .ok_or(HostError::Call(CallError::InvalidStop))?;
            Ok(Some((retired, work)))
        }

        pub(in crate::process) fn commit_execution_progress(
            &mut self,
            progress: Option<(u32, u32)>,
        ) {
            if let Some((retired, work)) = progress {
                let frame = self
                    .execution_frame
                    .as_mut()
                    .expect("prepared execution frame");
                frame.sealed_retired = retired;
                frame.sealed_work = work;
            }
        }

        pub(in crate::process::execution) fn execution_frame_pins(
            &self,
        ) -> impl Iterator<Item = u64> {
            self.execution_frame
                .iter()
                .flat_map(|frame| [frame.expected, frame.last.map(|(_, installed)| installed)])
                .flatten()
                .map(|installed| installed.unit_id)
        }

        pub fn finish_execution_receipt(
            &mut self,
            key: u64,
            token: u64,
        ) -> Result<ExecutionReceipt32, HostError> {
            self.memory()?;
            if key != self.key {
                return Err(HostError::InvalidArtifact);
            }
            let frame = self
                .execution_frame
                .filter(|frame| token != 0 && frame.token == token)
                .ok_or(HostError::Call(CallError::InvalidToken))?;
            // wrong key/token above is neutral. a valid-token terminal attempt
            // upgrades private continuation authority before void revocation,
            // even if a later terminal check fails and the pinned frame retries.
            let materialized = self.materialize_compiled_continuation();
            self.revoke_owned_execution();
            materialized?;
            let exit = decode_exit(&self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
                .map_err(|_| HostError::Call(CallError::InvalidStop))?;
            let total_retired = exit.retired;
            if total_retired > frame.budget {
                return Err(HostError::Call(CallError::InvalidStop));
            }
            let reason = header::read_u32(self.arena(), EXIT_OFFSET + 16);
            let receipt = if let Some((epoch, installed)) = frame.last {
                if epoch <= frame.start_epoch || epoch != self.detection_epoch {
                    return Err(HostError::Call(CallError::InvalidToken));
                }
                let detection = self.prepare_execution_terminal(
                    epoch,
                    total_retired,
                    frame.dispatch,
                    frame.sealed_retired,
                    frame.sealed_work,
                    frame.budget,
                )?;
                let owner = match detection.owner {
                    PendingOwner::Resident(id)
                    | PendingOwner::ResidentCallback { unit_id: id, .. } => id,
                    PendingOwner::Replacement(_) => return Err(HostError::InvalidArtifact),
                };
                if owner != installed.unit_id
                    || self.resident_installation_by_id(owner) != Some(installed)
                {
                    return Err(HostError::InvalidArtifact);
                }
                ExecutionReceipt32 {
                    epoch,
                    owner,
                    slot: installed.slot,
                    total_retired,
                    child_retired: detection.child_retired,
                    reason,
                    kind: detection.kind,
                }
            } else {
                if !frame.dispatch
                    || self.detection_epoch != frame.start_epoch
                    || total_retired != 0
                    || !matches!(exit.reason, ExitReason::Budget if frame.budget == 0)
                        && !(exit.reason == ExitReason::Cancelled
                            && header::read_u32(self.arena(), CANCEL_OFFSET) != 0)
                {
                    return Err(HostError::Call(CallError::InvalidStop));
                }
                ExecutionReceipt32 {
                    epoch: 0,
                    owner: 0,
                    slot: u32::MAX,
                    total_retired: 0,
                    child_retired: 0,
                    reason,
                    kind: 0,
                }
            };
            #[cfg(any(feature = "observability", test))]
            self.finished_execution_census_receipt(frame.token, receipt);
            // every fallible check precedes restoring the final child's sealed exit.
            if let Some((epoch, _)) = frame.last {
                // terminal preparation has validated this exact seal and all outer transformations.
                self.restore_execution_terminal(epoch);
            }
            self.execution_frame = None;
            Ok(receipt)
        }
    }
}

#[cfg(test)]
mod tests;
