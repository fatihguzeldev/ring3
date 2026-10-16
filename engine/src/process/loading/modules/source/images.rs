use super::leases::ModuleSourceLease32;
use crate::{
    files::FileSection32,
    loader::{LoadPlan32, ModuleError32, ModuleId32},
    process::{
        EngineInstance, HostError,
        windows::files::{ImageFile32, PreparedImageFiles32, ProviderFile32},
    },
};

#[derive(Clone, Copy)]
enum ImageOwner32 {
    Module(ModuleId32),
    Resource(u32),
}
pub(in crate::process::loading::modules) struct ModuleImageSource32 {
    owner: ImageOwner32,
    file: ImageFile32,
}
pub(in crate::process::loading::modules) struct PreparedModuleImageFiles32 {
    native: PreparedImageFiles32,
    owned: Vec<ModuleImageSource32>,
}
impl EngineInstance {
    pub(super) fn prepare_module_image_files(
        &mut self,
        plan: &LoadPlan32,
        sources: &[ModuleSourceLease32],
    ) -> Result<Option<PreparedModuleImageFiles32>, HostError> {
        let mut owners = Vec::new();
        for module in plan.additions() {
            if let Some(source) = sources
                .iter()
                .find(|source| source.identity == module.identity())
            {
                owners
                    .try_reserve(1)
                    .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
                owners.push((
                    ImageOwner32::Module(module.id()),
                    source.lease,
                    FileSection32::Image,
                ));
            }
        }
        self.prepare_owned_image_files(&owners)
    }
    pub(super) fn prepare_resource_image_files(
        &mut self,
        handle: u32,
        identity: &str,
        flags: u32,
        sources: &[ModuleSourceLease32],
    ) -> Result<Option<PreparedModuleImageFiles32>, HostError> {
        let Some(source) = sources.iter().find(|source| source.identity == identity) else {
            return Ok(None);
        };
        self.prepare_owned_image_files(&[(
            ImageOwner32::Resource(handle),
            source.lease,
            if flags & 0x20 != 0 {
                FileSection32::Image
            } else if flags & 0x40 != 0 {
                FileSection32::ExclusiveData
            } else {
                FileSection32::SharedData
            },
        )])
    }
    fn prepare_owned_image_files(
        &mut self,
        owners: &[(ImageOwner32, ProviderFile32, FileSection32)],
    ) -> Result<Option<PreparedModuleImageFiles32>, HostError> {
        if owners.is_empty() {
            return Ok(None);
        }
        self.modules
            .image_sources
            .try_reserve(owners.len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(owners.len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        sources.extend(owners.iter().map(|(_, source, image)| (*source, *image)));
        let native = self.prepare_image_files(&sources)?;
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(owners.len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        owned.extend(
            owners
                .iter()
                .zip(native.files())
                .map(|((owner, _, _), file)| ModuleImageSource32 {
                    owner: *owner,
                    file: *file,
                }),
        );
        Ok(Some(PreparedModuleImageFiles32 { native, owned }))
    }
    pub(in crate::process::loading::modules) fn commit_module_image_files(
        &mut self,
        prepared: Option<PreparedModuleImageFiles32>,
    ) {
        let Some(prepared) = prepared else {
            return;
        };
        self.commit_image_files(prepared.native);
        self.modules.image_sources.extend(prepared.owned);
    }
    pub(super) fn module_source_is_image(&self, source: ProviderFile32) -> bool {
        self.modules
            .image_sources
            .iter()
            .any(|image| image.file.is_source(source))
    }
    pub(in crate::process::loading::modules) fn prepare_module_image_release(
        &self,
        ids: &[ModuleId32],
    ) -> Result<Vec<ImageFile32>, HostError> {
        self.prepare_image_release(
            |owner| matches!(owner, ImageOwner32::Module(id) if ids.contains(&id)),
        )
    }
    pub(in crate::process::loading::modules) fn prepare_resource_image_release(
        &self,
        handle: u32,
    ) -> Result<Vec<ImageFile32>, HostError> {
        self.prepare_image_release(
            |owner| matches!(owner, ImageOwner32::Resource(id) if id == handle),
        )
    }
    fn prepare_image_release(
        &self,
        matches: impl Fn(ImageOwner32) -> bool,
    ) -> Result<Vec<ImageFile32>, HostError> {
        let mut files = Vec::new();
        for image in self
            .modules
            .image_sources
            .iter()
            .filter(|image| matches(image.owner))
        {
            files
                .try_reserve(1)
                .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
            files.push(image.file);
        }
        self.check_close_image_files(&files)?;
        Ok(files)
    }
    pub(in crate::process::loading::modules) fn commit_module_image_release(
        &mut self,
        images: &[ImageFile32],
    ) {
        self.commit_close_image_files(images);
        self.modules
            .image_sources
            .retain(|image| !images.contains(&image.file));
    }
    pub(in crate::process) fn retire_module_image_files_for_process_exit(&mut self) {
        self.modules.image_sources.clear();
    }
}
