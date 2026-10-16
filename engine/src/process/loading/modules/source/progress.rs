use super::super::ModuleOperation32;
use super::planning::ReadyModuleSource32;
use super::{CapturedModuleRequest32, PendingModuleLoad32};
use crate::{
    files::FileIo32,
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub(in crate::process) fn begin_module_source_request(
        &mut self,
        request: CapturedModuleRequest32,
    ) -> Result<ModuleOperation32, HostError> {
        if self.modules.source.is_some() {
            return Err(HostError::Call(crate::process::CallError::Busy));
        }
        if !matches!(request, CapturedModuleRequest32::DataFile { .. }) {
            self.ensure_module_activation_runtime()?;
        }
        self.refresh_module_file_sources()?;
        let token = self
            .modules
            .next_source
            .checked_add(1)
            .ok_or(HostError::Infrastructure)?;
        let manifest = match &request {
            CapturedModuleRequest32::Load { name, .. } => Some(name.clone()),
            CapturedModuleRequest32::Export { .. } | CapturedModuleRequest32::DataFile { .. } => {
                None
            }
        };
        let source = PendingModuleLoad32 {
            token,
            thread: self.current_windows_thread_id()?,
            context: super::super::initialization::SavedModuleContext::capture(self.arena())?,
            request,
            bytes: None,
            input: None,
            leases: Vec::new(),
            works: Some(Vec::new()),
            manifest,
            header_identities: Vec::new(),
            application_directory: None,
            error: None,
            call_token: self.pending_call.as_ref().map(|call| call.token),
            default_graph: self.capture_module_process_default_graph()?,
        };
        self.modules.next_source = token;
        self.advance_module_source(source)
    }
    pub(super) fn advance_module_source(
        &mut self,
        mut source: PendingModuleLoad32,
    ) -> Result<ModuleOperation32, HostError> {
        source.bytes = None;
        let outcome = if let Some(error) = source.error {
            Err(error)
        } else {
            self.poll_module_source(&mut source)
        };
        match outcome {
            Ok(FileIo32::NeedBytes(request)) => {
                source.bytes = Some(request);
                self.modules.source = Some(source);
                Ok(ModuleOperation32::NeedBytes { request })
            }
            Ok(FileIo32::Ready(ready)) => {
                let result = (|| {
                    let leases = self.module_source_leases(&source)?;
                    self.check_close_provider_files(&leases)?;
                    let mut content_leases = Vec::new();
                    content_leases
                        .try_reserve_exact(source.leases.len())
                        .map_err(|_| HostError::Module(crate::loader::ModuleError32::Allocation))?;
                    content_leases.extend(source.leases.iter().map(|source| source.lease));
                    let result = match ready {
                        ReadyModuleSource32::Image {
                            plan,
                            config,
                            completion,
                        } => self.install_module_plan_ready(
                            plan,
                            config,
                            completion,
                            source.works.as_ref().unwrap(),
                        )?,
                        ReadyModuleSource32::DataFile(plan) => self.install_datafile_plan(plan)?,
                    };
                    self.commit_close_provider_files(&content_leases);
                    Ok(result)
                })();
                if let Err(error) = result {
                    source.error = Some(error);
                    return self.finish_module_source_error(source, error);
                }
                result
            }
            Err(error) => {
                source.error = Some(error);
                self.finish_module_source_error(source, error)
            }
        }
    }
    fn finish_module_source_error(
        &mut self,
        source: PendingModuleLoad32,
        error: HostError,
    ) -> Result<ModuleOperation32, HostError> {
        let mut leases = match self.module_source_leases(&source) {
            Ok(leases) => leases,
            Err(cleanup) => {
                self.modules.source = Some(source);
                return Err(cleanup);
            }
        };
        let mut retained = 0;
        for index in 0..leases.len() {
            match self.provider_file_size(leases[index]) {
                Ok(_) => {
                    leases[retained] = leases[index];
                    retained += 1;
                }
                Err(HostError::File(crate::files::FileError32::InvalidHandle)) => {}
                Err(cleanup) => {
                    self.modules.source = Some(source);
                    return Err(cleanup);
                }
            }
        }
        leases.truncate(retained);
        if let Err(cleanup) = self.check_close_provider_files(&leases) {
            self.modules.source = Some(source);
            return Err(cleanup);
        }
        self.commit_close_provider_files(&leases);
        Err(error)
    }
}
