use super::super::{
    commands::{Command, Present, Progress, SignedRect},
    resources::{Format, Rect, SurfaceRef, Usage},
};
use super::{Device, DeviceError, Outcome};

impl Device {
    pub fn set_render_targets(
        &mut self,
        color: Option<SurfaceRef>,
        depth: Option<SurfaceRef>,
    ) -> Result<(), DeviceError> {
        let prepared = self.prepare_targets(color, depth)?;
        self.commit_targets(prepared)
    }

    pub fn clear(
        &mut self,
        flags: u32,
        color: u32,
        depth: u32,
        stencil: u32,
        rectangles: &[Rect],
    ) -> Result<Outcome<()>, DeviceError> {
        self.validate_clear(flags, depth)?;
        let viewport = self.state.snapshot().viewport;
        let extent = Rect {
            left: viewport.x,
            top: viewport.y,
            right: viewport.x + viewport.width,
            bottom: viewport.y + viewport.height,
        };
        let mut clipped = Vec::new();
        clipped
            .try_reserve_exact(rectangles.len().max(1))
            .map_err(|_| DeviceError::Allocation)?;
        if rectangles.is_empty() {
            clipped.push(extent);
        } else {
            for rectangle in rectangles {
                if rectangle.left >= rectangle.right || rectangle.top >= rectangle.bottom {
                    return Err(DeviceError::InvalidCall);
                }
                let rectangle = Rect {
                    left: rectangle.left.max(extent.left),
                    top: rectangle.top.max(extent.top),
                    right: rectangle.right.min(extent.right),
                    bottom: rectangle.bottom.min(extent.bottom),
                };
                if rectangle.left < rectangle.right && rectangle.top < rectangle.bottom {
                    clipped.push(rectangle);
                }
            }
        }
        if clipped.is_empty() {
            return Ok(Outcome::new((), Progress::Queued));
        }
        let targets = self.output_targets(flags & 1 != 0, flags & 6 != 0)?;
        let prepared = self.queue.prepare(&[Command::Clear {
            targets,
            flags,
            color,
            depth,
            stencil,
            rectangles: &clipped,
        }])?;
        let progress = self.queue.commit(prepared)?;
        self.commit_target_writes(flags & 1 != 0, flags & 6 != 0)?;
        Ok(Outcome::new((), progress))
    }

