use super::super::strings::{self as memory, Input32};
use crate::{
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32,
        shlwapi::{self, ShlwapiApi32 as Api, ShlwapiError32 as E},
    },
};

impl EngineInstance {
    pub fn register_shlwapi_module(&mut self, base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::windows::shlwapi::shlwapi_module(base))
    }

    pub(in crate::process) fn try_complete_shlwapi_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = Api::from_id(id) else {
            return Ok(false);
        };
        self.validate_windows_provider_owner(owner)?;
        self.checked_pending_call(owner, token)?;
        if frame.convention() != api.convention() || frame.stack_words() != api.stack_words() {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        let (value, error) = match self.execute_shlwapi_call(api, frame.arguments()) {
            Ok(value) => (value, None),
            Err(E::Memory(error)) => return Err(HostError::Memory(error)),
            Err(error) => (0, Some(error.win32())),
        };
        if let Some(error) = error {
            let thread = self.current_windows_thread()?.with_last_error(error);
            self.publish_windows_thread(thread)?;
        }
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
        Ok(true)
    }
    fn execute_shlwapi_call(&mut self, api: Api, args: &[u32]) -> Result<u32, E> {
        use Api::*;
        let wide = api.name().ends_with('W');
        let cp = self.windows_ansi_code_page();
        let bounded_comparison = api == StrCmpNIA && (args[2] as i32) >= 0;
        if bounded_comparison && args[2] == 0 {
            return Ok(0);
        }

        if matches!(
            api,
            PathFileExistsA | PathFileExistsW | PathIsDirectoryA | PathIsDirectoryW
        ) {
            let path = Input32::read(
                self.memory().map_err(|_| E::InvalidParameter)?,
                args[0],
                wide,
                cp,
                260,
            )?;
            return Ok(self.provider_path_info(&path.units).map_or(0, |info| {
                if matches!(api, PathIsDirectoryA | PathIsDirectoryW) {
                    info.attributes & 0x10
                } else {
                    1
                }
            }));
        }
        let string_api = api.name().starts_with("Str");
        let limit = if string_api { 32768 } else { 260 };
        let mut inputs = Vec::new();
        inputs.try_reserve(2).map_err(|_| E::Allocation)?;
        let input_at = if matches!(
            api,
            PathCombineA | PathCombineW | PathCanonicalizeA | PathCanonicalizeW
        ) {
            1
        } else {
            0
        };
        for &pointer in &args[input_at
            ..args.len().min(
                input_at
                    + if matches!(
                        api,
                        PathCombineA
                            | PathCombineW
                            | PathAppendA
                            | PathAppendW
                            | PathRenameExtensionA
                            | PathRenameExtensionW
                            | PathAddExtensionA
                            | PathAddExtensionW
                            | StrCmpIA
                            | StrCmpIW
                            | StrCmpNIA
                            | StrCmpNIW
                            | StrStrIA
                            | StrStrIW
                    ) {
                        2
                    } else {
                        1
                    },
            )]
        {
            inputs.push(if pointer == 0 {
                None
            } else {
                let memory = self.memory().map_err(|_| E::InvalidParameter)?;
                Some(if bounded_comparison {
                    Input32::ansi_prefix(memory, pointer, cp, args[2] as usize, limit)?
                } else {
                    Input32::read(memory, pointer, wide, cp, limit)?
                })
            });
        }
        if matches!(api, PathCombineA | PathCombineW) {
            let value = shlwapi::combine(
                inputs[0].as_ref().map(|i| i.units.as_slice()),
                inputs[1].as_ref().map(|i| i.units.as_slice()),
            )?;
            memory::write_path(self.memory.as_mut().unwrap(), args[0], &value, wide, cp)?;
            return Ok(args[0]);
        }
        let input = inputs[0].as_ref().ok_or(E::InvalidParameter)?;
        let path = &input.units;
        let other = inputs.get(1).and_then(Option::as_ref);
        let mut value = path.clone();
        let result = match api {
            PathFindFileNameA | PathFindFileNameW => {
                return input.position(shlwapi::file_name(path)).map_err(E::from);
            }
            PathFindExtensionA | PathFindExtensionW => {
                return input.position(shlwapi::extension(path)).map_err(E::from);
            }
            PathIsRelativeA | PathIsRelativeW => return Ok(u32::from(shlwapi::is_relative(path))),
            PathIsUNCA | PathIsUNCW => return Ok(u32::from(path.starts_with(&[92, 92]))),
            PathIsRootA | PathIsRootW => {
                return Ok(u32::from(shlwapi::is_root(path)));
            }
            PathCanonicalizeA | PathCanonicalizeW => {
                value = shlwapi::canonicalize(path)?;
                1
            }
            PathAppendA | PathAppendW => {
                value = shlwapi::append(path, &other.ok_or(E::InvalidParameter)?.units)?;
                1
            }
            PathRemoveFileSpecA | PathRemoveFileSpecW => {
                let end = shlwapi::remove_file_spec(path);
                value.truncate(end);
                u32::from(end < path.len())
            }
            PathAddBackslashA | PathAddBackslashW => {
                if !value.is_empty() && value.last() != Some(&92) {
                    value.push(92);
                }
                let encoded = if wide {
                    value.len() * 2
                } else {
                    cp.encode(&value, false)
                        .map_err(|_| E::InvalidParameter)?
                        .0
                        .len()
                };
                args[0]
                    .checked_add(encoded as u32)
                    .ok_or(E::InvalidParameter)?
            }
            PathRemoveBackslashA | PathRemoveBackslashW => {
                let removed = value.len() > shlwapi::root_length(path) && value.last() == Some(&92);
                if removed {
                    value.pop();
                }
                input.position(if removed {
                    value.len()
                } else {
                    value.len().saturating_sub(1)
                })?
            }
            PathRemoveExtensionA | PathRemoveExtensionW => {
                value.truncate(shlwapi::extension(path));
                0
            }
            PathRenameExtensionA | PathRenameExtensionW => {
                value.truncate(shlwapi::extension(path));
                value.extend_from_slice(&other.ok_or(E::InvalidParameter)?.units);
                1
            }
            PathAddExtensionA | PathAddExtensionW => {
                if shlwapi::extension(path) != path.len() {
                    return Ok(0);
                }
                value.extend_from_slice(&other.ok_or(E::InvalidParameter)?.units);
                1
            }
            PathStripPathA | PathStripPathW => {
                value = path[shlwapi::file_name(path)..].to_vec();
                0
            }
            PathStripToRootA | PathStripToRootW => {
                let root = shlwapi::root_length(path);
                value.truncate(root);
                u32::from(root != 0)
            }
            PathQuoteSpacesA | PathQuoteSpacesW => {
                if path.contains(&32) && path.first() != Some(&34) {
                    value.insert(0, 34);
                    value.push(34);
                }
                0
            }
            PathUnquoteSpacesA | PathUnquoteSpacesW => {
                if path.len() >= 2 && path.first() == Some(&34) && path.last() == Some(&34) {
                    value = path[1..path.len() - 1].to_vec();
                }
                0
            }
            StrCmpIA | StrCmpIW | StrCmpNIA | StrCmpNIW => {
                let other = &other.ok_or(E::InvalidParameter)?.units;
                let count = if matches!(api, StrCmpNIA | StrCmpNIW) {
                    args[2] as usize
                } else {
                    usize::MAX
                };
                let ordering = crate::windows::nls::linguistics::compare_for_locale(
                    &path[..path.len().min(count)],
                    &other[..other.len().min(count)],
                    1,
                    self.windows_user_locale(),
                )
                .map_err(|error| match error {
                    crate::windows::nls::NlsError32::Allocation
                    | crate::windows::nls::NlsError32::Capacity => E::Allocation,
                    _ => E::InvalidParameter,
                })?;
                return Ok(match ordering {
                    std::cmp::Ordering::Less => u32::MAX,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                });
            }
            StrStrIA | StrStrIW => {
                let other = &other.ok_or(E::InvalidParameter)?.units;
                let haystack = crate::windows::text::ordinal_key(path);
                let needle = crate::windows::text::ordinal_key(other);
                if needle.is_empty() {
                    return Ok(args[0]);
                }
                return haystack
                    .windows(needle.len())
                    .position(|p| p == needle)
                    .map_or(Ok(0), |offset| input.position(offset).map_err(E::from));
            }
            _ => return Err(E::InvalidParameter),
        };
        memory::write_path(self.memory.as_mut().unwrap(), args[0], &value, wide, cp)?;
        Ok(result)
    }
}

impl From<super::super::strings::WindowsStringError32> for E {
    fn from(error: super::super::strings::WindowsStringError32) -> Self {
        use super::super::strings::WindowsStringError32 as S;
        match error {
            S::InvalidParameter => Self::InvalidParameter,
            S::FilenameTooLong => Self::FilenameTooLong,
            S::Allocation => Self::Allocation,
            S::Memory(error) => Self::Memory(error),
        }
    }
}
