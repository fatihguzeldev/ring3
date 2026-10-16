use super::ActivationContexts32;
use crate::windows::{
    activation::{ActivationError32 as E, ComRedirection32, graph::file_path},
    text::ordinal_key,
};

impl ActivationContexts32 {
    pub fn active_com_redirection(
        &self,
        thread: u32,
        clsid: [u8; 16],
    ) -> Result<Option<ComRedirection32>, E> {
        for handle in self.effective_lookup_handles(thread) {
            if handle == 0 {
                continue;
            }
            let mut ordinal = 0;
            for (index, assembly) in self.graph(handle)?.assemblies().iter().enumerate() {
                for file in &assembly.manifest.files {
                    for class in &file.classes {
                        ordinal += 1;
                        if class.clsid != clsid
                            && super::super::sections::class_alias(
                                self.graph(handle)?,
                                handle,
                                ordinal,
                            ) != clsid
                        {
                            continue;
                        }
                        return Ok(Some(ComRedirection32 {
                            context: handle,
                            assembly_roster: index as u32 + 1,
                            clsid: class.clsid,
                            server: file_path(&assembly.directory, &file.name),
                            threading: class.threading,
                            progid: class
                                .progids
                                .first()
                                .map(|value| value.encode_utf16().collect()),
                            typelib: class.typelib,
                        }));
                    }
                }
            }
        }
        Ok(None)
    }
    pub fn active_dll_redirection(&self, thread: u32, name: &[u16]) -> Result<Option<Vec<u16>>, E> {
        let key = ordinal_key(name);
        for handle in self.effective_lookup_handles(thread) {
            if handle == 0 {
                continue;
            }
            for assembly in self.graph(handle)?.assemblies() {
                for file in &assembly.manifest.files {
                    if super::super::graph::key(&file.name) == key {
                        return Ok(Some(file_path(&assembly.directory, &file.name)));
                    }
                }
            }
        }
        Ok(None)
    }
}
