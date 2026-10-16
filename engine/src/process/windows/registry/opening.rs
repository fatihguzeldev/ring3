use super::{RegistryError32 as E, memory};
use crate::{
    memory::GuestAddress, process::EngineInstance, windows::registry::Advapi32Api32 as Api,
};
impl EngineInstance {
    pub(super) fn registry_open(&mut self, api: Api, args: &[u32], time: u64) -> Result<u32, E> {
        let mem = self.memory.as_mut().ok_or(E::NotConfigured)?;
        let runtime = self.registry.as_mut().ok_or(E::NotConfigured)?;
        if api == Api::RegOpenCurrentUser {
            memory::output(mem, args[1], 4)?;
            let prepared = runtime.native.prepare_root_handle(0x80000001, args[0])?;
            let handle = prepared.result().to_le_bytes();
            memory::publish(mem, &[(GuestAddress(args[1]), &handle)])?;
            runtime.native.commit(prepared);
            return Ok(0);
        }
        let (parent, path, desired, out, class, volatile, disposition, create) = match api {
            Api::RegConnectRegistryA | Api::RegConnectRegistryW => {
                let name = memory::string(mem, args[0], api.wide(), 32767)?;
                let name = String::from_utf16_lossy(&name)
                    .trim_start_matches('\\')
                    .to_ascii_uppercase();
                if !name.is_empty() && !matches!(name.as_str(), "." | "LOCALHOST" | "RING3") {
                    return Err(E::BadNetworkPath);
                }
                (
                    args[1],
                    Vec::new(),
                    0xf003f,
                    args[2],
                    Vec::new(),
                    false,
                    0,
                    false,
                )
            }
            Api::RegOpenKeyExA | Api::RegOpenKeyExW => {
                if args[2] != 0 {
                    return Err(E::InvalidParameter);
                }
                (
                    args[0],
                    memory::string(mem, args[1], api.wide(), 32767)?,
                    args[3],
                    args[4],
                    Vec::new(),
                    false,
                    0,
                    false,
                )
            }
            Api::RegOpenKeyA | Api::RegOpenKeyW => (
                args[0],
                memory::string(mem, args[1], api.wide(), 32767)?,
                0xf003f,
                args[2],
                Vec::new(),
                false,
                0,
                false,
            ),
            Api::RegCreateKeyA | Api::RegCreateKeyW => (
                args[0],
                memory::string(mem, args[1], api.wide(), 32767)?,
                0xf003f,
                args[2],
                Vec::new(),
                false,
                0,
                true,
            ),
            Api::RegCreateKeyExA | Api::RegCreateKeyExW => {
                if args[2] != 0 || args[4] & !1 != 0 {
                    return Err(E::InvalidParameter);
                }
                if args[6] != 0 {
                    let security = memory::read::<12>(mem, args[6])?;
                    if u32::from_le_bytes(security[..4].try_into().unwrap()) != 12 {
                        return Err(E::InvalidParameter);
                    }
                    if security[4..8] != [0; 4] {
                        return Err(E::CallNotImplemented);
                    }
                }
                (
                    args[0],
                    memory::string(mem, args[1], api.wide(), 32767)?,
                    args[5],
                    args[7],
                    memory::string(mem, args[3], api.wide(), 255)?,
                    args[4] & 1 != 0,
                    args[8],
                    true,
                )
            }
            _ => return Err(E::InvalidParameter),
        };
        memory::output(mem, out, 4)?;
        if disposition != 0 {
            memory::output(mem, disposition, 4)?;
        }
        let prepared = if create {
            runtime
                .native
                .prepare_create(parent, &path, desired, &class, volatile, time, true)?
        } else {
            runtime.native.prepare_open(parent, &path, desired)?
        };
        runtime.native.can_commit(&prepared)?;
        let plan = runtime.prepare_notifications(
            &prepared,
            self.synchronization.as_ref().map(|sync| sync.context()),
        )?;
        let handle = prepared.result().to_le_bytes();
        let disposition_value = prepared.disposition().to_le_bytes();
        let writes = [
            (GuestAddress(out), &handle[..]),
            (GuestAddress(disposition), &disposition_value[..]),
        ];
        memory::publish(mem, &writes[..if disposition == 0 { 1 } else { 2 }])?;
        runtime.native.commit(prepared);
        runtime.commit_notifications(
            plan,
            self.synchronization.as_mut().map(|sync| sync.context_mut()),
        );
        Ok(0)
    }
}
