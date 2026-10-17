use super::{DcKind32, DcState32, GdiError32, GdiLimits32, GdiObject32};

pub(super) const DEFAULT_BITMAP: u32 = 0x7f00_0100;
pub(super) const fn stock_handle(index: u32) -> u32 {
    0x7f00_0000 + index
}
pub(super) struct ObjectEntry32 {
    pub handle: u32,
    pub object: GdiObject32,
    pub font_metrics: Option<super::ResolvedFont32>,
    pub owner: super::ownership::ObjectOwner32,
}
pub(super) struct Dc32 {
    pub handle: u32,
    pub kind: DcKind32,
    pub state: DcState32,
    pub saved: Vec<DcState32>,
}
pub struct GdiContext32 {
    pub(super) limits: GdiLimits32,
    pub(super) objects: Vec<ObjectEntry32>,
    pub(super) dcs: Vec<Dc32>,
    next: u32,
}
enum PreparedPayload32 {
    Object(GdiObject32),
    Dc(Dc32),
}
pub struct PreparedGdiObject32 {
    handle: u32,
    retire: Option<u32>,
    payload: PreparedPayload32,
}
impl PreparedGdiObject32 {
    pub fn handle(&self) -> u32 {
        self.handle
    }
}
impl GdiContext32 {
    pub fn new(limits: GdiLimits32) -> Result<Self, GdiError32> {
        if !(1..=65_536).contains(&limits.objects)
            || !(1..=16_384).contains(&limits.dcs)
            || !(1..=256).contains(&limits.saved_states)
        {
            return Err(GdiError32::InvalidConfig);
        }
        let mut objects = Vec::new();
        let mut dcs = Vec::new();
        objects
            .try_reserve_exact(limits.objects)
            .map_err(|_| GdiError32::Allocation)?;
        dcs.try_reserve_exact(limits.dcs)
            .map_err(|_| GdiError32::Allocation)?;
        Ok(Self {
            limits,
            objects,
            dcs,
            next: 1,
        })
    }
    fn next_handle(&self) -> Result<u32, GdiError32> {
        if self.next > 0x00ff_ffff {
            Err(GdiError32::Exhausted)
        } else {
            Ok(0x4c00_0000 | self.next)
        }
    }
    pub fn prepare_object(&self, object: GdiObject32) -> Result<PreparedGdiObject32, GdiError32> {
        self.prepare_object_replacing(object, None)
    }
    pub(crate) fn prepare_object_replacing(
        &self,
        object: GdiObject32,
        retire: Option<u32>,
    ) -> Result<PreparedGdiObject32, GdiError32> {
        if let Some(handle) = retire {
            self.check_object_retirement(handle)?;
        }
        if self.objects.len() - usize::from(retire.is_some()) == self.limits.objects {
            return Err(GdiError32::Capacity);
        }
        Ok(PreparedGdiObject32 {
            handle: self.next_handle()?,
            retire,
            payload: PreparedPayload32::Object(object),
        })
    }
    pub fn prepare_dc(&self, kind: DcKind32) -> Result<PreparedGdiObject32, GdiError32> {
        if self.dcs.len() == self.limits.dcs {
            return Err(GdiError32::Capacity);
        }
        let handle = self.next_handle()?;
        let mut saved = Vec::new();
        saved
            .try_reserve_exact(self.limits.saved_states)
            .map_err(|_| GdiError32::Allocation)?;
        Ok(PreparedGdiObject32 {
            handle,
            retire: None,
            payload: PreparedPayload32::Dc(Dc32 {
                handle,
                kind,
                state: DcState32::default(),
                saved,
            }),
        })
    }
    fn check_object_retirement(&self, handle: u32) -> Result<(), GdiError32> {
        if self.is_stock(handle) {
            return Err(GdiError32::InvalidHandle);
        }
        if self.image_object(handle).is_ok() {
            return Ok(());
        }
        self.check_delete_object(handle)
    }
    pub(crate) fn check_prepared_new(
        &self,
        prepared: &PreparedGdiObject32,
    ) -> Result<(), GdiError32> {
        if self.next_handle()? != prepared.handle {
            return Err(GdiError32::InvalidRequest);
        }
        if let Some(handle) = prepared.retire {
            self.check_object_retirement(handle)?;
        }
        let capacity = match prepared.payload {
            PreparedPayload32::Object(_) => {
                self.objects.len() - usize::from(prepared.retire.is_some()) < self.limits.objects
            }
            PreparedPayload32::Dc(_) => self.dcs.len() < self.limits.dcs,
        };
        if !capacity {
            return Err(GdiError32::Capacity);
        }
        Ok(())
    }
    pub fn commit_new(&mut self, prepared: PreparedGdiObject32) -> u32 {
        self.check_prepared_new(&prepared)
            .expect("prepared GDI object authority");
        if let Some(handle) = prepared.retire {
            let index = self
                .objects
                .binary_search_by_key(&handle, |entry| entry.handle)
                .unwrap();
            self.objects.remove(index);
        }
        match prepared.payload {
            PreparedPayload32::Object(object) => self.objects.push(ObjectEntry32 {
                handle: prepared.handle,
                object,
                font_metrics: None,
                owner: super::ownership::ObjectOwner32::Application,
            }),
            PreparedPayload32::Dc(dc) => self.dcs.push(dc),
        }
        self.next += 1;
        prepared.handle
    }
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }
    pub fn dc_count(&self) -> usize {
        self.dcs.len()
    }
    pub fn object_handles(&self) -> impl Iterator<Item = u32> + '_ {
        self.objects.iter().map(|entry| entry.handle)
    }
    pub fn dc_handles(&self) -> impl Iterator<Item = u32> + '_ {
        self.dcs.iter().map(|entry| entry.handle)
    }
    pub fn clear(&mut self) {
        self.objects.clear();
        self.dcs.clear();
    }
}
