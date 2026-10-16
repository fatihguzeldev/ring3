#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeDebugOutput32 {
    pub id: u64,
    pub component: u32,
    pub level: u32,
    bytes: [u8; 512],
    length: usize,
}
impl NativeDebugOutput32 {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}
#[derive(Default)]
pub(crate) struct NativeDiagnostics32 {
    last: Option<NativeDebugOutput32>,
    sequence: u64,
}
impl NativeDiagnostics32 {
    pub(crate) fn next(&self) -> Option<u64> {
        self.sequence.checked_add(1)
    }
    pub(crate) fn commit(
        &mut self,
        id: u64,
        component: u32,
        level: u32,
        bytes: [u8; 512],
        length: usize,
    ) {
        assert_eq!(self.next(), Some(id));
        self.last = Some(NativeDebugOutput32 {
            id,
            component,
            level,
            bytes,
            length,
        });
        self.sequence = id;
    }
    pub(crate) fn last(&self) -> Option<&NativeDebugOutput32> {
        self.last.as_ref()
    }
}
