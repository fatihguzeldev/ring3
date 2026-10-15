use std::collections::VecDeque;

use super::{ModuleInitializer32, ModuleInitializerKind32, ModuleOperation32};
use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET},
        x86::{
            EXIT_SIZE, STATE_SIZE, decode_exit, decode_state, encode_exit_v3, encode_state_current,
        },
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    loader::{LoadedModule32, ModuleError32, ModuleId32},
    memory::GuestAddress,
    process::{
        EngineInstance, HostError, calls::call::PendingCall, cpu::context::ExtendedCpuState,
    },
    windows::CallbackFrame32,
};

pub(super) const INITIALIZER_RETURN_ID: u32 = 0x0003_0001;

pub(super) struct SavedModuleContext {
    state: [u8; STATE_SIZE],
    exit: [u8; EXIT_SIZE],
    extended: ExtendedCpuState,
}

impl SavedModuleContext {
    pub fn capture(arena: &[u8]) -> Result<Self, HostError> {
        decode_state(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::Module(ModuleError32::InvalidRequest))?;
        decode_exit(&arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
            .map_err(|_| HostError::Module(ModuleError32::InvalidRequest))?;
        Ok(Self {
            state: arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]
                .try_into()
                .unwrap(),
            exit: arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
                .try_into()
                .unwrap(),
            extended: ExtendedCpuState::capture(arena)?,
        })
    }

    pub fn state(&self) -> Result<State32, HostError> {
        decode_state(&self.state).map_err(|_| HostError::Infrastructure)
    }

    pub fn restore(&self, arena: &mut [u8]) {
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE].copy_from_slice(&self.state);
        arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&self.exit);
        self.extended.restore(arena);
    }
}

pub(super) enum InitStep {
    Tls {
        module: ModuleId32,
        reason: u32,
        index: u32,
    },
    DllMain {
        module: ModuleId32,
        reason: u32,
        reserved: u32,
    },
    Attached(ModuleId32),
}

pub(super) enum LoadCompletion {
    Library,
    Export(u32),
}

pub(super) enum OperationPurpose {
    Load {
        root: ModuleId32,
        new_ids: Vec<ModuleId32>,
        initial: bool,
        completion: LoadCompletion,
        introduced_edges: Vec<(ModuleId32, ModuleId32)>,
    },
    Shutdown,
    Unload {
        root: ModuleId32,
        ids: Vec<ModuleId32>,
    },
    Rollback {
        ids: Vec<ModuleId32>,
        initial: bool,
        introduced_edges: Vec<(ModuleId32, ModuleId32)>,
    },
}

pub(super) struct ModuleOperation {
    pub purpose: OperationPurpose,
    pub steps: VecDeque<InitStep>,
    pub active: Option<ModuleInitializer32>,
    pub saved: SavedModuleContext,
    pub outer: Option<PendingCall>,
    pub initial_before: Option<SavedModuleContext>,
    pub last_returned: u64,
}

pub(super) struct PreparedInitializer {
    pub descriptor: ModuleInitializer32,
    pub frame: CallbackFrame32,
    state: [u8; STATE_SIZE],
    exit: [u8; EXIT_SIZE],
}

pub(super) struct InitializerCall32 {
    pub entry: u32,
    pub kind: ModuleInitializerKind32,
    pub reason: u32,
    pub reserved: u32,
}

