use crate::process::{EngineInstance, HostError};

impl EngineInstance {
    pub(in crate::process) fn collect_gdi_font_file_holds(&mut self) -> Result<(), HostError> {
        let Some(runtime) = self.gdi.as_ref() else {
            return Ok(());
        };
        let mut releases = Vec::new();
        releases
            .try_reserve_exact(runtime.font_file_sources.len())
            .map_err(|_| HostError::Infrastructure)?;
        releases.extend(
            runtime
                .font_file_sources
                .iter()
                .filter(|source| {
                    source.references == 0 && !runtime.fonts.source_is_live(source.source)
                })
                .map(|source| source.lease),
        );
        self.check_close_font_source_files(&releases)?;
        self.commit_close_provider_files(&releases);
        if let Some(runtime) = self.gdi.as_mut() {
            runtime
                .font_file_sources
                .retain(|source| !releases.contains(&source.lease));
        }
        Ok(())
    }
}
