use std::collections::{BTreeMap, BTreeSet};

use super::super::Access;
use super::{DispatchError, GuestMemory, MemoryError, Process32, Register32, guest};

mod values;

const CURRENT_USER: u32 = 0x8000_0001;
const LOCAL_MACHINE: u32 = 0x8000_0002;
const MAX_KEYS: usize = 4096;
const MAX_HANDLES: usize = 4096;
const LAST_HANDLE: u32 = 0x76ff_fffc;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Open,
    Create,
    Close,
    Query,
    Set,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x278 => Some(Self::Open),
            0x27c => Some(Self::Create),
            0x280 => Some(Self::Close),
            0x284 => Some(Self::Query),
            0x288 => Some(Self::Set),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Open => 5,
            Self::Create => 9,
            Self::Close => 1,
            Self::Query | Self::Set => 6,
        }
    }
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct Key {
    root: u32,
    path: String,
}

pub(super) struct Registry {
    keys: BTreeSet<Key>,
    // the second word retains the granted access mask for value operations.
    handles: BTreeMap<u32, (Key, u32)>,
    next: u32,
    values: values::Values,
}

impl Default for Registry {
    fn default() -> Self {
        let keys = [CURRENT_USER, LOCAL_MACHINE]
            .into_iter()
            .flat_map(|root| {
                ["", "software"].map(|path| Key {
                    root,
                    path: path.to_owned(),
                })
            })
            .collect();
        Self {
            keys,
            handles: BTreeMap::new(),
            next: 0x7600_0004,
            values: values::Values::default(),
        }
    }
}

impl Registry {
    fn parent(&self, handle: u32) -> Result<Option<Key>, DispatchError> {
        match handle {
            CURRENT_USER | LOCAL_MACHINE => Ok(Some(Key {
                root: handle,
                path: String::new(),
            })),
            0x8000_0000 | 0x8000_0003..=0x8000_0007 | 0x8000_0050 | 0x8000_0060 => {
                Err(DispatchError::Unsupported)
            }
            _ => Ok(self.handles.get(&handle).map(|(key, _)| key.clone())),
        }
    }

    fn dispatch(
        &mut self,
        call: Call,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let Some(parent) = self.parent(args[0])? else {
            return Ok(6);
        };
        if matches!(call, Call::Query | Call::Set) {
            let access = self
                .handles
                .get(&args[0])
                .map_or(0xf003f, |(_, access)| *access);
            let required = if matches!(call, Call::Query) { 1 } else { 2 };
            if access & required == 0 {
                return Ok(5);
            }
            return self.values.dispatch(call, parent, args, memory);
        }
        if matches!(call, Call::Close) {
            self.handles.remove(&args[0]);
            return Ok(0);
        }
        let create = matches!(call, Call::Create);
        let (access, output, disposition) = if create {
            if args[1] == 0 || args[2] != 0 || args[3] != 0 || args[4] != 0 || args[6] != 0 {
                return Err(DispatchError::Unsupported);
            }
            (args[5], args[7], args[8])
        } else {
            if args[2] != 0 {
                return Err(DispatchError::Unsupported);
            }
            (args[3], args[4], 0)
        };
        if access & !0x000f_003f != 0 {
            return Err(DispatchError::Unsupported);
        }
        let path = read_name(memory, args[1])?;
        if !path.is_empty() && path.split('\\').any(str::is_empty) {
            return Err(DispatchError::Unsupported);
        }
        if !create && path.is_empty() && matches!(args[0], CURRENT_USER | LOCAL_MACHINE) {
            guest::write_word(memory, output, args[0])?;
            return Ok(0);
        }
        let target = join(parent, &path)?;
        let exists = self.keys.contains(&target);
        if !create && !exists {
            return Ok(2);
        }
        let missing = self.missing(&target);
        if create
            && target.root == LOCAL_MACHINE
            && missing.iter().any(|key| !key.path.contains('\\'))
        {
            return Ok(5);
        }
        if self.handles.len() == MAX_HANDLES
            || self.next > LAST_HANDLE
            || self.keys.len() + missing.len() > MAX_KEYS
        {
            return Ok(8);
        }
        guest::check(memory, output, 4, Access::Write)?;
        if disposition != 0 {
            guest::check(memory, disposition, 4, Access::Write)?;
        }
        guest::write_word(memory, output, self.next)?;
        if disposition != 0 {
            guest::write_word(memory, disposition, if exists { 2 } else { 1 })?;
        }
        self.keys.extend(missing);
        self.handles.insert(self.next, (target, access));
        self.next += 4;
        Ok(0)
    }

