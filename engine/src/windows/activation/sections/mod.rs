mod builder;
mod com;
mod strings;

use super::{ActivationError32 as E, ActivationGraph32};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivationSectionKey32 {
    String(Vec<u16>),
    Guid([u8; 16]),
}
#[derive(Clone, Debug)]
pub struct ActivationSectionEntry32 {
    pub key: ActivationSectionKey32,
    pub offset: u32,
    pub length: u32,
    pub roster: u32,
}
#[derive(Clone, Debug)]
pub struct ActivationSection32 {
    pub id: u32,
    pub bytes: Vec<u8>,
    pub entries: Vec<ActivationSectionEntry32>,
    pub global_offset: u32,
    pub global_length: u32,
}
impl ActivationSection32 {
    pub fn find(&self, key: &ActivationSectionKey32) -> Option<&ActivationSectionEntry32> {
        self.entries.iter().find(|entry| match (&entry.key, key) {
            (ActivationSectionKey32::Guid(left), ActivationSectionKey32::Guid(right)) => {
                left == right
            }
            (ActivationSectionKey32::String(left), ActivationSectionKey32::String(right)) => {
                crate::windows::text::ordinal_key(left) == crate::windows::text::ordinal_key(right)
            }
            _ => false,
        })
    }
}
pub fn encode_activation_sections(
    graph: &ActivationGraph32,
    context: u32,
    limit: usize,
) -> Result<Vec<ActivationSection32>, E> {
    let mut sections = Vec::new();
    let mut remaining = limit;
    for id in [2, 3, 4, 5, 6, 7] {
        let section = match id {
            2 | 3 | 7 => strings::encode(graph, context, id, remaining)?,
            _ => com::encode(graph, context, id, remaining)?,
        };
        if let Some(section) = section {
            remaining = remaining
                .checked_sub(section.bytes.len())
                .ok_or(E::NotEnoughMemory)?;
            sections.push(section);
        }
    }
    Ok(sections)
}
pub fn class_alias(graph: &ActivationGraph32, context: u32, ordinal: u32) -> [u8; 16] {
    let classes = || {
        graph
            .assemblies()
            .iter()
            .flat_map(|assembly| &assembly.manifest.files)
            .flat_map(|file| &file.classes)
    };
    let step = classes().count() as u64 + 1;
    let mut serial = u64::from(ordinal);
    loop {
        let mut guid = [0; 16];
        guid[..4].copy_from_slice(&context.to_le_bytes());
        guid[4..8].copy_from_slice(&[0x52, 0x33, 0x00, 0x40]);
        guid[8] = 0x80;
        guid[9..].copy_from_slice(&serial.to_le_bytes()[..7]);
        if !classes().any(|class| class.clsid == guid) {
            return guid;
        }
        serial += step;
    }
}
