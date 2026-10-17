use super::context::GdiRuntime32;

impl GdiRuntime32 {
    pub(in crate::process) fn system_color(&self, index: u32) -> u32 {
        self.colors.get(index).unwrap_or(0)
    }
    pub(in crate::process) fn system_colors(&self) -> crate::windows::user32::SystemColors32 {
        self.colors
    }
    pub(in crate::process) fn set_system_colors(
        &mut self,
        colors: crate::windows::user32::SystemColors32,
    ) {
        self.colors = colors;
    }
}
