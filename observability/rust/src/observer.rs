use crate::{Config, ConfigError, Field, Level, Value, clock::Clock, perfetto};
use std::{
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, Mutex},
};

const END_RESERVE: usize = 128;
const FINISH_RESERVE: usize = 384;
const MAX_TRACKS: usize = 256;

#[derive(Clone, Default)]
pub struct Observer(Option<Arc<Mutex<State>>>);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub level: Level,
    pub finished: bool,
    pub buffered_bytes: usize,
    pub dropped_events: u64,
    pub emitted_events: u64,
    pub open_scopes: u64,
}

impl Stats {
    pub fn drained(&self) -> bool {
        self.finished && self.buffered_bytes == 0
    }
}

struct State {
    config: Config,
    clock: Clock,
    last_timestamp: u64,
    packets: VecDeque<Vec<u8>>,
    packet_offset: usize,
    buffered_bytes: usize,
    reserved_bytes: usize,
    dropped: u64,
    emitted: u64,
    finished: bool,
    open_scopes: u64,
    next_track: u32,
    threads: HashMap<std::thread::ThreadId, u64>,
    counters: HashMap<(String, String), u64>,
}

impl State {
    fn timestamp(&mut self) -> u64 {
        self.last_timestamp = self.last_timestamp.max(self.clock.now_ns());
        self.last_timestamp
    }
    fn accepts(&self, level: Level, context: &str) -> bool {
        !self.finished && self.config.accepts(level, context)
    }
    fn push(&mut self, packet: Vec<u8>, reserve: usize) -> bool {
        let needed = self
            .buffered_bytes
            .saturating_add(packet.len())
            .saturating_add(self.reserved_bytes)
            .saturating_add(reserve)
            .saturating_add(FINISH_RESERVE);
        if needed > self.config.max_buffer_bytes {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        self.force_push(packet);
        true
    }
    fn force_push(&mut self, packet: Vec<u8>) {
        self.buffered_bytes += packet.len();
        self.packets.push_back(packet);
    }
    fn thread_track(&mut self) -> Option<u64> {
        let thread = std::thread::current().id();
        if let Some(track) = self.threads.get(&thread) {
            return Some(*track);
        }
        let track = if self.threads.is_empty() {
            self.config.track_uuid
        } else {
            self.track("native thread", false)?
        };
        self.threads.insert(thread, track);
        Some(track)
    }
    fn track(&mut self, name: &str, counter: bool) -> Option<u64> {
        if self.next_track as usize >= MAX_TRACKS {
            self.dropped = self.dropped.saturating_add(1);
            return None;
        }
        let track = self.config.track_uuid + u64::from(self.next_track);
        let timestamp = self.timestamp();
        let packet = perfetto::descriptor(
            self.config.producer_sequence_id,
            timestamp,
            track,
            name,
            Some(self.config.track_uuid),
            counter,
        );
        if !self.push(packet, 0) {
            return None;
        }
        self.next_track += 1;
        Some(track)
    }
    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        track: u64,
        kind: u64,
        context: &str,
        name: &str,
        fields: &[Field<'_>],
        counter: Option<f64>,
        reserve: usize,
    ) -> bool {
        let timestamp = self.timestamp();
        let packet = perfetto::event(
            self.config.producer_sequence_id,
            timestamp,
            track,
            kind,
            context,
            name,
            fields,
            counter,
        );
        if !self.push(packet, reserve) {
            return false;
        }
        self.emitted = self.emitted.saturating_add(1);
        true
    }
    fn valid(&mut self, context: &str, name: &str, fields: &[Field<'_>]) -> bool {
        if context.len() <= 128
            && name.len() <= 256
            && fields.len() <= 32
            && fields.iter().all(|field| {
                field.name.len() <= 128
                    && match field.value {
                        Value::String(value) => value.len() <= 1024,
                        _ => true,
                    }
            })
        {
            true
        } else {
            self.dropped = self.dropped.saturating_add(1);
            false
        }
    }
}

impl Observer {
    pub const fn disabled() -> Self {
        Self(None)
    }
    pub fn new(config: Config) -> Result<Self, ConfigError> {
        config.validate()?;
        if config.level == Level::Off {
            return Ok(Self::disabled());
        }
        let clock = Clock::new();
        let timestamp = clock.now_ns();
        let descriptor = perfetto::descriptor(
            config.producer_sequence_id,
            timestamp,
            config.track_uuid,
            &config.track_name,
            None,
            false,
        );
        let mut state = State {
            config,
            clock,
            last_timestamp: timestamp,
            packets: VecDeque::new(),
            packet_offset: 0,
            buffered_bytes: 0,
            reserved_bytes: 0,
            dropped: 0,
            emitted: 0,
            finished: false,
            open_scopes: 0,
            next_track: 1,
            threads: HashMap::new(),
            counters: HashMap::new(),
        };
        state.force_push(perfetto::clock_snapshot(
            state.config.producer_sequence_id,
            timestamp,
        ));
        state.force_push(descriptor);
        let track = state.config.track_uuid;
        state.record(track, 3, "observability", "capture.started", &[], None, 0);
        Ok(Self(Some(Arc::new(Mutex::new(state)))))
    }
    fn with<T>(&self, default: T, operation: impl FnOnce(&mut State) -> T) -> T {
        self.0.as_ref().map_or(default, |state| {
            operation(
                &mut state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            )
        })
    }
    pub fn is_recording(&self) -> bool {
        self.with(false, |state| !state.finished)
    }
    pub fn enabled(&self, level: Level, context: &str) -> bool {
        self.with(false, |state| state.accepts(level, context))
    }
    pub fn stats(&self) -> Stats {
        self.with(Stats::default(), |state| Stats {
            level: state.config.level,
            finished: state.finished,
            buffered_bytes: state.buffered_bytes,
            dropped_events: state.dropped,
            emitted_events: state.emitted,
            open_scopes: state.open_scopes,
        })
    }
    pub fn instant(&self, level: Level, context: &str, name: &str, fields: &[Field<'_>]) {
        self.with((), |state| {
            if !state.accepts(level, context) || !state.valid(context, name, fields) {
                return;
            }
            if let Some(track) = state.thread_track() {
                state.record(track, 3, context, name, fields, None, 0);
            }
        });
    }
    pub fn counter(&self, level: Level, context: &str, name: &str, value: f64) {
        self.with((), |state| {
            if !state.accepts(level, context)
                || !state.valid(context, name, &[])
                || !value.is_finite()
            {
                return;
            }
            let key = (context.to_owned(), name.to_owned());
            let track = if let Some(track) = state.counters.get(&key) {
                *track
            } else {
                let Some(track) = state.track(&format!("{context}.{name}"), true) else {
                    return;
                };
                state.counters.insert(key, track);
                track
            };
            state.record(track, 4, "", "", &[], Some(value), 0);
        });
    }
    pub fn scope(&self, level: Level, context: &str, name: &str, fields: &[Field<'_>]) -> Scope {
        let track = self.with(None, |state| {
            if !state.accepts(level, context) || !state.valid(context, name, fields) {
                return None;
            }
            let track = state.thread_track()?;
            if !state.record(track, 1, context, name, fields, None, END_RESERVE) {
                return None;
            }
            state.reserved_bytes += END_RESERVE;
            state.open_scopes += 1;
            Some(track)
        });
        Scope {
            observer: self.clone(),
            track,
            thread_bound: PhantomData,
        }
    }
    pub fn drain(&self) -> Vec<u8> {
        self.drain_chunk(usize::MAX)
    }
    pub fn drain_chunk(&self, maximum: usize) -> Vec<u8> {
        self.with(Vec::new(), |state| {
            let mut output = Vec::with_capacity(state.buffered_bytes.min(maximum));
            while output.len() < maximum {
                let Some(packet) = state.packets.front() else {
                    break;
                };
                let length = (packet.len() - state.packet_offset).min(maximum - output.len());
                output
                    .extend_from_slice(&packet[state.packet_offset..state.packet_offset + length]);
                state.packet_offset += length;
                state.buffered_bytes -= length;
                if state.packet_offset == packet.len() {
                    state.packets.pop_front();
                    state.packet_offset = 0;
                }
            }
            output
        })
    }
    pub fn finish(&self) -> Vec<u8> {
        self.finalize();
        self.drain()
    }
    pub fn finalize(&self) {
        self.with((), |state| {
            if state.finished {
                return;
            }
            let fields = [
                Field::unsigned("dropped_events", state.dropped),
                Field::unsigned("open_scopes", state.open_scopes),
            ];
            let timestamp = state.timestamp();
            let packet = perfetto::event(
                state.config.producer_sequence_id,
                timestamp,
                state.config.track_uuid,
                3,
                "observability",
                "capture.finished",
                &fields,
                None,
            );
            state.force_push(packet);
            state.finished = true;
            state.reserved_bytes = 0;
            state.emitted = state.emitted.saturating_add(1);
        });
    }
}

pub struct Scope {
    observer: Observer,
    track: Option<u64>,
    thread_bound: PhantomData<Rc<()>>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        let Some(track) = self.track else {
            return;
        };
        self.observer.with((), |state| {
            if state.finished {
                return;
            }
            state.reserved_bytes -= END_RESERVE;
            state.open_scopes -= 1;
            let timestamp = state.timestamp();
            let packet = perfetto::event(
                state.config.producer_sequence_id,
                timestamp,
                track,
                2,
                "",
                "",
                &[],
                None,
            );
            debug_assert!(packet.len() <= END_RESERVE);
            state.force_push(packet);
            state.emitted = state.emitted.saturating_add(1);
        });
    }
}
