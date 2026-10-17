use super::{GdiContext32, GdiError32, GdiObject32};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GdiClipboardObject32 {
    Bitmap(u32),
    Palette(u32),
}
impl GdiClipboardObject32 {
    pub(crate) fn handle(self) -> u32 {
        match self {
            Self::Bitmap(handle) | Self::Palette(handle) => handle,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ObjectOwner32 {
    Application,
    Clipboard,
    Retired,
}
impl GdiContext32 {
    pub(crate) fn object_selected(&self, handle: u32) -> bool {
        self.dcs.iter().any(|dc| {
            core::iter::once(&dc.state).chain(&dc.saved).any(|state| {
                [
                    state.bitmap,
                    state.brush,
                    state.pen,
                    state.font,
                    state.palette,
                ]
                .contains(&handle)
            })
        })
    }
    pub(crate) fn check_write_object(&self, handle: u32) -> Result<(), GdiError32> {
        if self.is_stock(handle) {
            return Ok(());
        }
        let entry = self
            .objects
            .iter()
            .find(|entry| entry.handle == handle)
            .ok_or(GdiError32::InvalidHandle)?;
        if entry.owner != ObjectOwner32::Application {
            return Err(GdiError32::SelectedObject);
        }
        Ok(())
    }
    pub(crate) fn capture_clipboard_object(
        &self,
        format: u32,
        handle: u32,
    ) -> Result<GdiClipboardObject32, GdiError32> {
        let entry = self
            .objects
            .iter()
            .find(|entry| entry.handle == handle)
            .ok_or(GdiError32::InvalidHandle)?;
        if entry.owner == ObjectOwner32::Retired {
            return Err(GdiError32::InvalidHandle);
        }
        match (format, &entry.object) {
            (2, GdiObject32::Bitmap(_)) => Ok(GdiClipboardObject32::Bitmap(handle)),
            (9, GdiObject32::Palette(_)) => Ok(GdiClipboardObject32::Palette(handle)),
            _ => Err(GdiError32::InvalidHandle),
        }
    }
    pub(crate) fn check_clipboard_object(
        &self,
        object: GdiClipboardObject32,
    ) -> Result<(), GdiError32> {
        let format = match object {
            GdiClipboardObject32::Bitmap(_) => 2,
            GdiClipboardObject32::Palette(_) => 9,
        };
        if self.capture_clipboard_object(format, object.handle())? != object {
            return Err(GdiError32::InvalidRequest);
        }
        if self
            .objects
            .iter()
            .find(|entry| entry.handle == object.handle())
            .unwrap()
            .owner
            != ObjectOwner32::Clipboard
        {
            return Err(GdiError32::InvalidRequest);
        }
        Ok(())
    }
    pub(crate) fn commit_clipboard_transfer(&mut self, object: GdiClipboardObject32) {
        self.objects
            .iter_mut()
            .find(|entry| entry.handle == object.handle())
            .expect("checked canonical clipboard object")
            .owner = ObjectOwner32::Clipboard;
    }
    pub(crate) fn defer_clipboard_retirement(&mut self, object: GdiClipboardObject32) {
        self.objects
            .iter_mut()
            .find(|entry| entry.handle == object.handle())
            .expect("checked canonical clipboard object")
            .owner = ObjectOwner32::Retired;
    }
    pub(crate) fn retired_clipboard_objects(
        &self,
    ) -> impl Iterator<Item = GdiClipboardObject32> + '_ {
        self.objects
            .iter()
            .filter(|entry| entry.owner == ObjectOwner32::Retired)
            .filter_map(|entry| match entry.object {
                GdiObject32::Bitmap(_) => Some(GdiClipboardObject32::Bitmap(entry.handle)),
                GdiObject32::Palette(_) => Some(GdiClipboardObject32::Palette(entry.handle)),
                _ => None,
            })
    }
    pub(crate) fn commit_clipboard_retirement(&mut self, object: GdiClipboardObject32) {
        let index = self
            .objects
            .iter()
            .position(|entry| entry.handle == object.handle())
            .expect("checked clipboard retirement identity");
        assert!(!self.object_selected(object.handle()));
        self.objects.remove(index);
    }
}
