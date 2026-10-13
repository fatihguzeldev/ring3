mod logical;
mod moves;

#[derive(Clone, Copy)]
pub(super) struct VectorLayout {
    pub lower: u32,
    pub upper: u32,
}
