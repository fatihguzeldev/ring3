use crate::windows::activation::{
    ActivationError32 as E, ActivationGraph32,
    manifest::{AssemblyIdentity32, RequestedLevel32},
};

pub(super) struct QueryData32 {
    pub(super) bytes: Vec<u8>,
    pub(super) written: u32,
}
pub(super) fn build(
    graph: Option<&ActivationGraph32>,
    handle: u32,
    class: u32,
    index: [u32; 2],
    base: u32,
) -> Result<QueryData32, E> {
    let mut data = Data32 {
        bytes: vec![
            0;
            match class {
                1 => 8,
                2 => 48,
                3 => 88,
                4 => 20,
                5 => 12,
                6 => 4,
                _ => return Err(E::InvalidParameter),
            }
        ],
        base,
    };
    match class {
        1 => data.word(0, handle),
        2 => {
            if let Some(graph) = graph {
                let root = &graph.assemblies()[0];
                data.word(4, 1);
                data.word(8, graph.assemblies().len() as u32);
                data.word(12, 1);
                data.word(16, root.manifest_path.len() as u32);
                let pointer = data.text(&root.manifest_path)?;
                data.word(36, pointer);
                if let Some(path) = graph.configuration_path() {
                    data.word(20, 1);
                    data.word(24, path.len() as u32);
                    let pointer = data.text(path)?;
                    data.word(40, pointer);
                }
                data.word(28, 1);
                data.word(32, graph.application_directory().len() as u32);
                let pointer = data.text(graph.application_directory())?;
                data.word(44, pointer);
            }
        }
        3 => {
            let graph = graph.ok_or(E::InvalidParameter)?;
            let assembly = graph
                .assemblies()
                .get(index[0].checked_sub(1).ok_or(E::InvalidParameter)? as usize)
                .ok_or(E::InvalidParameter)?;
            let identity = encoded_identity(&assembly.manifest.identity);
            let identity = identity.encode_utf16().collect::<Vec<_>>();
            data.word(0, if index[0] == 1 { 1 } else { 0x10 });
            data.word(4, identity.len() as u32 * 2);
            let pointer = data.text(&identity)?;
            data.word(64, pointer);
            data.word(8, 1);
            data.word(12, assembly.manifest_path.len() as u32 * 2);
            data.bytes[16..24].copy_from_slice(&assembly.modified.to_le_bytes());
            let pointer = data.text(&assembly.manifest_path)?;
            data.word(68, pointer);
            data.word(44, 1);
            data.word(80, assembly.manifest.files.len() as u32);
            if index[0] != 1 {
                let directory = assembly
                    .directory
                    .strip_suffix(&[u16::from(b'\\')])
                    .unwrap_or(&assembly.directory);
                let start = directory
                    .iter()
                    .rposition(|&unit| unit == u16::from(b'\\'))
                    .map_or(0, |index| index + 1);
                let directory = &directory[start..];
                data.word(60, directory.len() as u32 * 2);
                let pointer = data.text(directory)?;
                data.word(76, pointer);
            }
        }
        4 => {
            let graph = graph.ok_or(E::InvalidParameter)?;
            let assembly = graph
                .assemblies()
                .get(index[0].checked_sub(1).ok_or(E::InvalidParameter)? as usize)
                .ok_or(E::InvalidParameter)?;
            let file = assembly
                .manifest
                .files
                .get(index[1] as usize)
                .ok_or(E::InvalidParameter)?;
            let name = file.name.encode_utf16().collect::<Vec<_>>();
            data.word(0, 2);
            data.word(4, name.len() as u32 * 2);
            let pointer = data.text(&name)?;
            data.word(12, pointer);
        }
        5 => {
            if let Some(level) =
                graph.and_then(|graph| graph.assemblies()[0].manifest.requested_level)
            {
                let (level, access) = match level {
                    RequestedLevel32::AsInvoker { ui_access } => (1, ui_access),
                    RequestedLevel32::HighestAvailable { ui_access } => (2, ui_access),
                    RequestedLevel32::RequireAdministrator { ui_access } => (3, ui_access),
                };
                data.word(4, level);
                data.word(8, u32::from(access));
            }
        }
        6 => {
            if let Some(graph) = graph {
                let os = &graph.assemblies()[0].manifest.supported_os;
                data.word(0, os.len() as u32);
                data.bytes
                    .try_reserve_exact(os.len() * 20)
                    .map_err(|_| E::NotEnoughMemory)?;
                for id in os {
                    data.bytes.extend_from_slice(id);
                    data.bytes.extend_from_slice(&1u32.to_le_bytes());
                }
            }
        }
        _ => unreachable!(),
    }
    let written = if class == 4 {
        0
    } else {
        data.bytes.len() as u32
    };
    Ok(QueryData32 {
        bytes: data.bytes,
        written,
    })
}
fn encoded_identity(identity: &AssemblyIdentity32) -> String {
    let mut result = identity.name.clone();
    if identity.language_specified {
        result.push_str(&format!(",language=\"{}\"", identity.language));
    }
    if identity.architecture_specified {
        result.push_str(&format!(
            ",processorArchitecture=\"{}\"",
            identity.architecture
        ));
    }
    if let Some(token) = identity.public_key_token {
        result.push_str(",publicKeyToken=\"");
        for byte in token {
            result.push_str(&format!("{byte:02x}"));
        }
        result.push('"');
    }
    let version = identity.version;
    result.push_str(&format!(
        ",type=\"win32\",version=\"{}.{}.{}.{}\"",
        version[0], version[1], version[2], version[3]
    ));
    result
}
struct Data32 {
    bytes: Vec<u8>,
    base: u32,
}
impl Data32 {
    fn word(&mut self, offset: usize, value: u32) {
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn text(&mut self, units: &[u16]) -> Result<u32, E> {
        let pointer = self
            .base
            .checked_add(
                self.bytes
                    .len()
                    .try_into()
                    .map_err(|_| E::NotEnoughMemory)?,
            )
            .ok_or(E::NoAccess)?;
        self.bytes
            .try_reserve_exact((units.len() + 1) * 2)
            .map_err(|_| E::NotEnoughMemory)?;
        self.bytes
            .extend(units.iter().copied().chain([0]).flat_map(u16::to_le_bytes));
        Ok(pointer)
    }
}