    pub fn present(
        &mut self,
        swap_chain: u64,
        window: u32,
        source_rect: Option<Rect>,
        destination_rect: Option<SignedRect>,
        dirty_rects: &[Rect],
    ) -> Result<Outcome<()>, DeviceError> {
        self.present_surface(
            self.back_buffers[0],
            swap_chain,
            if window == 0 {
                self.description.window
            } else {
                window
            },
            source_rect,
            destination_rect,
            dirty_rects,
            !dirty_rects.is_empty(),
            self.description.swap_effect,
            self.description.interval,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn present_surface(
        &mut self,
        surface: SurfaceRef,
        swap_chain: u64,
        window: u32,
        source_rect: Option<Rect>,
        destination_rect: Option<SignedRect>,
        dirty_rects: &[Rect],
        dirty_region: bool,
        swap_effect: u32,
        interval: u32,
    ) -> Result<Outcome<()>, DeviceError> {
        self.available()?;
        if self.scene
            || swap_chain == 0
            || window == 0
            || !matches!(swap_effect, 1..=4)
            || (swap_effect <= 2 && (source_rect.is_some() || destination_rect.is_some()))
            || (swap_effect <= 2 && dirty_region)
            || (!dirty_region && !dirty_rects.is_empty())
        {
            return Err(DeviceError::InvalidCall);
        }
        self.drain_retired()?;
        self.ensure_resource_idle(surface.resource)?;
        let desc = self.resources.desc(surface.resource)?;
        if self.resources.is_locked(surface.resource)?
            || !desc.usage().contains(Usage::RENDER_TARGET)
            || desc.format().is_depth()
        {
            return Err(DeviceError::InvalidCall);
        }
        let source = self.surface_version(surface)?;
        let info = self.resources.info(surface)?;
        for rectangle in source_rect.iter().chain(dirty_rects) {
            if rectangle.left >= rectangle.right
                || rectangle.top >= rectangle.bottom
                || rectangle.right > info.width
                || rectangle.bottom > info.height
            {
                return Err(DeviceError::InvalidCall);
            }
        }
        if destination_rect.is_some_and(|rect| rect.left >= rect.right || rect.top >= rect.bottom) {
            return Err(DeviceError::InvalidCall);
        }
        let progress = self.queue.submit(&Command::Present(Present {
            swap_chain,
            window,
            source,
            source_rect,
            destination_rect,
            dirty_rects,
            dirty_region,
            interval,
            swap_effect,
        }))?;
        self.presented = Some(source);
        Ok(Outcome::new((), progress))
    }

    pub fn set_palette_entries(
        &mut self,
        index: u32,
        entries: &[[u8; 4]; 256],
    ) -> Result<Outcome<()>, DeviceError> {
        self.available()?;
        let position = self
            .palettes
            .iter()
            .position(|(stored, _)| *stored == index);
        if index > u16::MAX as u32 || (position.is_none() && self.palettes.len() == 256) {
            return Err(DeviceError::Capacity);
        }
        let progress = self.queue.submit(&Command::Palette { index, entries })?;
        if let Some(position) = position {
            self.palettes[position].1 = *entries;
        } else {
            self.palettes.push((index, *entries));
        }
        Ok(Outcome::new((), progress))
    }

    pub fn palette_entries(&self, index: u32) -> Result<&[[u8; 4]; 256], DeviceError> {
        self.palettes
            .iter()
            .find(|(stored, _)| *stored == index)
            .map(|(_, entries)| entries)
            .ok_or(DeviceError::InvalidCall)
    }

    pub fn set_gamma(
        &mut self,
        flags: u32,
        ramp: &[[u16; 256]; 3],
    ) -> Result<Outcome<()>, DeviceError> {
        self.available()?;
        if flags & !1 != 0 {
            return Err(DeviceError::InvalidCall);
        }
        let progress = self.queue.submit(&Command::Gamma { flags, ramp })?;
        self.gamma = *ramp;
        Ok(Outcome::new((), progress))
    }

    pub fn gamma(&self) -> &[[u16; 256]; 3] {
        &self.gamma
    }

    pub fn set_cursor_shape(
        &mut self,
        surface: SurfaceRef,
        hotspot: (u32, u32),
    ) -> Result<Outcome<()>, DeviceError> {
        self.available()?;
        let info = self.resources.info(surface)?;
        if self.resources.desc(surface.resource)?.format() != Format::A8R8G8B8
            || info.depth != 1
            || hotspot.0 >= info.width
            || hotspot.1 >= info.height
            || self.resources.is_locked(surface.resource)?
        {
            return Err(DeviceError::InvalidCall);
        }
        let source = self.surface_version(surface)?;
        let progress = self
            .queue
            .submit(&Command::CursorShape { source, hotspot })?;
        Ok(Outcome::new((), progress))
    }

    pub fn set_cursor_position(
        &mut self,
        x: i32,
        y: i32,
        flags: u32,
    ) -> Result<Outcome<()>, DeviceError> {
        self.available()?;
        if flags & !1 != 0 {
            return Err(DeviceError::InvalidCall);
        }
        let progress = self
            .queue
            .submit(&Command::CursorPosition { x, y, flags })?;
        Ok(Outcome::new((), progress))
    }

    pub fn show_cursor(&mut self, visible: bool) -> Result<Outcome<bool>, DeviceError> {
        self.available()?;
        let progress = self.queue.submit(&Command::CursorVisible(visible))?;
        let previous = std::mem::replace(&mut self.cursor_visible, visible);
        Ok(Outcome::new(previous, progress))
    }
}
