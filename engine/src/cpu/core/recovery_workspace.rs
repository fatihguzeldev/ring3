use super::recovery_packet as packet;

// the allocation is stable across leases; its bytes carry no persistent fp state.
pub(crate) struct RecoveryWorkspace {
    words: Box<[u64]>,
}

impl RecoveryWorkspace {
    pub(crate) const RESERVATION: usize =
        std::mem::size_of::<Self>() + packet::WORKSPACE_BYTES as usize;

    pub(crate) fn new(context: u32, continuation: u32, memory: u64) -> Self {
        let mut workspace = Self {
            words: vec![0; packet::WORKSPACE_BYTES as usize / 8].into_boxed_slice(),
        };
        workspace.set_word(packet::MAGIC_OFFSET, packet::MAGIC);
        workspace.set_word(packet::VERSION_OFFSET, packet::VERSION);
        workspace.set_word(packet::BYTES_OFFSET, packet::WORKSPACE_BYTES);
        workspace.set_word(packet::CAPACITY_OFFSET, packet::MAX_CAPTURE_COLORS);
        workspace.set_word(packet::CONTEXT, context);
        workspace.set_word(packet::CONTINUATION, continuation);
        workspace.words[packet::MEMORY as usize / 8] = memory;
        workspace.set_word(packet::CELLS, packet::CELLS_OFFSET);
        workspace
    }

    fn set_word(&mut self, offset: u32, value: u32) {
        let index = offset as usize / 8;
        let shift = (offset % 8) * 8;
        self.words[index] =
            (self.words[index] & !(u64::from(u32::MAX) << shift)) | (u64::from(value) << shift);
    }

    pub(crate) fn pointer(&mut self) -> usize {
        self.words.as_mut_ptr() as usize
    }

    pub(crate) fn is_disarmed(&self) -> bool {
        self.words[packet::STATE as usize / 8] as u32 == packet::IDLE
            && self.words[packet::ARMED_NONCE as usize / 8] == 0
            && self.words[packet::OUTPUT_MASK as usize / 8] == 0
    }

    pub(crate) fn arm(&mut self, nonce: u64) -> bool {
        if nonce == 0 || !self.is_disarmed() {
            return false;
        }
        self.words[packet::ARMED_NONCE as usize / 8] = nonce;
        true
    }

    pub(crate) fn disarm(&mut self) {
        self.set_word(packet::STATE, packet::IDLE);
        self.words[packet::ARMED_NONCE as usize / 8] = 0;
        self.words[packet::OUTPUT_MASK as usize / 8] = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_workspace_binding_is_stable_and_cannot_overlap_live_leases() {
        let mut workspace = RecoveryWorkspace::new(0x100, 0x200, 7);
        let pointer = workspace.pointer();
        assert_eq!(pointer % 8, 0);
        assert_eq!(workspace.words[0], 0x0000_0001_5233_504b);
        assert_eq!(workspace.words[1], (8192_u64 << 32) | 132096);
        assert_eq!(workspace.words[2], (0x200_u64 << 32) | 0x100);
        assert_eq!(workspace.words[3], 7);
        assert_eq!(workspace.words[5], 1024);
        let mut moved = vec![workspace].pop().unwrap();
        assert_eq!(moved.pointer(), pointer);
        moved.words[6] = u64::from(u32::MAX) << 32;
        moved.words[128] = 0x7ff8_dead_beef_0001;
        assert!(moved.arm(0x1234));
        let sealed = moved.words.clone();
        assert!(!moved.arm(0x5678));
        assert_eq!(moved.words, sealed);
        moved.disarm();
        assert!(moved.is_disarmed());
        assert_eq!(moved.words[6], u64::from(u32::MAX) << 32);
        assert_eq!(moved.words[128], 0x7ff8_dead_beef_0001);
        assert!(moved.arm(0x5678));
        assert_eq!(moved.pointer(), pointer);
        assert_eq!(
            moved.words.len() * 8 + std::mem::size_of::<RecoveryWorkspace>(),
            RecoveryWorkspace::RESERVATION
        );
    }
}
