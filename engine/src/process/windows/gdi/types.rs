use crate::windows::gdi::GdiLimits32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GdiConfig32 {
    pub objects: usize,
    pub dcs: usize,
    pub saved_states: usize,
    pub bitmap_bytes: usize,
    pub bitmap_base: u32,
    pub bitmap_pages: u32,
    pub screen_width: u32,
    pub screen_height: u32,
}
impl Default for GdiConfig32 {
    fn default() -> Self {
        let limits = GdiLimits32::default();
        Self {
            objects: limits.objects,
            dcs: limits.dcs,
            saved_states: limits.saved_states,
            bitmap_bytes: 64 * 1024 * 1024,
            bitmap_base: 0x2c00_0000,
            bitmap_pages: 16_384,
            screen_width: 1024,
            screen_height: 768,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GdiFrame32 {
    pub format: u32,
    pub id: u64,
    pub revision: u64,
    pub window: u32,
    pub window_dc: bool,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bytes: u32,
    pub damage: crate::windows::user32::Rect32,
}
