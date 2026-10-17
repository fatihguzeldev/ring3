use super::{DcKind32, DcState32, GdiContext32, GdiError32, GdiObject32, Region32};

impl GdiContext32 {
    pub(super) fn dc(&self, handle: u32) -> Result<&super::context::Dc32, GdiError32> {
        self.dcs
            .binary_search_by_key(&handle, |dc| dc.handle)
            .ok()
            .map(|index| &self.dcs[index])
            .ok_or(GdiError32::InvalidHandle)
    }
    pub fn dc_kind(&self, handle: u32) -> Result<DcKind32, GdiError32> {
        Ok(self.dc(handle)?.kind)
    }
    pub fn state(&self, handle: u32) -> Result<&DcState32, GdiError32> {
        Ok(&self.dc(handle)?.state)
    }
    pub fn set_state(&mut self, handle: u32, state: DcState32) -> Result<(), GdiError32> {
        let index = self
            .dcs
            .binary_search_by_key(&handle, |dc| dc.handle)
            .map_err(|_| GdiError32::InvalidHandle)?;
        self.dcs[index].state = state;
        Ok(())
    }
    pub fn set_display_visibility(
        &mut self,
        window: u32,
        window_dc: bool,
        bounds: crate::windows::user32::Rect32,
    ) {
        for dc in &mut self.dcs {
            if !matches!(dc.kind, DcKind32::Display { window: owner, window_dc: whole, .. } if owner == window && whole == window_dc)
            {
                continue;
            }
            for state in core::iter::once(&mut dc.state).chain(&mut dc.saved) {
                state.visible_bounds = Some(bounds);
            }
        }
    }
    pub(crate) fn set_dc_visible_region(
        &mut self,
        handle: u32,
        region: Option<std::sync::Arc<Region32>>,
    ) {
        let dc = self
            .dcs
            .iter_mut()
            .find(|dc| dc.handle == handle)
            .expect("accepted display dc remains current");
        for state in core::iter::once(&mut dc.state).chain(&mut dc.saved) {
            state.visible_region = region.clone();
        }
    }
    pub fn check_select(&self, dc: u32, object: u32) -> Result<u32, GdiError32> {
        let current = self.dc(dc)?;
        Ok(match self.object(object)? {
            GdiObject32::Image(_) => return Err(GdiError32::InvalidHandle),
            GdiObject32::Brush(_) => current.state.brush,
            GdiObject32::Pen(_) => current.state.pen,
            GdiObject32::StockFont(_) | GdiObject32::Font(_) => current.state.font,
            GdiObject32::StockPalette | GdiObject32::Palette(_) => {
                return Err(GdiError32::InvalidRequest);
            }
            GdiObject32::Bitmap(_) => {
                if !matches!(current.kind, DcKind32::Memory { .. }) {
                    return Err(GdiError32::InvalidRequest);
                }
                if !self.is_stock(object)
                    && self.dcs.iter().any(|other| {
                        other.handle != dc
                            && core::iter::once(&other.state)
                                .chain(&other.saved)
                                .any(|state| state.bitmap == object)
                    })
                {
                    return Err(GdiError32::SelectedObject);
                }
                current.state.bitmap
            }
            GdiObject32::Region(region) => region.complexity(),
        })
    }
    pub fn select_object(&mut self, dc: u32, object: u32) -> Result<u32, GdiError32> {
        let previous = self.check_select(dc, object)?;
        let object_type = self.object(object)?;
        let index = self.dcs.binary_search_by_key(&dc, |dc| dc.handle).unwrap();
        let state = &mut self.dcs[index].state;
        match object_type {
            GdiObject32::Image(_) => return Err(GdiError32::InvalidHandle),
            GdiObject32::Brush(_) => state.brush = object,
            GdiObject32::Pen(_) => state.pen = object,
            GdiObject32::StockFont(_) | GdiObject32::Font(_) => {
                state.font = object;
                state.font_realization = None;
            }
            GdiObject32::Bitmap(_) => state.bitmap = object,
            GdiObject32::Region(region) => state.clip = Some(region),
            GdiObject32::StockPalette | GdiObject32::Palette(_) => unreachable!(),
        }
        Ok(previous)
    }
    pub(crate) fn font_metric_snapshot(
        &self,
        handle: u32,
    ) -> Result<Option<super::ResolvedFont32>, GdiError32> {
        match self.object(handle)? {
            GdiObject32::StockFont(_) => Ok(None),
            GdiObject32::Font(_) => Ok(self
                .objects
                .iter()
                .find(|entry| entry.handle == handle)
                .unwrap()
                .font_metrics
                .clone()),
            _ => Err(GdiError32::InvalidHandle),
        }
    }
    pub(crate) fn bind_font_metric_snapshot(
        &mut self,
        handle: u32,
        proposed: super::ResolvedFont32,
    ) -> Result<(), GdiError32> {
        let logical = match self.object(handle)? {
            GdiObject32::StockFont(_) => return Ok(()),
            GdiObject32::Font(font) => font,
            _ => return Err(GdiError32::InvalidHandle),
        };
        if &logical != proposed.descriptor() {
            return Err(GdiError32::InvalidRequest);
        }
        let index = self
            .objects
            .iter()
            .position(|entry| entry.handle == handle)
            .unwrap();
        if self.objects[index].font_metrics.is_some() {
            return Ok(());
        }
        let shared = self.objects.iter().find_map(|entry| {
            matches!(&entry.object, GdiObject32::Font(font) if font == &logical)
                .then(|| entry.font_metrics.clone())
                .flatten()
        });
        self.objects[index].font_metrics = Some(shared.unwrap_or(proposed));
        Ok(())
    }
    pub(crate) fn restored_dc_state(&self, dc: u32, saved: i32) -> Result<DcState32, GdiError32> {
        let dc = self.dc(dc)?;
        let index = if saved > 0 {
            i64::from(saved) - 1
        } else if saved < 0 {
            dc.saved.len() as i64 + i64::from(saved)
        } else {
            return Err(GdiError32::InvalidRequest);
        };
        dc.saved
            .get(usize::try_from(index).map_err(|_| GdiError32::InvalidRequest)?)
            .cloned()
            .ok_or(GdiError32::InvalidRequest)
    }
    pub fn save_dc(&mut self, dc: u32) -> Result<u32, GdiError32> {
        let index = self
            .dcs
            .binary_search_by_key(&dc, |dc| dc.handle)
            .map_err(|_| GdiError32::InvalidHandle)?;
        if self.dcs[index].saved.len() == self.limits.saved_states {
            return Err(GdiError32::Capacity);
        }
        let state = self.dcs[index].state.clone();
        self.dcs[index].saved.push(state);
        Ok(self.dcs[index].saved.len() as u32)
    }
    pub fn restore_dc(&mut self, dc: u32, saved: i32) -> Result<(), GdiError32> {
        let dc = self
            .dcs
            .iter_mut()
            .find(|entry| entry.handle == dc)
            .ok_or(GdiError32::InvalidHandle)?;
        let index = if saved > 0 {
            saved as i64 - 1
        } else if saved < 0 {
            dc.saved.len() as i64 + i64::from(saved)
        } else {
            return Err(GdiError32::InvalidRequest);
        };
        if index < 0 || index >= dc.saved.len() as i64 {
            return Err(GdiError32::InvalidRequest);
        }
        dc.state = dc.saved[index as usize].clone();
        dc.saved.truncate(index as usize);
        Ok(())
    }
    pub fn set_clip(&mut self, dc: u32, clip: Option<Region32>) -> Result<u32, GdiError32> {
        let mut state = self.state(dc)?.clone();
        let complexity = clip.as_ref().map_or(2, Region32::complexity);
        state.clip = clip;
        self.set_state(dc, state)?;
        Ok(complexity)
    }
    pub fn delete_dc(&mut self, handle: u32) -> Result<(), GdiError32> {
        let index = self
            .dcs
            .binary_search_by_key(&handle, |dc| dc.handle)
            .map_err(|_| GdiError32::InvalidHandle)?;
        if matches!(self.dcs[index].kind, DcKind32::Display { .. }) {
            return Err(GdiError32::InvalidRequest);
        }
        self.dcs.remove(index);
        Ok(())
    }
    pub fn release_dc(
        &mut self,
        window: u32,
        handle: u32,
        thread: u32,
    ) -> Result<bool, GdiError32> {
        let index = self
            .dcs
            .binary_search_by_key(&handle, |dc| dc.handle)
            .map_err(|_| GdiError32::InvalidHandle)?;
        let DcKind32::Display {
            window: owner,
            thread: caller,
            private,
            class,
            ..
        } = self.dcs[index].kind
        else {
            return Ok(false);
        };
        if window != owner || caller != thread {
            return Ok(false);
        }
        if private || class.is_some() {
            return Ok(false);
        }
        self.dcs.remove(index);
        Ok(true)
    }
    pub fn persistent_dc(&self, window: u32, class: Option<u16>, window_dc: bool) -> Option<u32> {
        self.dcs.iter().find(|dc| matches!(dc.kind, DcKind32::Display { window: owner, private, class: existing, window_dc: whole, .. } if whole == window_dc && ((private && owner == window) || (class.is_some() && class == existing)))).map(|dc| dc.handle)
    }
    pub fn bind_display_dc(
        &mut self,
        handle: u32,
        window: u32,
        thread: u32,
    ) -> Result<(), GdiError32> {
        let index = self
            .dcs
            .binary_search_by_key(&handle, |dc| dc.handle)
            .map_err(|_| GdiError32::InvalidHandle)?;
        let DcKind32::Display {
            private,
            class,
            window_dc,
            ..
        } = self.dcs[index].kind
        else {
            return Err(GdiError32::InvalidHandle);
        };
        self.dcs[index].kind = DcKind32::Display {
            window,
            thread,
            private,
            class,
            window_dc,
        };
        Ok(())
    }
    pub fn retire_window(&mut self, window: u32) {
        self.dcs.retain(|dc| !matches!(dc.kind, DcKind32::Display { window: owner, class: None, .. } if owner == window));
    }
    pub fn retire_thread(&mut self, thread: u32) {
        self.dcs.retain(|dc| !matches!(dc.kind, DcKind32::Memory { owner: Some(owner) } if owner == thread) && !matches!(dc.kind, DcKind32::Display { thread: owner, private: false, class: None, .. } if owner == thread));
    }
}
