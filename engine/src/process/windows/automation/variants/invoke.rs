#[derive(Clone, Copy)]
pub(in crate::process::windows::automation) struct InvokeParameters32 {
    pub locale: u32,
    pub flags: u16,
    pub target: u16,
}
