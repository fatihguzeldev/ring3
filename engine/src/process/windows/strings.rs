use crate::{
    loader::ModuleError32,
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub(super) fn windows_string(
        &self,
        pointer: u32,
        wide: bool,
        limit: usize,
    ) -> Result<String, HostError> {
        self.read_windows_string(pointer, wide, limit, Some(self.windows_ansi_code_page()))
    }
    pub(super) fn windows_export_name(
        &self,
        pointer: u32,
        limit: usize,
    ) -> Result<String, HostError> {
        self.read_windows_string(pointer, false, limit, None)
    }
    fn read_windows_string(
        &self,
        pointer: u32,
        wide: bool,
        limit: usize,
        code_page: Option<crate::windows::text::CodePage32>,
    ) -> Result<String, HostError> {
        if pointer == 0 {
            return Err(HostError::Module(ModuleError32::InvalidName));
        }
        let mut units = Vec::new();
        units
            .try_reserve(limit)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        for index in 0..=limit {
            let address = pointer
                .checked_add((index * if wide { 2 } else { 1 }) as u32)
                .ok_or(HostError::Module(ModuleError32::InvalidName))?;
            let mut bytes = [0; 2];
            self.memory()?
                .read(
                    GuestAddress(address),
                    &mut bytes[..if wide { 2 } else { 1 }],
                )
                .map_err(HostError::Memory)?;
            let unit = u16::from_le_bytes(bytes);
            if unit == 0 {
                if !wide {
                    let bytes = units.iter().map(|unit| *unit as u8).collect::<Vec<_>>();
                    if let Some(code_page) = code_page {
                        units = code_page
                            .decode(&bytes, false)
                            .map_err(|_| HostError::Module(ModuleError32::InvalidName))?;
                    } else {
                        return String::from_utf8(bytes)
                            .map_err(|_| HostError::Module(ModuleError32::InvalidName));
                    }
                }
                return String::from_utf16(&units)
                    .map_err(|_| HostError::Module(ModuleError32::InvalidName));
            }
            if index == limit {
                break;
            }
            units.push(unit);
        }
        Err(HostError::Module(ModuleError32::InvalidName))
    }
}

mod borrowed {
    use crate::{
        memory::{AddressSpace, GuestAddress},
        windows::text::CodePage32,
    };

    #[derive(Debug)]
    pub(in crate::process::windows) enum WindowsStringError32 {
        InvalidParameter,
        FilenameTooLong,
        Allocation,
        Memory(crate::memory::MemoryError),
    }
    use WindowsStringError32 as E;

    pub(in crate::process::windows) struct Input32 {
        pub units: Vec<u16>,
        pub pointer: u32,
        wide: bool,
        cp: CodePage32,
    }
    impl Input32 {
        pub fn read(
            memory: &AddressSpace,
            pointer: u32,
            wide: bool,
            cp: CodePage32,
            limit: usize,
        ) -> Result<Self, E> {
            if pointer == 0 {
                return Err(E::InvalidParameter);
            }
            let width = if wide { 2 } else { 1 };
            let mut bytes = Vec::new();
            bytes
                .try_reserve(limit.min(256) * width)
                .map_err(|_| E::Allocation)?;
            for index in 0..limit {
                let at = pointer
                    .checked_add((index * width) as u32)
                    .ok_or(E::InvalidParameter)?;
                let mut unit = [0; 2];
                memory
                    .read(GuestAddress(at), &mut unit[..width])
                    .map_err(E::Memory)?;
                if unit[..width].iter().all(|&b| b == 0) {
                    let units = if wide {
                        bytes
                            .chunks_exact(2)
                            .map(|v| u16::from_le_bytes([v[0], v[1]]))
                            .collect()
                    } else {
                        cp.decode(&bytes, true).map_err(|_| E::InvalidParameter)?
                    };
                    return Ok(Self {
                        units,
                        pointer,
                        wide,
                        cp,
                    });
                }
                bytes.try_reserve(width).map_err(|_| E::Allocation)?;
                bytes.extend_from_slice(&unit[..width]);
            }
            Err(E::FilenameTooLong)
        }
        pub fn ansi_prefix(
            memory: &AddressSpace,
            pointer: u32,
            cp: CodePage32,
            count: usize,
            limit: usize,
        ) -> Result<Self, E> {
            if count == 0 {
                return Ok(Self {
                    units: Vec::new(),
                    pointer,
                    wide: false,
                    cp,
                });
            }
            if pointer == 0 {
                return Err(E::InvalidParameter);
            }
            let width = 1;
            let mut bytes = Vec::new();
            bytes
                .try_reserve(count.min(limit).min(256) * width)
                .map_err(|_| E::Allocation)?;
            let mut ended = false;
            for index in 0..count.min(limit) {
                let at = pointer
                    .checked_add((index * width) as u32)
                    .ok_or(E::InvalidParameter)?;
                let mut unit = [0; 2];
                memory
                    .read(GuestAddress(at), &mut unit[..width])
                    .map_err(E::Memory)?;
                if unit[..width].iter().all(|&b| b == 0) {
                    ended = true;
                    break;
                }
                bytes.try_reserve(width).map_err(|_| E::Allocation)?;
                bytes.extend_from_slice(&unit[..width]);
            }
            if count > limit && !ended {
                return Err(E::FilenameTooLong);
            }
            let units = cp.decode(&bytes, false).map_err(|_| E::InvalidParameter)?;
            Ok(Self {
                units,
                pointer,
                wide: false,
                cp,
            })
        }
        pub fn position(&self, index: usize) -> Result<u32, E> {
            let count = if self.wide {
                index.checked_mul(2).ok_or(E::InvalidParameter)?
            } else {
                self.cp
                    .encode(&self.units[..index], false)
                    .map_err(|_| E::InvalidParameter)?
                    .0
                    .len()
            };
            self.pointer
                .checked_add(count as u32)
                .ok_or(E::InvalidParameter)
        }
    }
    pub(in crate::process::windows) fn write_path(
        memory: &mut AddressSpace,
        pointer: u32,
        units: &[u16],
        wide: bool,
        cp: CodePage32,
    ) -> Result<(), E> {
        if pointer == 0 {
            return Err(E::InvalidParameter);
        }
        let mut bytes = if wide {
            units
                .iter()
                .flat_map(|u| u.to_le_bytes())
                .collect::<Vec<_>>()
        } else {
            cp.encode_windows(units, 0, b'?')
                .map_err(|_| E::InvalidParameter)?
                .0
        };
        let width = if wide { 2 } else { 1 };
        if bytes.len() / width >= 260 {
            return Err(E::FilenameTooLong);
        }
        bytes.resize(bytes.len() + width, 0);
        memory
            .write(GuestAddress(pointer), &bytes)
            .map_err(E::Memory)
    }
}
pub(in crate::process::windows) use borrowed::{Input32, WindowsStringError32, write_path};
