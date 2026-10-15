use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::{encode_segment_context, encode_state},
    },
    cpu::x86::{SegmentCache, SegmentRegister, StackFrameKind, State32},
    memory::GuestAddress,
    process::{EngineInstance, StoreCompletion},
};
use std::collections::BTreeMap;

pub const REGISTERS: [u32; 8] = [
    0x1111_2222,
    0x2222_3333,
    0x3333_4444,
    0x4444_5555,
    0x8080,
    0x8080,
    0x6666_7777,
    0x7777_8888,
];
const PAGES: [u32; 10] = [
    0,
    0x1000,
    0x5000,
    0x6000,
    0x7000,
    0x8000,
    0x9000,
    0xf000,
    0x10000,
    0xffff_f000,
];
pub type Ram = BTreeMap<u32, u8>;

pub struct Context {
    pub engine: EngineInstance,
    resident: Option<u64>,
}

impl Context {
    pub fn new(resident: bool, entries: bool) -> Self {
        let mut engine = EngineInstance::new(12, 0x5353).unwrap();
        for address in PAGES {
            engine
                .map(address, 1, if address == 0x1000 { 7 } else { 3 })
                .unwrap();
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4096].fill(0xa5);
            engine.upload(address, 4096).unwrap();
        }
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0xeb, 0xfe]);
        engine.upload(0x1000, 2).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
            .copy_from_slice(&0x1000_u32.to_le_bytes());
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
            .copy_from_slice(&2_u32.to_le_bytes());
        let resident = match (resident, entries) {
            (false, false) => {
                engine.compile(1).unwrap();
                None
            }
            (false, true) => {
                engine.compile_entries(1, 0).unwrap();
                None
            }
            (true, false) => Some(engine.compile_resident(1).unwrap().get()),
            (true, true) => Some(engine.compile_resident_entries(1, 0).unwrap().get()),
        };
        Self { engine, resident }
    }

    pub fn seed(&mut self, registers: [u32; 8], cache: Option<u32>) {
        let mut context = self.engine.segment_context().unwrap();
        if let Some(bits) = cache {
            context.caches[SegmentRegister::Ss.index()] = SegmentCache::from_bits(bits).unwrap();
        }
        encode_segment_context(
            &context,
            &mut self.engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + 64],
        )
        .unwrap();
        let state = State32 {
            registers,
            eip: 0x1000,
            eflags: 0xcd7,
        };
        let arena = self.engine.arena_mut().unwrap();
        encode_state(&state, &mut arena[..56]).unwrap();
        arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 64].fill(0xe7);
        arena[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet([0, 0xabc, 0, 0, 0, 0]));
        for (index, value) in registers.into_iter().enumerate() {
            arena[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
    }

    pub fn ram(&self) -> Ram {
        let mut ram = Ram::new();
        for address in PAGES {
            let mut bytes = [0; 4096];
            self.engine
                .memory()
                .unwrap()
                .read(GuestAddress(address), &mut bytes)
                .unwrap();
            for (index, value) in bytes.into_iter().enumerate() {
                ram.insert(address + index as u32, value);
            }
        }
        ram
    }

    fn execute(&mut self, kind: StackFrameKind, allocation: u16, nesting: u8) -> StoreCompletion {
        let control = kind.control(allocation, nesting);
        match self.resident {
            Some(id) => self
                .engine
                .stack_execute_resident(self.engine.key(), id, control)
                .unwrap(),
            None => self.engine.stack_execute(control).unwrap(),
        }
    }

    pub fn failure(
        &mut self,
        kind: StackFrameKind,
        allocation: u16,
        nesting: u8,
        fields: [u32; 6],
        denied: Option<(u32, u32)>,
    ) {
        let before_ram = self.ram();
        if let Some((page, permissions)) = denied {
            self.engine.protect(page, 1, permissions).unwrap();
        }
        let before = self.engine.arena().to_vec();
        assert_eq!(
            self.execute(kind, allocation, nesting),
            StoreCompletion::Complete
        );
        let mut expected = before;
        expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet(fields));
        assert_eq!(
            self.engine.arena(),
            expected,
            "only the exact failure packet may publish"
        );
        if let Some((page, _)) = denied {
            self.engine.protect(page, 1, 3).unwrap();
        }
        assert_eq!(
            self.ram(),
            before_ram,
            "failure must preserve every mapped RAM byte"
        );
    }

    pub fn success(
        &mut self,
        kind: StackFrameKind,
        allocation: u16,
        nesting: u8,
        registers: [u32; 8],
        expected_ram: &Ram,
    ) {
        let mut expected = self.engine.arena().to_vec();
        expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet([0; 6]));
        for (index, value) in registers.into_iter().enumerate() {
            expected[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            self.execute(kind, allocation, nesting),
            StoreCompletion::Complete
        );
        assert_eq!(
            self.engine.arena(),
            expected,
            "helper changes only packet and staged GPR result"
        );
        assert_eq!(&self.ram(), expected_ram, "exact writes and untouched RAM");
    }
}

fn packet(fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..4].copy_from_slice(b"R3MH");
    bytes[4..8].copy_from_slice(&0x0001_0007_u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&40_u32.to_le_bytes());
    for (index, value) in fields.into_iter().enumerate() {
        bytes[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
