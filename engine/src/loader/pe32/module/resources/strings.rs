use super::{PeResources32, ResourceError32 as E, ResourceLocation32, ResourceString32};
impl PeResources32<'_> {
    pub fn string_table(
        &self,
        location: ResourceLocation32,
        index: u32,
    ) -> Result<ResourceString32, E> {
        if index >= 16 {
            return Err(E::Malformed);
        }
        let end = u64::from(location.data_rva) + u64::from(location.size);
        self.image_span(location.data_rva, location.size)?;
        let mut rva = location.data_rva;
        for current in 0..=index {
            if u64::from(rva) + 2 > end {
                return Err(E::Malformed);
            }
            let units = u32::from(self.half(rva)?);
            let start = rva.checked_add(2).ok_or(E::Malformed)?;
            let next = u64::from(start) + u64::from(units) * 2;
            if next > end {
                return Err(E::Malformed);
            }
            if current == index {
                return Ok(ResourceString32 {
                    data_rva: start,
                    units,
                });
            }
            rva = u32::try_from(next).map_err(|_| E::Malformed)?;
        }
        unreachable!()
    }
}
