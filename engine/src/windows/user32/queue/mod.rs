mod context;

use super::Message32;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QueuedKey32 {
    pub virtual_key: u8,
    pub pressed: bool,
    pub repeated: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QueuedKeyboard32 {
    pub key: Option<QueuedKey32>,
    pub state: [u8; 256],
}
#[derive(Clone, Copy)]
pub(super) struct MessageSlot32 {
    message: Message32,
    keyboard: Option<QueuedKeyboard32>,
    input: bool,
    seen: u32,
}

pub(crate) struct MessageQueue32 {
    posted: VecDeque<MessageSlot32>,
    quits: Vec<MessageSlot32>,
    limit: usize,
    generation: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_observation_preserves_messages_and_only_hardware_records_carry_key_state() {
        let mut queue = MessageQueue32::new(4).unwrap();
        let message = Message32 {
            thread_id: 1,
            message: 0x100,
            wparam: 65,
            ..Default::default()
        };
        let key = QueuedKey32 {
            virtual_key: 65,
            pressed: true,
            repeated: false,
        };
        let mut state = [0; 256];
        state[65] = 0x80;
        queue.post(message).unwrap();
        queue.post_input(message, Some(key), state).unwrap();
        assert_eq!(queue.status(1), (0x109, 0x109, 2));
        queue.observe(1, u32::MAX);
        assert_eq!(queue.status(1), (0x109, 0, 2));
        assert!(queue.find(1, u32::MAX, |_| true).is_some());
        assert_eq!(queue.remove(0), None);
        assert_eq!(
            queue.remove(0),
            Some(QueuedKeyboard32 {
                key: Some(key),
                state
            })
        );
    }
}
