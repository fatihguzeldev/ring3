use super::super::{
    bitmaps::dib_info,
    context::GdiRuntime32,
    storage::{Storage32, SurfaceKey32},
};
use crate::{
    files::FileError32,
    memory::{Access, GuestAddress},
    process::{
        EngineInstance, HostError, calls::call::PendingOwner, windows::files::FileCallOutcome32,
    },
    windows::{
        CallFrame32,
        gdi::{GdiApi32, GdiError32, GdiObject32},
    },
};

enum SectionError32 {
    Gdi(GdiError32),
    File(FileError32),
}
impl From<GdiError32> for SectionError32 {
    fn from(error: GdiError32) -> Self {
        Self::Gdi(error)
    }
}
impl From<FileError32> for SectionError32 {
    fn from(error: FileError32) -> Self {
        Self::File(error)
    }
}
impl From<crate::memory::MemoryError> for SectionError32 {
    fn from(error: crate::memory::MemoryError) -> Self {
        Self::Gdi(error.into())
    }
}

impl GdiRuntime32 {
    fn section_lease(&self, handle: u32) -> Option<crate::process::windows::files::DibSection32> {
        self.surface_index(SurfaceKey32::Bitmap(handle))
            .ok()
            .and_then(|index| self.surfaces[index].storage.section())
    }
}
impl EngineInstance {
    pub(in crate::process::windows::gdi) fn complete_gdi_section(
        &mut self,
        owner: PendingOwner,
        token: u32,
        api: GdiApi32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let args = frame.arguments();
        let create = api == GdiApi32::CreateDIBSection && args[4] != 0;
        let lease = (api == GdiApi32::DeleteObject)
            .then(|| self.gdi.as_ref().and_then(|gdi| gdi.section_lease(args[0])))
            .flatten();
        if !create && lease.is_none() {
            return Ok(false);
        }
        if let Some(files) = &self.files {
            files.check_gdi_file_call(owner, token, api.id(), frame)?;
        }
        let result = self.execute_gdi_section(api, args, lease);
        let (value, error) = match result {
            Ok(FileCallOutcome32::PendingBytes(request)) => {
                self.files.as_mut().unwrap().park_gdi_file_call(
                    owner,
                    token,
                    api.id(),
                    frame,
                    request,
                );
                return Ok(true);
            }
            Ok(FileCallOutcome32::Return { value, last_error }) => (value, last_error),
            Ok(_) => unreachable!("DIB mapping has only byte-wait and return outcomes"),
            Err(SectionError32::Gdi(GdiError32::Memory(
                error @ (crate::memory::MemoryError::Allocation
                | crate::memory::MemoryError::VersionExhausted),
            ))) => return Err(HostError::Memory(error)),
            Err(SectionError32::File(
                error @ (FileError32::Allocation | FileError32::Exhausted),
            )) => {
                return Err(HostError::File(error));
            }
            Err(SectionError32::Gdi(error)) => (0, Some(error.win32())),
            Err(SectionError32::File(error)) => (0, Some(error.win32())),
        };
        let prepared = self.prepare_gdi_result(Some(api), frame, value, error)?;
        if !self.defer_guard_service_cleanup(
            crate::process::windows::service::WindowsServiceCleanup32::Files,
        ) && let Some(files) = self.files.as_mut()
        {
            files.finish_gdi_file_call();
        }
        self.publish_prepared_call(prepared);
        Ok(true)
    }
    fn execute_gdi_section(
        &mut self,
        api: GdiApi32,
        args: &[u32],
        lease: Option<crate::process::windows::files::DibSection32>,
    ) -> Result<FileCallOutcome32, SectionError32> {
        if api == GdiApi32::DeleteObject {
            let gdi = self.gdi.as_mut().ok_or(GdiError32::InvalidConfig)?;
            gdi.model.check_delete_object(args[0])?;
            let index = gdi.surface_index(SurfaceKey32::Bitmap(args[0]))?;
            gdi.surfaces[index]
                .storage
                .check(self.memory.as_ref().unwrap())?;
            let files = self.files.as_mut().ok_or(FileError32::InvalidHandle)?;
            let outcome = files.unmap_dib_section(self.memory.as_mut().unwrap(), lease.unwrap())?;
            if matches!(outcome, FileCallOutcome32::Return { value: 1, .. }) {
                let surface = gdi.surfaces.remove(index);
                gdi.bytes -= surface.layout.byte_len()?;
                gdi.model
                    .delete_object(args[0])
                    .expect("exact unselected DIB object checked before mapping retirement");
            }
            return Ok(outcome);
        }
        let reserved = self
            .guest_address_reservations()
            .map_err(|_| GdiError32::Allocation)?;
        let memory = self.memory.as_mut().unwrap();
        memory.check_span(GuestAddress(args[3]), 4, Access::Write)?;
        memory.ensure_transaction_versions(2)?;
        let (layout, colors) = dib_info(memory, args[1], args[2])?;
        let gdi = self.gdi.as_mut().ok_or(GdiError32::InvalidConfig)?;
        let prepared = gdi.model.prepare_object(GdiObject32::Bitmap(layout))?;
        let mut surface =
            gdi.prepare_surface(SurfaceKey32::Bitmap(prepared.handle()), layout, colors)?;
        let files = self.files.as_mut().ok_or(FileError32::InvalidHandle)?;
        let outcome = files.map_dib_section(memory, &reserved, args, layout.byte_len()?)?;
        let FileCallOutcome32::Return { value: bits, .. } = outcome else {
            return Ok(outcome);
        };
        if bits == 0 {
            return Ok(outcome);
        }
        let lease = files.dib_section_lease(memory, args[4], bits, args[5]);
        surface.storage = Storage32::Dib {
            range: lease.range,
            address: lease.bits,
            identity: lease.identity,
            mapping: lease.incarnation,
            section: Some(lease),
        };
        let handle = gdi.model.commit_new(prepared);
        gdi.publish_surface(surface);
        Ok(FileCallOutcome32::Return {
            value: handle,
            last_error: None,
        })
    }
}
