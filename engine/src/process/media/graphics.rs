use super::{EngineInstance, HostError, MediaError};
use crate::graphics::{BatchId, GraphicsContext, ResourceId};

impl EngineInstance {
    pub fn graphics(&self) -> Result<&GraphicsContext, HostError> {
        Ok(&self.media_contexts()?.graphics)
    }

    pub fn graphics_create_vertex_buffer(&mut self, bytes: &[u8]) -> Result<ResourceId, HostError> {
        self.media_contexts_mut()?
            .graphics
            .create_vertex_buffer(bytes)
            .map_err(|error| HostError::Media(MediaError::Graphics(error)))
    }

    pub fn graphics_update_vertex_buffer(
        &mut self,
        id: ResourceId,
        first_vertex: u32,
        bytes: &[u8],
    ) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .graphics
            .update_vertex_buffer(id, first_vertex, bytes)
            .map_err(|error| HostError::Media(MediaError::Graphics(error)))
    }

    pub fn graphics_destroy_vertex_buffer(&mut self, id: ResourceId) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .graphics
            .destroy_vertex_buffer(id)
            .map_err(|error| HostError::Media(MediaError::Graphics(error)))
    }

    pub fn graphics_draw_triangles(
        &mut self,
        id: ResourceId,
        first_vertex: u32,
        vertex_count: u32,
    ) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .graphics
            .draw_triangles(id, first_vertex, vertex_count)
            .map_err(|error| HostError::Media(MediaError::Graphics(error)))
    }

    pub fn graphics_clear(&mut self, rgba: [f32; 4]) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .graphics
            .clear(rgba)
            .map_err(|error| HostError::Media(MediaError::Graphics(error)))
    }

    pub fn graphics_present(&mut self, surface: u32) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .graphics
            .present(surface)
            .map_err(|error| HostError::Media(MediaError::Graphics(error)))
    }

    pub fn graphics_seal_batch(&mut self) -> Result<Option<BatchId>, HostError> {
        self.media_contexts_mut()?
            .graphics
            .seal_batch()
            .map_err(|error| HostError::Media(MediaError::Graphics(error)))
    }

    pub fn graphics_acknowledge_batch(&mut self, id: BatchId) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .graphics
            .acknowledge_batch(id)
            .map_err(|error| HostError::Media(MediaError::Graphics(error)))
    }
}
