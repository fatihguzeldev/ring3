use super::{ActivationSection32, ActivationSectionKey32 as Key, builder::Builder32, class_alias};
use crate::windows::{
    activation::{ActivationError32 as E, ActivationGraph32, manifest::ComInterface32},
    com::ServerThreadingModel32,
};

pub(super) fn encode(
    graph: &ActivationGraph32,
    context: u32,
    id: u32,
    limit: usize,
) -> Result<Option<ActivationSection32>, E> {
    let count: usize = graph
        .assemblies()
        .iter()
        .map(|assembly| {
            let files: usize = assembly
                .manifest
                .files
                .iter()
                .map(|file| match id {
                    4 => file.classes.len() * 2,
                    5 => file.interfaces.len(),
                    6 => file.type_libraries.len(),
                    _ => 0,
                })
                .sum();
            files
                + if id == 5 {
                    assembly.manifest.interfaces.len()
                } else {
                    0
                }
        })
        .sum();
    if count == 0 {
        return Ok(None);
    }
    let mut output = Builder32::new(id, count, true, limit)?;
    let mut names = Vec::new();
    if id != 5 {
        let start = output.section.bytes.len() as u32;
        for file in graph
            .assemblies()
            .iter()
            .flat_map(|assembly| &assembly.manifest.files)
        {
            if id == 4 && !file.classes.is_empty() || id == 6 && !file.type_libraries.is_empty() {
                let (offset, length) = output.text(&file.name)?;
                names.push((file.name.as_str(), offset, length));
            }
        }
        output.word(32, start);
        output.word(36, output.section.bytes.len() as u32 - start);
    }
    let mut ordinal = 0;
    for (roster, assembly) in graph.assemblies().iter().enumerate() {
        let roster = roster as u32 + 1;
        if id == 5 {
            for record in &assembly.manifest.interfaces {
                interface(&mut output, record, roster)?;
            }
        }
        for file in &assembly.manifest.files {
            match id {
                4 => {
                    for record in &file.classes {
                        ordinal += 1;
                        let alias = class_alias(graph, context, ordinal);
                        output.align()?;
                        let data = output.zeroes(120)?;
                        let (name_offset, name_length) = names
                            .iter()
                            .find(|(name, _, _)| *name == file.name)
                            .map(|(_, offset, length)| (*offset, *length))
                            .unwrap();
                        output.word(data as usize, 120);
                        output.word(data as usize + 4, u32::from(record.misc_mask) << 8);
                        output.word(
                            data as usize + 8,
                            match record.threading {
                                ServerThreadingModel32::Apartment => 1,
                                ServerThreadingModel32::Free => 2,
                                ServerThreadingModel32::MainSta => 3,
                                ServerThreadingModel32::Both => 4,
                                ServerThreadingModel32::Neutral => 5,
                            },
                        );
                        output.section.bytes[data as usize + 12..data as usize + 28]
                            .copy_from_slice(&record.clsid);
                        output.section.bytes[data as usize + 28..data as usize + 44]
                            .copy_from_slice(&alias);
                        output.section.bytes[data as usize + 44..data as usize + 60]
                            .copy_from_slice(&record.clsid);
                        output.section.bytes[data as usize + 60..data as usize + 76]
                            .copy_from_slice(&record.typelib.unwrap_or([0; 16]));
                        output.word(data as usize + 76, name_length);
                        output.word(data as usize + 80, name_offset);
                        if let Some(progid) = record.progids.first() {
                            let (offset, length) = output.text(progid)?;
                            output.word(data as usize + 84, length);
                            output.word(data as usize + 88, offset - data);
                        }
                        for (index, value) in record.misc_status.iter().enumerate() {
                            output.word(data as usize + 100 + index * 4, *value);
                        }
                        let length = output.section.bytes.len() as u32 - data;
                        output.entry(Key::Guid(record.clsid), data, length, roster)?;
                        output.entry(Key::Guid(alias), data, length, roster)?;
                    }
                }
                5 => {
                    for record in &file.interfaces {
                        interface(&mut output, record, roster)?;
                    }
                }
                6 => {
                    for record in &file.type_libraries {
                        output.align()?;
                        let data = output.zeroes(32)?;
                        let (name_offset, name_length) = names
                            .iter()
                            .find(|(name, _, _)| *name == file.name)
                            .map(|(_, offset, length)| (*offset, *length))
                            .unwrap();
                        let (help_offset, help_length) = output.text(&record.help_directory)?;
                        output.word(data as usize, 32);
                        output.word(data as usize + 8, name_length);
                        output.word(data as usize + 12, name_offset);
                        output.word(data as usize + 16, u32::from(record.flags) << 16);
                        output.word(data as usize + 20, help_length);
                        output.word(data as usize + 24, help_offset - data);
                        output.word(
                            data as usize + 28,
                            u32::from(record.version[0]) | u32::from(record.version[1]) << 16,
                        );
                        let length = output.section.bytes.len() as u32 - data;
                        output.entry(Key::Guid(record.id), data, length, roster)?;
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    output.finish().map(Some)
}
fn interface(output: &mut Builder32, record: &ComInterface32, roster: u32) -> Result<(), E> {
    output.align()?;
    let data = output.zeroes(68)?;
    output.word(data as usize, 68);
    output.word(
        data as usize + 4,
        u32::from(record.methods.is_some()) | u32::from(record.base.is_some()) << 1,
    );
    output.section.bytes[data as usize + 8..data as usize + 24]
        .copy_from_slice(&record.proxy.unwrap_or(record.iid));
    output.word(data as usize + 24, record.methods.unwrap_or(0));
    output.section.bytes[data as usize + 28..data as usize + 44]
        .copy_from_slice(&record.typelib.unwrap_or([0; 16]));
    output.section.bytes[data as usize + 44..data as usize + 60]
        .copy_from_slice(&record.base.unwrap_or([0; 16]));
    if let Some(name) = &record.name {
        let (offset, length) = output.text(name)?;
        output.word(data as usize + 60, length);
        output.word(data as usize + 64, offset - data);
    }
    let length = output.section.bytes.len() as u32 - data;
    output.entry(Key::Guid(record.iid), data, length, roster)
}
