use super::context::GdiRuntime32;

impl GdiRuntime32 {
    pub(super) fn system_color(&self, index: u32) -> u32 {
        self.colors.get(index).unwrap_or(0)
    }
}
