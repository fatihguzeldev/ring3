use super::{
    FontDatabase32, LogicalFont32, ResolvedFont32,
    faces::{FontFace32, parse},
};
use crate::windows::gdi::GdiError32;
use std::sync::Arc;

pub(super) struct FontResource32 {
    pub id: u32,
    pub faces: Vec<Arc<FontFace32>>,
    pub enumerable: bool,
}
pub struct PreparedFontSource32 {
    identity: Arc<()>,
    id: u32,
    faces: Vec<Arc<FontFace32>>,
    enumerable: bool,
    size: usize,
}
impl PreparedFontSource32 {
    pub fn handle(&self) -> u32 {
        self.id
    }
    pub fn count(&self) -> u32 {
        self.faces.len() as u32
    }
}
impl FontDatabase32 {
    pub(crate) fn retire_process_resources(&mut self) {
        self.faces.truncate(2);
        self.resources.clear();
        self.retired.clear();
        self.bytes = 1_082_044;
    }

    fn collect_sources(&mut self) {
        let bytes = &mut self.bytes;
        self.retired.retain(|(_, source, size)| {
            if source.strong_count() == 0 {
                *bytes -= size;
                false
            } else {
                true
            }
        });
    }
    pub fn prepare_font_source(
        &mut self,
        bytes: &[u8],
        enumerable: bool,
    ) -> Result<PreparedFontSource32, GdiError32> {
        self.collect_sources();
        let count = rustybuzz::ttf_parser::fonts_in_collection(bytes).unwrap_or(1) as usize;
        if count == 0 {
            return Err(GdiError32::InvalidRequest);
        }
        if bytes.len() > self.byte_limit - self.bytes
            || self.resources.len() + self.retired.len() == 256
            || count > 256usize.saturating_sub(self.faces.len() - 2)
        {
            return Err(GdiError32::Capacity);
        }
        self.next_resource
            .checked_add(1)
            .ok_or(GdiError32::Exhausted)?;
        let source: Arc<[u8]> = Arc::from(bytes);
        let mut faces = Vec::new();
        faces
            .try_reserve_exact(count)
            .map_err(|_| GdiError32::Allocation)?;
        for index in 0..count {
            faces.push(parse(source.clone(), index as u32)?);
        }
        self.resources
            .try_reserve(1)
            .map_err(|_| GdiError32::Allocation)?;
        self.faces
            .try_reserve(count)
            .map_err(|_| GdiError32::Allocation)?;
        Ok(PreparedFontSource32 {
            identity: self.identity.clone(),
            id: self.next_resource,
            faces,
            enumerable,
            size: bytes.len(),
        })
    }
    pub fn check_font_source(&self, prepared: &PreparedFontSource32) -> Result<(), GdiError32> {
        if !Arc::ptr_eq(&self.identity, &prepared.identity) || prepared.id != self.next_resource {
            return Err(GdiError32::InvalidRequest);
        }
        if prepared.size > self.byte_limit - self.bytes
            || self.resources.len() + self.retired.len() == 256
            || prepared.faces.len() > 256usize.saturating_sub(self.faces.len() - 2)
        {
            return Err(GdiError32::Capacity);
        }
        Ok(())
    }
    pub fn commit_font_source(&mut self, prepared: PreparedFontSource32) -> u32 {
        self.check_font_source(&prepared)
            .expect("prepared font source authority");
        self.faces.extend(prepared.faces.iter().cloned());
        self.bytes += prepared.size;
        self.next_resource += 1;
        self.resources.push(FontResource32 {
            id: prepared.id,
            faces: prepared.faces,
            enumerable: prepared.enumerable,
        });
        prepared.id
    }
    pub fn add_memory_font(&mut self, bytes: &[u8]) -> Result<u32, GdiError32> {
        let prepared = self.prepare_font_source(bytes, false)?;
        Ok(self.commit_font_source(prepared))
    }
    pub fn remove_memory_font(&mut self, id: u32) -> Result<(), GdiError32> {
        let index = self
            .resources
            .iter()
            .position(|entry| entry.id == id)
            .ok_or(GdiError32::InvalidHandle)?;
        self.retired
            .try_reserve(1)
            .map_err(|_| GdiError32::Allocation)?;
        let resource = self.resources.remove(index);
        let source = &resource.faces[0].bytes;
        self.retired
            .push((id, Arc::downgrade(source), source.len()));
        self.faces
            .retain(|entry| !resource.faces.iter().any(|face| Arc::ptr_eq(entry, face)));
        drop(resource);
        self.collect_sources();
        Ok(())
    }
    pub fn source_is_live(&self, id: u32) -> bool {
        self.resources.iter().any(|source| source.id == id)
            || self
                .retired
                .iter()
                .any(|(retired, source, _)| *retired == id && source.strong_count() != 0)
    }
    pub fn source_count(&self, id: u32) -> Result<u32, GdiError32> {
        self.resources
            .iter()
            .find(|source| source.id == id)
            .map(|source| source.faces.len() as u32)
            .ok_or(GdiError32::InvalidHandle)
    }
    pub fn enumerable_fonts(&self) -> Result<Vec<ResolvedFont32>, GdiError32> {
        let mut fonts = Vec::new();
        fonts
            .try_reserve_exact(self.faces.len())
            .map_err(|_| GdiError32::Allocation)?;
        for face in &self.faces {
            if self.resources.iter().any(|source| {
                !source.enumerable && source.faces.iter().any(|entry| Arc::ptr_eq(entry, face))
            }) {
                continue;
            }
            let descriptor = LogicalFont32 {
                face: face.name.clone(),
                weight: face.design.weight,
                italic: face.design.italic,
                ..LogicalFont32::stock(13)
            };
            fonts.push(ResolvedFont32::from_face(descriptor, face.clone()));
        }
        Ok(fonts)
    }
}
