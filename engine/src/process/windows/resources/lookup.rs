use super::{ResourceError32 as E, memory};
use crate::{process::EngineInstance, windows::resources::ResourceApi32 as Api};
impl EngineInstance {
    pub(super) fn resource_lookup(&mut self, api: Api, args: &[u32]) -> Result<u32, E> {
        let module = self.resource_module(args[0])?;
        let preferred = self.current_thread_ui_language();
        match api {
            Api::FindResourceA
            | Api::FindResourceW
            | Api::FindResourceExA
            | Api::FindResourceExW => {
                let extended = matches!(api, Api::FindResourceExA | Api::FindResourceExW);
                let mem = self.memory.as_ref().ok_or(E::NotConfigured)?;
                let kind = memory::identifier(
                    mem,
                    args[if extended { 1 } else { 2 }],
                    api.wide(),
                    self.windows_ansi_code_page(),
                )?;
                let name = memory::identifier(
                    mem,
                    args[if extended { 2 } else { 1 }],
                    api.wide(),
                    self.windows_ansi_code_page(),
                )?;
                let mut reader = self.resource_reader(module)?;
                let location = if extended && args[3] as u16 != 0 {
                    reader.find_exact(&kind, &name, args[3] as u16)?
                } else {
                    reader.find_languages(
                        &kind,
                        &name,
                        &[preferred, preferred & 0x3ff, 0, 0x409, 9],
                        true,
                    )?
                };
                if self
                    .resources
                    .as_ref()
                    .ok_or(E::NotConfigured)?
                    .handles
                    .iter()
                    .all(Option::is_some)
                {
                    let mut live = Vec::new();
                    live.try_reserve_exact(
                        self.loaded_windows_modules().len() + self.resource_only_modules().len(),
                    )
                    .map_err(|_| E::Allocation)?;
                    live.extend(
                        self.loaded_windows_modules()
                            .map(|module| module.id())
                            .chain(self.resource_only_modules().map(|module| module.id())),
                    );
                    self.resources.as_mut().unwrap().prune(&live);
                }
                self.resources
                    .as_mut()
                    .unwrap()
                    .cache(module.id, location.entry_rva)
            }
            Api::LoadResource | Api::SizeofResource => {
                let handle = self
                    .resources
                    .as_ref()
                    .ok_or(E::NotConfigured)?
                    .handle(args[1])?;
                if handle.module != module.id {
                    return Err(E::InvalidHandle);
                }
                let reader = self.resource_reader(module)?;
                let location = reader.location(handle.entry_rva)?;
                if api == Api::SizeofResource {
                    return Ok(location.size);
                }
                let address = reader.address(location.data_rva, location.size)?.0;
                self.resources
                    .as_mut()
                    .unwrap()
                    .handles
                    .iter_mut()
                    .flatten()
                    .find(|entry| entry.value == handle.value)
                    .unwrap()
                    .loaded = true;
                Ok(address)
            }
            _ => Err(E::InvalidParameter),
        }
    }
    pub(super) fn lock_resource(&self, pointer: u32) -> Result<u32, E> {
        let runtime = self.resources.as_ref().ok_or(E::NotConfigured)?;
        for handle in runtime
            .handles
            .iter()
            .flatten()
            .filter(|handle| handle.loaded)
        {
            let Ok(module) = self.resource_module_id(handle.module) else {
                continue;
            };
            let reader = self.resource_reader(module)?;
            let location = reader.location(handle.entry_rva)?;
            if reader.address(location.data_rva, location.size)?.0 == pointer {
                return Ok(pointer);
            }
        }
        Err(E::InvalidHandle)
    }
}