impl PreparedInitializer {
    pub fn prepare(
        token: u64,
        module: &LoadedModule32,
        call: InitializerCall32,
        state: State32,
        return_pc: u32,
    ) -> Result<Self, HostError> {
        let frame = CallbackFrame32::prepare(
            state,
            call.entry,
            return_pc,
            &[module.base(), call.reason, call.reserved],
        )
        .map_err(|_| HostError::Module(ModuleError32::InvalidRequest))?;
        let descriptor = ModuleInitializer32 {
            token,
            module: module.id(),
            entry: call.entry,
            return_pc,
            return_id: INITIALIZER_RETURN_ID,
            return_esp: frame.return_esp(),
            reason: call.reason,
            kind: call.kind,
        };
        let mut state = [0; STATE_SIZE];
        let mut exit = [0; EXIT_SIZE];
        encode_state_current(frame.state(), &mut state).map_err(|_| HostError::Infrastructure)?;
        encode_exit_v3(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::NeedCode,
            },
            &mut exit,
        )
        .map_err(|_| HostError::Infrastructure)?;
        Ok(Self {
            descriptor,
            frame,
            state,
            exit,
        })
    }

    pub fn publish(&self, arena: &mut [u8]) {
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE].copy_from_slice(&self.state);
        arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&self.exit);
    }
}

pub(super) fn load_steps(
    order: &[ModuleId32],
    initial: bool,
) -> Result<VecDeque<InitStep>, ModuleError32> {
    let mut steps = VecDeque::new();
    steps
        .try_reserve(order.len() * 3)
        .map_err(|_| ModuleError32::Allocation)?;
    for module in order {
        steps.push_back(InitStep::Tls {
            module: *module,
            reason: 1,
            index: 0,
        });
        steps.push_back(InitStep::DllMain {
            module: *module,
            reason: 1,
            reserved: u32::from(initial),
        });
        steps.push_back(InitStep::Attached(*module));
    }
    Ok(steps)
}

impl EngineInstance {
    pub fn module_initializer(&self) -> Result<Option<ModuleInitializer32>, HostError> {
        self.memory()?;
        Ok(self
            .modules
            .operations
            .last()
            .and_then(|operation| operation.active))
    }

    pub fn finish_module_initializer(
        &mut self,
        token: u64,
    ) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        if self.call_cancelled() {
            return Err(HostError::Call(crate::process::CallError::Cancelled));
        }
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        let operation = self
            .modules
            .operations
            .last()
            .ok_or(HostError::Module(ModuleError32::InvalidToken))?;
        if operation.active.is_none() && token != 0 && operation.last_returned == token {
            let operation = self.modules.operations.pop().unwrap();
            return self.advance_module_operation(operation);
        }
        let active = operation
            .active
            .ok_or(HostError::Module(ModuleError32::InvalidToken))?;
        if token == 0 || active.token != token {
            return Err(HostError::Module(ModuleError32::InvalidToken));
        }
        let state = decode_state(&self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::Module(ModuleError32::InvalidReturn))?;
        let exit = decode_exit(&self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
            .map_err(|_| HostError::Module(ModuleError32::InvalidReturn))?;
        ExtendedCpuState::capture(self.arena())?;
        if state.eip != active.return_pc
            || state.registers[4] != active.return_esp
            || exit.reason
                != (ExitReason::Gate {
                    id: active.return_id,
                })
        {
            return Err(HostError::Module(ModuleError32::InvalidReturn));
        }
        if self.call_cancelled() {
            return Err(HostError::Call(crate::process::CallError::Cancelled));
        }
        let failed = active.kind == ModuleInitializerKind32::DllMain
            && active.reason == 1
            && state.registers[0] == 0;
        let mut operation = self.modules.operations.pop().unwrap();
        operation.active = None;
        operation.last_returned = token;
        if failed
            && let Err(error) = self.prepare_module_rollback(&mut operation, Some(active.module))
        {
            operation.active = Some(active);
            self.modules.operations.push(operation);
            return Err(error);
        }
        self.advance_module_operation(operation)
    }

