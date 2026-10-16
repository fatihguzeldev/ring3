use super::{ActivationSection32, ActivationSectionKey32 as Key, builder::Builder32, class_alias};
use crate::windows::activation::{ActivationError32 as E, ActivationGraph32};

pub(super) fn encode(
    graph: &ActivationGraph32,
    context: u32,
    id: u32,
    limit: usize,
) -> Result<Option<ActivationSection32>, E> {
    let count = graph
        .assemblies()
        .iter()
        .flat_map(|assembly| &assembly.manifest.files)
        .map(|file| match id {
            2 => 1,
            3 => file.window_classes.len(),
            7 => file.classes.iter().map(|class| class.progids.len()).sum(),
            _ => 0,
        })
        .sum();
    if count == 0 {
        return Ok(None);
    }
    let mut output = Builder32::new(id, count, false, limit)?;
    let mut aliases = Vec::new();
    if id == 7 {
        let global = output.section.bytes.len() as u32;
        let mut ordinal = 0;
        for class in graph
            .assemblies()
            .iter()
            .flat_map(|assembly| &assembly.manifest.files)
            .flat_map(|file| &file.classes)
        {
            ordinal += 1;
            let offset = output.bytes(&class_alias(graph, context, ordinal))?;
            aliases.push((class.clsid, offset));
        }
        output.global(global, output.section.bytes.len() as u32 - global);
    }
    for (roster, assembly) in graph.assemblies().iter().enumerate() {
        for file in &assembly.manifest.files {
            if id == 2 {
                output.align()?;
                let data = output.zeroes(20)?;
                output.word(data as usize, 20);
                output.word(data as usize + 4, 2);
                output.entry(
                    Key::String(file.name.encode_utf16().collect()),
                    data,
                    20,
                    roster as u32 + 1,
                )?;
            } else if id == 3 {
                for window in &file.window_classes {
                    output.align()?;
                    let data = output.zeroes(24)?;
                    let version = assembly.manifest.identity.version;
                    let name = if window.versioned {
                        format!(
                            "{}.{}.{}.{}!{}",
                            version[0], version[1], version[2], version[3], window.name
                        )
                    } else {
                        window.name.clone()
                    };
                    let (name_offset, name_length) = output.text(&name)?;
                    let (module_offset, module_length) = output.text(&file.name)?;
                    output.word(data as usize, 24);
                    output.word(data as usize + 8, name_length);
                    output.word(data as usize + 12, name_offset - data);
                    output.word(data as usize + 16, module_length);
                    output.word(data as usize + 20, module_offset);
                    let length = output.section.bytes.len() as u32 - data;
                    output.entry(
                        Key::String(window.name.encode_utf16().collect()),
                        data,
                        length,
                        roster as u32 + 1,
                    )?;
                }
            } else {
                for class in &file.classes {
                    for progid in &class.progids {
                        output.align()?;
                        let data = output.zeroes(12)?;
                        output.word(data as usize, 12);
                        output.word(
                            data as usize + 8,
                            aliases
                                .iter()
                                .find(|(clsid, _)| *clsid == class.clsid)
                                .unwrap()
                                .1,
                        );
                        output.entry(
                            Key::String(progid.encode_utf16().collect()),
                            data,
                            12,
                            roster as u32 + 1,
                        )?;
                    }
                }
            }
        }
    }
    output.finish().map(Some)
}
