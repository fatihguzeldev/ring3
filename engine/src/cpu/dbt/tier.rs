#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum CompilationTier32 {
    #[default]
    Baseline = 0,
    Optimized = 1,
}