    pub fn abort_module_operation(&mut self) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        let mut operation = self
            .modules
            .operations
            .pop()
            .ok_or(HostError::Module(ModuleError32::InvalidRequest))?;
        if let Err(error) = self.prepare_module_rollback(&mut operation, None) {
            self.modules.operations.push(operation);
            return Err(error);
        }
        operation.active = None;
        self.advance_module_operation(operation)
    }

    pub(super) fn advance_module_operation(
        &mut self,
        mut operation: ModuleOperation,
    ) -> Result<ModuleOperation32, HostError> {
        loop {
            let step = match operation.steps.front() {
                Some(step) => step,
                None => return self.complete_module_operation(operation),
            };
            let prepared = (|| {
                let (id, entry, kind, reason, reserved) = match *step {
                    InitStep::Attached(id) => {
                        if !self.modules.attached.contains(&id) {
                            self.modules.attached.push(id);
                        }
                        return Ok(None);
                    }
                    InitStep::Tls {
                        module,
                        reason,
                        index,
                    } => {
                        let loaded = self
                            .modules
                            .loaded
                            .get(module)
                            .ok_or(HostError::Module(ModuleError32::InvalidHandle))?;
                        let Some(tls) = loaded.tls() else {
                            return Ok(None);
                        };
                        let Some(array) = tls.callbacks_address else {
                            return Ok(None);
                        };
                        if index as usize >= self.modules.catalog.limits().parser.metadata_entries {
                            return Err(HostError::Module(ModuleError32::Capacity));
                        }
                        let address = index
                            .checked_mul(4)
                            .and_then(|offset| array.0.checked_add(offset))
                            .ok_or(HostError::Module(ModuleError32::InvalidRequest))?;
                        let mut bytes = [0; 4];
                        self.memory()?
                            .read(GuestAddress(address), &mut bytes)
                            .map_err(HostError::Memory)?;
                        let entry = u32::from_le_bytes(bytes);
                        if entry == 0 {
                            return Ok(None);
                        }
                        (
                            module,
                            entry,
                            ModuleInitializerKind32::TlsCallback,
                            reason,
                            0,
                        )
                    }
                    InitStep::DllMain {
                        module,
                        reason,
                        reserved,
                    } => {
                        let loaded = self
                            .modules
                            .loaded
                            .get(module)
                            .ok_or(HostError::Module(ModuleError32::InvalidHandle))?;
                        if !loaded.is_dll() {
                            return Ok(None);
                        }
                        let Some(entry) = loaded.entry() else {
                            return Ok(None);
                        };
                        (
                            module,
                            entry.0,
                            ModuleInitializerKind32::DllMain,
                            reason,
                            reserved,
                        )
                    }
                };
                let module = self
                    .modules
                    .loaded
                    .get(id)
                    .ok_or(HostError::Module(ModuleError32::InvalidHandle))?;
                let token = self
                    .modules
                    .next_initializer
                    .checked_add(1)
                    .ok_or(HostError::Module(ModuleError32::Capacity))?;
                let return_pc = self
                    .modules
                    .thread
                    .as_ref()
                    .ok_or(HostError::Infrastructure)?
                    .return_pc();
                PreparedInitializer::prepare(
                    token,
                    module,
                    InitializerCall32 {
                        entry,
                        kind,
                        reason,
                        reserved,
                    },
                    operation.saved.state()?,
                    return_pc,
                )
                .map(Some)
            })();
            let prepared = match prepared {
                Ok(prepared) => prepared,
                Err(error) => {
                    self.modules.operations.push(operation);
                    return Err(error);
                }
            };
            if let Some(prepared) = prepared {
                if let Err(error) = self
                    .memory
                    .as_mut()
                    .ok_or(HostError::Closed)?
                    .write_words32(prepared.frame.writes())
                {
                    self.modules.operations.push(operation);
                    return Err(HostError::Memory(error));
                }
                if let Some(InitStep::Tls { index, .. }) = operation.steps.front_mut() {
                    *index += 1;
                } else {
                    operation.steps.pop_front();
                }
                self.modules.next_initializer = prepared.descriptor.token;
                operation.saved.restore(self.arena.as_mut().get_mut());
                prepared.publish(self.arena.as_mut().get_mut());
                operation.active = Some(prepared.descriptor);
                self.modules.operations.push(operation);
                return Ok(ModuleOperation32::Pending(prepared.descriptor));
            }
            operation.steps.pop_front();
        }
    }
}
