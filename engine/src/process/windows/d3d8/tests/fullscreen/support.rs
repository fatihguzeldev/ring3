use super::super::support::runtime;
use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason, x86::State32},
    graphics::d3d8::{
        api::{AdapterProfile32, DisplayMode32, InterfaceKind, MethodGate},
        resources::Format,
    },
    memory::GuestAddress,
    process::{CallError, EngineInstance, ExecutionConfig32, HostError},
    windows::{
        CallFrame32, CallingConvention32,
        user32::{User32Api32, User32Limits32},
    },
};

pub(super) const KEY: u64 = 91;
pub(super) const DATA: u32 = 0x2000;
pub(super) const PP: u32 = DATA + 128;
pub(super) const OUT: u32 = DATA + 192;
const PROC: u32 = 0x9010;
const RETURN: u32 = 0x9020;
const DEFAULT: u32 = 0x9030;
const RETURN_ID: u32 = 0x200ff;
const STACK: u32 = 0x8f00;
const RESUME: u32 = 0x9f00;

pub(super) struct Fixture {
    pub engine: EngineInstance,
    pub callback: u64,
    pub parent: GuestAddress,
    pub window: u32,
    atom: u32,
}

pub(super) fn words(engine: &EngineInstance, address: u32, count: usize) -> Vec<u32> {
    let mut bytes = vec![0; count * 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect()
}

pub(super) fn write(engine: &mut EngineInstance, address: u32, words: &[u32]) {
    let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), &bytes)
        .unwrap();
}

pub(super) fn state(engine: &EngineInstance) -> State32 {
    decode_state(&engine.arena()[..STATE_SIZE]).unwrap()
}

fn stop(engine: &mut EngineInstance, pc: u32, id: u32, esp: u32, result: u32) {
    let mut state = State32 {
        eip: pc,
        ..Default::default()
    };
    state.registers[0] = result;
    state.registers[4] = esp;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}

fn unit(engine: &mut EngineInstance, words: &[u32], blocks: u32, gates: u32) -> u64 {
    for (index, word) in words.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine
        .compile_resident_with_gates(blocks, gates)
        .unwrap()
        .get()
}

