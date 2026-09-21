use super::super::GuestModule;
use super::{GuestMemory, LoadError, MemoryError, PAGE_SIZE, Permissions};

const BASE: u32 = 0x7000_4000;
const MAX_BYTES: usize = 64 * 1024;

/// explicit narrow-byte inputs; the host environment is never inherited.
#[derive(Clone, Copy, Debug)]
pub struct ProcessOptions<'a> {
    pub image_path: &'a [u8],
    pub current_directory: &'a [u8],
    /// existing virtual directories; declarations also make their ancestors available.
    pub directories: &'a [&'a [u8]],
    pub command_line: &'a [u8],
    pub environment: &'a [&'a [u8]],
    pub diagnostic_imports: bool,
    pub modules: &'a [GuestModule<'a>],
}

impl Default for ProcessOptions<'_> {
    fn default() -> Self {
        Self {
            image_path: b"C:\\program.exe",
            current_directory: b"C:\\",
            directories: &[],
            command_line: b"program.exe",
            environment: &[],
            diagnostic_imports: false,
            modules: &[],
        }
    }
}

pub(super) struct Parameters {
    bytes: Vec<u8>,
    pub(super) command_line: u32,
    pub(super) argc: u32,
    pub(super) argv: u32,
    pub(super) environment: u32,
}

impl Parameters {
    pub(super) fn prepare(options: ProcessOptions<'_>) -> Result<Self, LoadError> {
        validate_image_path(options)?;
        if options.command_line.len() > 32767
            || options.command_line.contains(&0)
            || options.environment.len() > 256
            || options.environment.iter().any(|entry| {
                entry.len() >= MAX_BYTES || entry.contains(&0) || !entry.contains(&b'=')
            })
        {
            return Err(LoadError::InvalidProcessParameters);
        }
        let arguments = parse(options.command_line);
        let argc = u32::try_from(arguments.len()).expect("bounded command line");
        let table_bytes = (arguments.len() + options.environment.len() + 2) * 4;
        if table_bytes > MAX_BYTES {
            return Err(LoadError::InvalidProcessParameters);
        }
        let mut parameters = Self {
            bytes: vec![0; table_bytes],
            command_line: 0,
            argc,
            argv: BASE,
            environment: BASE + (argc + 1) * 4,
        };
        parameters.command_line = parameters.append(options.command_line)?;
        for (index, argument) in arguments.iter().enumerate() {
            let address = parameters.append(argument)?;
            parameters.bytes[index * 4..index * 4 + 4].copy_from_slice(&address.to_le_bytes());
        }
        for (index, entry) in options.environment.iter().enumerate() {
            let address = parameters.append(entry)?;
            let offset = (arguments.len() + 1 + index) * 4;
            parameters.bytes[offset..offset + 4].copy_from_slice(&address.to_le_bytes());
        }
        Ok(parameters)
    }

    fn append(&mut self, value: &[u8]) -> Result<u32, LoadError> {
        if self.bytes.len() + value.len() + 1 > MAX_BYTES {
            return Err(LoadError::InvalidProcessParameters);
        }
        let address = BASE + u32::try_from(self.bytes.len()).expect("bounded parameter image");
        self.bytes.extend_from_slice(value);
        self.bytes.push(0);
        Ok(address)
    }

    pub(super) fn map(&self, memory: &mut GuestMemory) -> Result<(), MemoryError> {
        let length = (self.bytes.len() as u64).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        memory.map_zeroed(u64::from(BASE), length, Permissions::READ_WRITE)?;
        memory.write(u64::from(BASE), &self.bytes)
    }
}

fn validate_image_path(options: ProcessOptions<'_>) -> Result<(), LoadError> {
    let path = options.image_path;
    if !valid_absolute_path(path) {
        return Err(LoadError::InvalidProcessParameters);
    }
    let parent = path
        .iter()
        .rposition(|&byte| byte == b'\\')
        .expect("absolute path")
        + 1;
    if options
        .modules
        .iter()
        .any(|module| parent + module.name.len() > 32767)
    {
        return Err(LoadError::InvalidProcessParameters);
    }
    Ok(())
}

pub(super) fn valid_absolute_path(path: &[u8]) -> bool {
    (4..=32767).contains(&path.len())
        && path[0].is_ascii_alphabetic()
        && &path[1..3] == b":\\"
        && !path[3..].split(|&byte| byte == b'\\').any(|component| {
            component.is_empty()
                || matches!(component, b"." | b"..")
                || component
                    .iter()
                    .any(|&byte| !(0x20..=0x7e).contains(&byte) || b"<>:\"|?*/".contains(&byte))
        })
}

fn whitespace(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

fn parse(line: &[u8]) -> Vec<Vec<u8>> {
    let mut cursor = 0;
    let mut quoted = false;
    let mut program = Vec::new();
    while let Some(&byte) = line.get(cursor) {
        if !quoted && whitespace(byte) {
            break;
        }
        if byte == b'"' {
            quoted = !quoted;
        } else {
            program.push(byte);
        }
        cursor += 1;
    }
    let mut arguments = vec![program];
    loop {
        while line.get(cursor).is_some_and(|&byte| whitespace(byte)) {
            cursor += 1;
        }
        if cursor == line.len() {
            return arguments;
        }
        let mut argument = Vec::new();
        quoted = false;
        while let Some(&byte) = line.get(cursor) {
            if !quoted && whitespace(byte) {
                break;
            }
            if byte == b'\\' {
                let start = cursor;
                while line.get(cursor) == Some(&b'\\') {
                    cursor += 1;
                }
                let count = cursor - start;
                if line.get(cursor) != Some(&b'"') {
                    argument.extend(std::iter::repeat_n(b'\\', count));
                    continue;
                }
                argument.extend(std::iter::repeat_n(b'\\', count / 2));
                if count % 2 != 0 {
                    argument.push(b'"');
                    cursor += 1;
                    continue;
                }
            }
            if line.get(cursor) == Some(&b'"') {
                if quoted && line.get(cursor + 1) == Some(&b'"') {
                    argument.push(b'"');
                    cursor += 1;
                } else {
                    quoted = !quoted;
                }
            } else {
                argument.push(line[cursor]);
            }
            cursor += 1;
        }
        arguments.push(argument);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_parameter_limit_and_mapping_failures_are_exact() {
        let entry = vec![b'='; MAX_BYTES - 19];
        let environment = [entry.as_slice()];
        let parameters = Parameters::prepare(ProcessOptions {
            command_line: b"",
            environment: &environment,
            diagnostic_imports: false,
            modules: &[],
            ..ProcessOptions::default()
        })
        .unwrap();
        assert_eq!(parameters.bytes.len(), MAX_BYTES);
        assert_eq!(
            parameters.map(&mut GuestMemory::new(15)),
            Err(MemoryError::PageLimitExceeded)
        );
        let mut memory = GuestMemory::new(16);
        parameters.map(&mut memory).unwrap();
        assert_eq!(
            parameters.map(&mut memory),
            Err(MemoryError::PageLimitExceeded)
        );
        let mut memory = GuestMemory::new(32);
        memory
            .map_zeroed(u64::from(BASE), PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(
            parameters.map(&mut memory),
            Err(MemoryError::AlreadyMapped {
                address: u64::from(BASE)
            })
        );
        let too_long = vec![b'='; MAX_BYTES - 18];
        assert!(matches!(
            Parameters::prepare(ProcessOptions {
                command_line: b"",
                environment: &[&too_long],
                diagnostic_imports: false,
                modules: &[],
                ..ProcessOptions::default()
            }),
            Err(LoadError::InvalidProcessParameters)
        ));
    }
}
