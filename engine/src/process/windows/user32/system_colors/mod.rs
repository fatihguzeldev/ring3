mod calls;
mod continuation;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage32 {
    Notify,
    Repaint,
    Finish,
}

#[derive(Clone, Copy)]
pub(super) struct ColorChange32 {
    thread: u32,
    through: u32,
    after: u32,
    stage: Stage32,
    child: Option<u64>,
    completed: Option<u64>,
}