impl Fixture {
    pub fn new(style: u32) -> Self {
        let (mut runtime, memory, _) = runtime();
        let modes: Vec<_> = [(640, 480), (800, 600)]
            .into_iter()
            .map(|(width, height)| DisplayMode32 {
                width,
                height,
                refresh_rate: 60,
                format: Format::X8R8G8B8,
            })
            .collect();
        runtime.config.adapter = AdapterProfile32::new(modes[0], modes).unwrap();
        let parent = runtime.records[0].as_ref().unwrap().pointer;
        let mut engine = EngineInstance::new(256, KEY).unwrap();
        engine.memory = Some(memory);
        engine.d3d8 = Some(runtime);
        engine
            .configure_execution(ExecutionConfig32 {
                units: 32,
                ..Default::default()
            })
            .unwrap();
        engine.configure_cpu_clock(1_000_000, 0).unwrap();
        engine.configure_media(Default::default()).unwrap();
        engine
            .configure_user32(
                User32Limits32 {
                    classes: 8,
                    windows: 8,
                    messages: 64,
                    continuations: 16,
                },
                crate::memory::PageRange::new(GuestAddress(0x3000_0000), 16).unwrap(),
            )
            .unwrap();
        engine.map(0x8000, 1, 3).unwrap();
        engine.map(0x9000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x9000), &[0x0f, 0x0b, 0x90, 0x90].repeat(1024))
            .unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(PROC), &[0x90])
            .unwrap();
        let callback = unit(
            &mut engine,
            &[
                PROC,
                1,
                RETURN,
                2,
                DEFAULT,
                2,
                RETURN,
                RETURN_ID,
                DEFAULT,
                User32Api32::DefWindowProcA.id(),
            ],
            3,
            2,
        );
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(DATA + 384), b"D3DFullscreen\0")
            .unwrap();
        write(
            &mut engine,
            DATA + 256,
            &[0, PROC, 0, 0, 0x400000, 0, 0, 0, 0, DATA + 384],
        );
        let mut fixture = Self {
            engine,
            callback,
            parent,
            window: 0,
            atom: 0,
        };
        fixture.call(User32Api32::RegisterClassA.id(), &[DATA + 256]);
        fixture.atom = state(&fixture.engine).registers[0];
        assert!(fixture.atom >= 0xc000);
        fixture.window = fixture.create_window(style, 0, 0, 0, 0);
        fixture
    }

    pub fn create_window(&mut self, style: u32, x: u32, y: u32, width: u32, height: u32) -> u32 {
        self.call(
            User32Api32::CreateWindowExA.id(),
            &[
                0, self.atom, 0, style, x, y, width, height, 0, 0, 0x400000, 0,
            ],
        );
        self.drain(|_, _| {});
        let window = state(&self.engine).registers[0];
        assert_ne!(window, 0);
        window
    }

    pub fn parameters(&mut self, window: u32, width: u32, height: u32, windowed: bool) {
        write(
            &mut self.engine,
            PP,
            &[
                width,
                height,
                22,
                1,
                0,
                1,
                window,
                windowed as u32,
                0,
                0,
                0,
                60,
                1,
            ],
        );
    }

    pub fn foreign_window(&mut self) -> u32 {
        use crate::process::windows::user32::{User32CallContext32, User32Outcome32};
        let arguments = [
            0,
            self.atom,
            0,
            0x8000_0000,
            31,
            47,
            80,
            60,
            0,
            0,
            0x400000,
            0,
        ];
        write(
            &mut self.engine,
            STACK,
            &std::iter::once(RESUME).chain(arguments).collect::<Vec<_>>(),
        );
        let mut state = State32::default();
        state.registers[4] = STACK;
        let frame = CallFrame32::capture(
            self.engine.memory().unwrap(),
            state,
            CallingConvention32::Stdcall,
            12,
        )
        .unwrap();
        let mut context = self.engine.user32.take().unwrap();
        let memory = self.engine.memory.as_mut().unwrap();
        let mut outcome = context
            .prepare(
                User32Api32::CreateWindowExA,
                &frame,
                memory,
                User32CallContext32 {
                    thread_id: 2,
                    instance: 0x400000,
                    ..Default::default()
                },
            )
            .unwrap();
        let window = loop {
            match outcome {
                User32Outcome32::Return(window) => break window,
                User32Outcome32::Callback(callback) => {
                    outcome = context
                        .complete(
                            callback.continuation,
                            u32::from(callback.arguments[1] == 0x81),
                            memory,
                        )
                        .unwrap();
                }
                User32Outcome32::Wait { .. } => panic!("foreign fixture creation cannot wait"),
            }
        };
        self.engine.user32 = Some(context);
        assert_ne!(window, 0);
        window
    }

    pub fn create_device(&mut self, focus: u32) -> (u64, u32) {
        self.call(
            MethodGate {
                interface: InterfaceKind::Direct3D,
                slot: 15,
            }
            .id(),
            &[self.parent.0, 0, 1, focus, 0x80, PP, OUT],
        )
    }

    pub fn reset(&mut self, device: u32) -> (u64, u32) {
        self.call(
            MethodGate {
                interface: InterfaceKind::Device,
                slot: 14,
            }
            .id(),
            &[device, PP],
        )
    }

    pub fn call(&mut self, id: u32, args: &[u32]) -> (u64, u32) {
        let (owner, token) = self.capture(id, args);
        self.engine
            .complete_resident_windows_call(KEY, owner, token)
            .unwrap();
        (owner, token)
    }

    pub fn capture(&mut self, id: u32, args: &[u32]) -> (u64, u32) {
        let pc = 0x9100
            + self
                .engine
                .resident
                .as_ref()
                .map_or(0, |registry| registry.usage().units as u32)
                * 4;
        let owner = unit(&mut self.engine, &[pc, 2, pc, id], 1, 1);
        let frame: Vec<_> = std::iter::once(RESUME)
            .chain(args.iter().copied())
            .collect();
        write(&mut self.engine, STACK, &frame);
        stop(&mut self.engine, pc, id, STACK, 0xfeedface);
        let token = self
            .engine
            .capture_resident_call(KEY, owner, CallingConvention32::Stdcall, args.len() as u32)
            .unwrap()
            .token;
        (owner, token)
    }

    pub fn drain(&mut self, mut edit: impl FnMut(&mut EngineInstance, [u32; 4])) -> Vec<u32> {
        let mut messages = Vec::new();
        while let Some(request) = self.engine.pending_windows_callback() {
            let arguments: [u32; 4] = request.arguments[..4].try_into().unwrap();
            messages.push(arguments[1]);
            edit(&mut self.engine, arguments);
            self.finish(arguments);
        }
        messages
    }

    fn finish(&mut self, arguments: [u32; 4]) {
        let return_esp = self
            .engine
            .pending_call
            .as_ref()
            .unwrap()
            .frame
            .state()
            .registers[4];
        let record = self
            .engine
            .begin_windows_resident_callback(KEY, self.callback, RETURN, RETURN_ID)
            .unwrap();
        self.engine
            .authorize_resident_callback(KEY, self.callback, record.token)
            .unwrap();
        let result = if arguments[1] == 0x83 {
            let frame: Vec<_> = std::iter::once(PROC).chain(arguments).collect();
            write(&mut self.engine, STACK, &frame);
            stop(
                &mut self.engine,
                DEFAULT,
                User32Api32::DefWindowProcA.id(),
                STACK,
                0,
            );
            let inner = self
                .engine
                .capture_resident_callback_call(
                    KEY,
                    self.callback,
                    record.token,
                    CallingConvention32::Stdcall,
                    4,
                )
                .unwrap()
                .token;
            self.engine
                .complete_active_resident_callback_windows_call(
                    KEY,
                    self.callback,
                    record.token,
                    inner,
                )
                .unwrap();
            state(&self.engine).registers[0]
        } else {
            u32::from(arguments[1] == 0x81)
        };
        stop(&mut self.engine, RETURN, RETURN_ID, return_esp, result);
        self.engine
            .finish_windows_resident_callback(KEY, self.callback, record.token)
            .unwrap();
    }

    pub fn assert_pending_retry(&mut self, owner: u64, token: u32) {
        let before = self.engine.arena().to_vec();
        assert_eq!(
            self.engine.complete_resident_call(KEY, owner, token, 0),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(self.engine.arena(), before);
        self.engine
            .complete_resident_windows_call(KEY, owner, token)
            .unwrap();
        assert_eq!(self.engine.arena(), before);
    }

    pub fn assert_return(&self, stack_words: u32) {
        let state = state(&self.engine);
        assert_eq!(
            (state.eip, state.registers[0], state.registers[4]),
            (RESUME, 0, STACK + 4 + stack_words * 4)
        );
        assert!(self.engine.pending_call.is_none());
        assert!(self.engine.windows_replies.is_empty());
    }

    pub fn drain_batches(&mut self, device: u32) {
        let epoch = self
            .engine
            .d3d8
            .as_ref()
            .unwrap()
            .device(GuestAddress(device))
            .unwrap()
            .queue()
            .epoch();
        while let Some(id) = self.engine.d3d8_seal_batch(epoch).unwrap() {
            self.engine.d3d8_acknowledge_batch(id).unwrap();
        }
    }
}
