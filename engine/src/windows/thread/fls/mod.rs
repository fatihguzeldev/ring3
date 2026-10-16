mod context;
mod operations;
#[cfg(test)]
mod tests;

use super::ThreadError32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FlsStep32 {
    pub operation: u64,
    pub cursor: usize,
    pub thread: u32,
    pub index: u32,
    pub value: u32,
    pub procedure: u32,
    pub complete: bool,
}
#[derive(Clone, Copy)]
struct Slot32 {
    callback: u32,
    freeing: bool,
}
#[derive(Clone, Copy)]
enum OperationKind32 {
    Free(u32),
    Exit(u32),
}
struct Operation32 {
    id: u64,
    owner: u32,
    kind: OperationKind32,
    cursor: usize,
    active: Option<FlsStep32>,
}
pub(crate) struct FlsContext32 {
    slots: Vec<Option<Slot32>>,
    rows: Vec<Option<Vec<u32>>>,
    operations: Vec<Operation32>,
    next: u64,
}