    fn missing(&self, target: &Key) -> Vec<Key> {
        let mut missing = Vec::new();
        for end in target
            .path
            .match_indices('\\')
            .map(|(index, _)| index)
            .chain(std::iter::once(target.path.len()))
        {
            let key = Key {
                root: target.root,
                path: target.path[..end].to_owned(),
            };
            if !self.keys.contains(&key) {
                missing.push(key);
            }
        }
        missing
    }
}

fn read_name(memory: &GuestMemory, pointer: u32) -> Result<String, DispatchError> {
    let mut name = String::new();
    if pointer == 0 {
        return Ok(name);
    }
    for offset in 0..256 {
        let address = pointer
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 {
            return Ok(name);
        }
        if !(0x20..=0x7e).contains(&byte[0]) {
            return Err(DispatchError::Unsupported);
        }
        name.push(char::from(byte[0].to_ascii_lowercase()));
    }
    Err(DispatchError::Unsupported)
}

fn join(mut parent: Key, path: &str) -> Result<Key, DispatchError> {
    if !parent.path.is_empty() && !path.is_empty() {
        parent.path.push('\\');
    }
    parent.path.push_str(path);
    if parent.path.len() > 255 || parent.path.split('\\').count() > 32 {
        return Err(DispatchError::Unsupported);
    }
    Ok(parent)
}

impl Process32 {
    pub(super) fn registry(&mut self, call: Call, arguments: &[u32]) -> Result<(), DispatchError> {
        let value = self.registry.dispatch(call, arguments, &mut self.memory)?;
        self.cpu.set_register(Register32::Eax, value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::Permissions;

    fn memory() -> GuestMemory {
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        memory.write(0x1000, b"software\\sample\0").unwrap();
        memory
    }

    #[test]
    fn resource_limits_leave_keys_outputs_and_identity_unchanged() {
        for full_handles in [false, true] {
            let mut registry = Registry::default();
            if full_handles {
                for index in 0..MAX_HANDLES {
                    registry.handles.insert(
                        u32::try_from(index).unwrap(),
                        (
                            Key {
                                root: CURRENT_USER,
                                path: "software".to_owned(),
                            },
                            1,
                        ),
                    );
                }
            } else {
                for index in 4..MAX_KEYS {
                    registry.keys.insert(Key {
                        root: CURRENT_USER,
                        path: format!("key{index}"),
                    });
                }
            }
            let mut memory = memory();
            let key_count = registry.keys.len();
            assert!(matches!(
                registry.dispatch(
                    Call::Create,
                    &[CURRENT_USER, 0x1000, 0, 0, 0, 1, 0, 0xffff_ffff, 0],
                    &mut memory
                ),
                Ok(8)
            ));
            assert_eq!(registry.keys.len(), key_count);
            assert_eq!(registry.next, 0x7600_0004);
        }
    }

    #[test]
    fn exhausted_identities_are_not_recycled_and_access_is_retained() {
        let mut registry = Registry {
            next: LAST_HANDLE,
            ..Registry::default()
        };
        let mut memory = memory();
        assert!(matches!(
            registry.dispatch(
                Call::Create,
                &[CURRENT_USER, 0x1000, 0, 0, 0, 0x20019, 0, 0x1100, 0],
                &mut memory
            ),
            Ok(0)
        ));
        assert_eq!(registry.handles[&LAST_HANDLE].1, 0x20019);
        assert!(matches!(
            registry.dispatch(Call::Close, &[LAST_HANDLE], &mut memory),
            Ok(0)
        ));
        assert!(matches!(
            registry.dispatch(
                Call::Open,
                &[CURRENT_USER, 0x1000, 0, 1, 0x1100],
                &mut memory
            ),
            Ok(8)
        ));
        assert_eq!(registry.next, LAST_HANDLE + 4);
        assert!(registry.handles.is_empty());
    }
}
