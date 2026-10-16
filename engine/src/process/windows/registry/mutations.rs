use super::{RegistryError32 as E, memory};
use crate::{process::EngineInstance, windows::registry::Advapi32Api32 as Api};
impl EngineInstance {
    pub(super) fn registry_mutation(
        &mut self,
        api: Api,
        args: &[u32],
        time: u64,
    ) -> Result<u32, E> {
        let cp = self.windows_ansi_code_page();
        let mem = self.memory.as_ref().ok_or(E::NotConfigured)?;
        let runtime = self.registry.as_mut().ok_or(E::NotConfigured)?;
        let registry = &mut runtime.native;
        let prepared = match api {
            Api::RegSetValueExA | Api::RegSetValueExW => {
                if args[2] != 0 {
                    return Err(E::InvalidParameter);
                }
                let name = memory::string(mem, args[1], api.wide(), 16383, cp)?;
                let bytes = memory::input(
                    mem,
                    args[4],
                    args[5] as usize,
                    registry.maximum_value_bytes(),
                )?;
                let bytes = memory::value_from_guest(args[3], bytes, api.wide(), cp)?;
                registry.prepare_set_value(args[0], &name, args[3], &bytes, time, true)?
            }
            Api::RegSetValueA | Api::RegSetValueW => {
                if args[2] != 1 || args[3] == 0 {
                    return Err(E::InvalidParameter);
                }
                let path = memory::string(mem, args[1], api.wide(), 32767, cp)?;
                let units = memory::string(
                    mem,
                    args[3],
                    api.wide(),
                    registry.maximum_value_bytes() / 2,
                    cp,
                )?;
                let bytes = memory::encoded_name(&units, true, true, cp)?;
                registry.prepare_set_path(args[0], &path, &[], 1, &bytes, time, true)?
            }
            Api::RegSetKeyValueA | Api::RegSetKeyValueW => {
                let path = memory::string(mem, args[1], api.wide(), 32767, cp)?;
                let name = memory::string(mem, args[2], api.wide(), 16383, cp)?;
                let bytes = memory::input(
                    mem,
                    args[4],
                    args[5] as usize,
                    registry.maximum_value_bytes(),
                )?;
                let bytes = memory::value_from_guest(args[3], bytes, api.wide(), cp)?;
                registry.prepare_set_path(args[0], &path, &name, args[3], &bytes, time, true)?
            }
            Api::RegDeleteKeyValueA | Api::RegDeleteKeyValueW => {
                let path = memory::string(mem, args[1], api.wide(), 32767, cp)?;
                let name = memory::string(mem, args[2], api.wide(), 16383, cp)?;
                registry.prepare_delete_path_value(args[0], &path, &name, time)?
            }
            Api::RegCopyTreeA | Api::RegCopyTreeW => {
                let path = memory::string(mem, args[1], api.wide(), 32767, cp)?;
                registry.prepare_copy_tree(args[0], &path, args[2], time)?
            }
            Api::RegDeleteValueA | Api::RegDeleteValueW => {
                let name = memory::string(mem, args[1], api.wide(), 16383, cp)?;
                registry.prepare_delete_value(args[0], &name, time)?
            }
            Api::RegDeleteKeyA
            | Api::RegDeleteKeyW
            | Api::RegDeleteKeyExA
            | Api::RegDeleteKeyExW => {
                if matches!(api, Api::RegDeleteKeyExA | Api::RegDeleteKeyExW) && args[3] != 0 {
                    return Err(E::InvalidParameter);
                }
                let name = memory::string(mem, args[1], api.wide(), 32767, cp)?;
                registry.prepare_delete_key(
                    args[0],
                    &name,
                    if matches!(api, Api::RegDeleteKeyExA | Api::RegDeleteKeyExW) {
                        args[2]
                    } else {
                        0
                    },
                    false,
                    false,
                    time,
                )?
            }
            Api::RegDeleteTreeA | Api::RegDeleteTreeW => {
                let name = memory::string(mem, args[1], api.wide(), 32767, cp)?;
                registry.prepare_delete_key(args[0], &name, 0, true, name.is_empty(), time)?
            }
            _ => return Err(E::CallNotImplemented),
        };
        registry.can_commit(&prepared)?;
        let plan = runtime.prepare_notifications(
            &prepared,
            self.synchronization.as_ref().map(|sync| sync.context()),
        )?;
        runtime.native.commit(prepared);
        runtime.commit_notifications(
            plan,
            self.synchronization.as_mut().map(|sync| sync.context_mut()),
        );
        Ok(0)
    }
}
