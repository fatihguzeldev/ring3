use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::gdi::{
        GdiRuntime32, bitmaps, memory,
        storage::{Storage32, Surface32, SurfaceKey32},
    },
    windows::gdi::{Brush32, DcState32, GdiError32, GdiObject32},
};
impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn create_dib_pattern_brush(
        &mut self,
        ram: &AddressSpace,
        address: u32,
        usage: u32,
    ) -> Result<u32, GdiError32> {
        self.create_dib_pattern_brush_bounded(ram, address, usage, None)
    }
    pub(in crate::process::windows::gdi) fn create_dib_pattern_brush_bounded(
        &mut self,
        ram: &AddressSpace,
        address: u32,
        usage: u32,
        limit: Option<u32>,
    ) -> Result<u32, GdiError32> {
        let object = self
            .model
            .prepare_object(GdiObject32::Brush(Brush32::DibPattern { address, usage }))?;
        let surface =
            self.prepare_dib_pattern_surface(ram, address, usage, object.handle(), limit)?;
        let handle = self.model.commit_new(object);
        self.publish_surface(surface);
        Ok(handle)
    }
    pub(in crate::process::windows::gdi) fn prepare_dib_pattern_surface(
        &self,
        ram: &AddressSpace,
        address: u32,
        usage: u32,
        handle: u32,
        limit: Option<u32>,
    ) -> Result<Surface32, GdiError32> {
        if usage > 1 {
            return Err(GdiError32::InvalidRequest);
        }
        if limit.is_some_and(|limit| limit < 40) {
            return Err(GdiError32::InvalidRequest);
        }
        let header = memory::words(ram, address)?;
        let needed_header = header[0].max(if header[4] == 3 { 52 } else { 40 });
        if limit.is_some_and(|limit| needed_header > limit) {
            return Err(GdiError32::InvalidRequest);
        }
        let info = bitmaps::dib_layout(ram, address, header)?;
        let size = if usage == 1 { 2 } else { 4 };
        let table_len = info.colors * size;
        let table_at = address
            .checked_add(info.table_offset)
            .ok_or(GdiError32::InvalidRequest)?;
        let pixels_at = table_at
            .checked_add(table_len as u32)
            .ok_or(GdiError32::InvalidRequest)?;
        let total = info
            .table_offset
            .checked_add(table_len as u32)
            .and_then(|value| value.checked_add(u32::try_from(info.layout.byte_len().ok()?).ok()?))
            .ok_or(GdiError32::Capacity)?;
        if limit.is_some_and(|limit| total > limit) {
            return Err(GdiError32::InvalidRequest);
        }
        let mut table = memory::zeroes(table_len)?;
        ram.read(GuestAddress(table_at), &mut table)?;
        let mut colors = Vec::new();
        colors
            .try_reserve_exact(info.colors)
            .map_err(|_| GdiError32::Allocation)?;
        if usage == 1 {
            colors.extend(
                table
                    .chunks_exact(2)
                    .map(|part| u32::from(u16::from_le_bytes(part.try_into().unwrap()))),
            );
        } else {
            colors.extend(table.chunks_exact(4).map(bitmaps::quad_to_color));
        }
        let mut surface =
            self.prepare_surface(SurfaceKey32::PatternBrush(handle), info.layout, colors)?;
        let Storage32::Owned(pixels) = &mut surface.storage else {
            unreachable!()
        };
        ram.read(GuestAddress(pixels_at), pixels)?;
        Ok(surface)
    }
    pub(in crate::process::windows::gdi) fn realize_pattern_palette(
        &self,
        state: &DcState32,
        handle: u32,
    ) -> Result<Option<std::sync::Arc<[u32]>>, GdiError32> {
        let brush = match self.model.object(handle)? {
            GdiObject32::Brush(brush) => brush,
            GdiObject32::Pen(pen) => match pen.extended {
                Some(extended) => extended.brush,
                None => return Ok(None),
            },
            _ => return Ok(None),
        };
        if !matches!(brush, Brush32::DibPattern { usage: 1, .. }) {
            return Ok(None);
        }
        let surface = &self.surfaces[self.surface_index(SurfaceKey32::PatternBrush(handle))?];
        if surface.layout.bits_per_pixel > 8 {
            return Ok(None);
        }
        let palette = self.model.palette_entries(state.palette)?;
        let mut colors = Vec::new();
        colors
            .try_reserve_exact(surface.palette.len())
            .map_err(|_| GdiError32::Allocation)?;
        colors.extend(
            surface
                .palette
                .iter()
                .map(|index| palette[*index as usize % palette.len()] & 0xffffff),
        );
        Ok(Some(colors.into()))
    }
}
