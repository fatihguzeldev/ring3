mod cross_lane;
mod integer;
mod logical;
mod moves;
mod multiply;
mod permute;
mod rearrange;
mod shift;

#[derive(Clone, Copy)]
pub(super) struct VectorLayout {
    pub lower: u32,
    pub upper: u32,
}
