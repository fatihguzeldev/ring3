use super::context::{DEFAULT_BITMAP, stock_handle};
use super::{BitmapLayout32, Brush32, GdiContext32, GdiError32, GdiObject32, Pen32};

impl GdiContext32 {
    pub fn stock_object(index: u32) -> Result<u32, GdiError32> {
        stock(index)
            .map(|_| stock_handle(index))
            .ok_or(GdiError32::InvalidRequest)
    }
    pub fn object(&self, handle: u32) -> Result<GdiObject32, GdiError32> {
        if let Some(index) = super::colors::system_brush_index(handle) {
            return Ok(GdiObject32::Brush(Brush32::System(index)));
        }
        if handle == DEFAULT_BITMAP {
            return Ok(GdiObject32::Bitmap(
                BitmapLayout32::new(1, 1, 1, false).unwrap(),
            ));
        }
        if let Some(index) = handle.checked_sub(stock_handle(0))
            && let Some(object) = stock(index)
        {
            return Ok(object);
        }
        self.objects
            .binary_search_by_key(&handle, |entry| entry.handle)
            .ok()
            .map(|index| self.objects[index].object.clone())
            .filter(|object| !matches!(object, GdiObject32::Image(_)))
            .ok_or(GdiError32::InvalidHandle)
    }
    pub(crate) fn image_object(&self, handle: u32) -> Result<super::GdiImageObject32, GdiError32> {
        let index = self
            .objects
            .binary_search_by_key(&handle, |entry| entry.handle)
            .map_err(|_| GdiError32::InvalidHandle)?;
        let GdiObject32::Image(image) = self.objects[index].object else {
            return Err(GdiError32::InvalidHandle);
        };
        Ok(image)
    }
    pub(crate) fn delete_image_object(&mut self, handle: u32) -> Result<(), GdiError32> {
        self.image_object(handle)?;
        let index = self
            .objects
            .binary_search_by_key(&handle, |entry| entry.handle)
            .unwrap();
        self.objects.remove(index);
        Ok(())
    }
    pub fn object_type(&self, handle: u32) -> Result<u32, GdiError32> {
        if let Ok(dc) = self.dc(handle) {
            return Ok(if matches!(dc.kind, super::DcKind32::Memory { .. }) {
                10
            } else {
                3
            });
        }
        Ok(self.object(handle)?.object_type())
    }
    pub fn check_delete_object(&self, handle: u32) -> Result<(), GdiError32> {
        self.object(handle)?;
        self.check_write_object(handle)?;
        if self.is_stock(handle) {
            return Ok(());
        }
        if self.object_selected(handle) {
            return Err(GdiError32::SelectedObject);
        }
        Ok(())
    }
    pub fn delete_object(&mut self, handle: u32) -> Result<(), GdiError32> {
        self.check_delete_object(handle)?;
        if self.is_stock(handle) {
            return Ok(());
        }
        let index = self
            .objects
            .binary_search_by_key(&handle, |entry| entry.handle)
            .unwrap();
        self.objects.remove(index);
        Ok(())
    }
    pub fn is_stock(&self, handle: u32) -> bool {
        handle == DEFAULT_BITMAP
            || super::colors::system_brush_index(handle).is_some()
            || handle
                .checked_sub(stock_handle(0))
                .and_then(stock)
                .is_some()
    }
    pub fn replace_region(
        &mut self,
        handle: u32,
        region: super::Region32,
    ) -> Result<(), GdiError32> {
        let index = self
            .objects
            .binary_search_by_key(&handle, |entry| entry.handle)
            .map_err(|_| GdiError32::InvalidHandle)?;
        if !matches!(self.objects[index].object, GdiObject32::Region(_)) {
            return Err(GdiError32::InvalidHandle);
        }
        self.objects[index].object = GdiObject32::Region(region);
        Ok(())
    }
}
fn stock(index: u32) -> Option<GdiObject32> {
    Some(match index {
        0..=4 => GdiObject32::Brush(Brush32::Solid(
            [0x00ff_ffff, 0x00c0_c0c0, 0x0080_8080, 0x0040_4040, 0][index as usize],
        )),
        5 => GdiObject32::Brush(Brush32::Null),
        6..=8 => GdiObject32::Pen(Pen32 {
            style: if index == 8 { 5 } else { 0 },
            width: 1,
            extended: None,
            color: if index == 6 { 0x00ff_ffff } else { 0 },
        }),
        10..=14 | 16..=17 => GdiObject32::StockFont(index),
        15 => GdiObject32::StockPalette,
        18 => GdiObject32::Brush(Brush32::Solid(0x00ff_ffff)),
        19 => GdiObject32::Pen(Pen32 {
            style: 0,
            width: 1,
            extended: None,
            color: 0,
        }),
        _ => return None,
    })
}
