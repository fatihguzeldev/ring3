use crate::{
    memory::{GuestAddress, WordWrite32},
    process::{EngineInstance, HostError},
    windows::automation::{AutomationError32 as E, Oleaut32Api32 as Api},
};
impl EngineInstance {
    pub(in crate::process::windows::automation) fn automation_array_call(
        &mut self,
        api: Api,
        args: &[u32],
    ) -> Result<u32, HostError> {
        use Api::*;
        if matches!(api, SafeArrayCreate | SafeArrayCreateVector) {
            return self.create_automation_array(api, args);
        }
        if args[0] == 0
            && matches!(
                api,
                SafeArrayGetDim | SafeArrayGetElemsize | SafeArrayDestroy
            )
        {
            return Ok(0);
        }
        let (index, locks) = self.automation_array_index(args[0])?;
        let array = &self.automation.arrays[index];
        match api {
            SafeArrayGetDim => Ok(array.layout.bounds.len() as u32),
            SafeArrayGetElemsize => Ok(array.layout.element_bytes as u32),
            SafeArrayGetVartype | SafeArrayGetLBound | SafeArrayGetUBound => {
                let mut bytes = [0; 4];
                let (at, size) = if api == SafeArrayGetVartype {
                    bytes[..2].copy_from_slice(&array.layout.vartype.to_le_bytes());
                    (args[1], 2)
                } else {
                    let bound = array.layout.bound(args[1]).map_err(HostError::Automation)?;
                    let value = if api == SafeArrayGetLBound {
                        bound.lower
                    } else {
                        bound.lower.wrapping_add(bound.count as i32).wrapping_sub(1)
                    };
                    bytes.copy_from_slice(&value.to_le_bytes());
                    (args[2], 4)
                };
                if at == 0 {
                    return Err(HostError::Automation(E::InvalidParameter));
                }
                self.memory
                    .as_mut()
                    .unwrap()
                    .write(GuestAddress(at), &bytes[..size])
                    .map_err(HostError::Memory)?;
                Ok(0)
            }
            SafeArrayLock | SafeArrayUnlock | SafeArrayUnaccessData | SafeArrayAccessData => {
                if api == SafeArrayAccessData && args[1] == 0 {
                    return Err(HostError::Automation(E::InvalidParameter));
                }
                let acquire = matches!(api, SafeArrayLock | SafeArrayAccessData);
                let next = if acquire {
                    locks.checked_add(1)
                } else {
                    locks.checked_sub(1)
                }
                .ok_or(HostError::Automation(E::Unexpected))?;
                let writes = [
                    WordWrite32 {
                        address: GuestAddress(args[0] + 8),
                        value: next,
                    },
                    WordWrite32 {
                        address: GuestAddress(args.get(1).copied().unwrap_or(0)),
                        value: array.data,
                    },
                ];
                self.memory
                    .as_mut()
                    .unwrap()
                    .write_words32(&writes[..if api == SafeArrayAccessData { 2 } else { 1 }])
                    .map_err(HostError::Memory)?;
                Ok(0)
            }
            SafeArrayGetElement | SafeArrayPutElement => {
                self.automation_array_element(index, api, args)
            }
            SafeArrayCopy if array.layout.vartype == 8 => self.copy_bstr_array(index, args[1]),
            SafeArrayCopy => self.copy_automation_array(index, args[1]),
            SafeArrayRedim if array.layout.vartype == 8 => {
                if locks != 0 {
                    return Err(HostError::Automation(E::ArrayLocked));
                }
                self.redim_bstr_array(index, args[1])
            }
            SafeArrayRedim => self.redim_automation_array(index, locks, args[1]),
            SafeArrayDestroy => {
                if locks != 0 {
                    return Err(HostError::Automation(E::ArrayLocked));
                }
                if array.layout.vartype == 8 {
                    return self.destroy_bstr_array(index);
                }
                if array.layout.owns_elements() {
                    return Err(HostError::Automation(E::NotImplemented));
                }
                self.destroy_automation_array(index)?;
                Ok(0)
            }
            _ => Err(HostError::Automation(E::InvalidParameter)),
        }
    }
    pub(super) fn destroy_automation_array(&mut self, index: usize) -> Result<(), HostError> {
        let array = &self.automation.arrays[index];
        self.check_windows_runtime_bytes(array.base, array.allocation_bytes as u32)?;
        if array.separate_data {
            self.check_windows_runtime_bytes(array.data, array.layout.data_bytes as u32)?;
        }
        let array = self.automation.arrays.remove(index);
        self.free_windows_runtime_bytes(array.base);
        if array.separate_data {
            self.free_windows_runtime_bytes(array.data);
        }
        self.automation.bytes -= array.allocation_bytes
            + if array.separate_data {
                array.layout.data_bytes
            } else {
                0
            };
        Ok(())
    }
}
