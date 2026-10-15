pub const KEY_COUNT: usize = 256;
pub const BUTTON_COUNT: usize = 8;
/// leaves room for every synthetic release and its focus-loss boundary.
pub const MIN_QUEUE_EVENTS: usize = KEY_COUNT + BUTTON_COUNT + 1;
pub const MAX_QUEUE_EVENTS: usize = 65_536;
pub const MAX_BATCH_EVENTS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputLimits {
    pub queue_events: usize,
    pub batch_events: usize,
}

impl Default for InputLimits {
    fn default() -> Self {
        Self {
            queue_events: 1024,
            batch_events: 127,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    InvalidLimits,
    Allocation,
    StaleSession,
    SequenceOrder,
    TimeOrder,
    InvalidEvent,
    QueueFull,
    OutputCapacity,
    InvalidCount,
    NotFocused,
    NotCaptured,
    Exhausted,
    InvalidWire,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    /// normalized physical indices; virtual-key and directinput mappings belong to frontends.
    Key {
        code: u16,
        pressed: bool,
    },
    Button {
        button: u8,
        pressed: bool,
    },
    Relative {
        dx: i32,
        dy: i32,
        wheel: i32,
    },
    Focus {
        focused: bool,
    },
    Capture {
        captured: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedInputEvent {
    pub timestamp_ns: u64,
    pub kind: InputKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputOrigin {
    Host,
    FocusRelease,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputRecord {
    pub batch_sequence: u64,
    pub event: TimedInputEvent,
    pub origin: InputOrigin,
}

impl Default for InputRecord {
    fn default() -> Self {
        Self {
            batch_sequence: 0,
            event: TimedInputEvent {
                timestamp_ns: 0,
                kind: InputKind::Focus { focused: false },
            },
            origin: InputOrigin::Host,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelativeTotals {
    pub dx: i64,
    pub dy: i64,
    pub wheel: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputState {
    pub keys: [bool; KEY_COUNT],
    pub buttons: [bool; BUTTON_COUNT],
    pub relative: RelativeTotals,
    pub focused: bool,
    pub captured: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            keys: [false; KEY_COUNT],
            buttons: [false; BUTTON_COUNT],
            relative: RelativeTotals::default(),
            focused: false,
            captured: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputLease {
    pub(super) session_epoch: u64,
    pub(super) focus_generation: u64,
    pub(super) capture_generation: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueRead {
    pub count: usize,
    pub bytes: usize,
}
