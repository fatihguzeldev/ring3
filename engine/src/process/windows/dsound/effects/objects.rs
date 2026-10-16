use crate::{
    audio::effects::{EffectKind, Parameters},
    memory::GuestAddress,
};

pub(in crate::process::windows::dsound) struct EffectObject32 {
    pub buffer: GuestAddress,
    pub generation: u64,
    pub index: usize,
    pub kind: EffectKind,
    pub detached: Option<DetachedEffect32>,
}

#[derive(Clone, Copy)]
pub(in crate::process::windows::dsound) struct DetachedEffect32 {
    pub parameters: Parameters,
    pub quality: u32,
    pub preset: u32,
}
