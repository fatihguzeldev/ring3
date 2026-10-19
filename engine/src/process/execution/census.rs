use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, WORK_CONSUMED_OFFSET},
        header,
        x86::EIP_OFFSET,
    },
    process::{CallError, EngineInstance, ExecutionReceipt32, HostError},
};

const MAX_CAPACITY: u32 = 32768;
const MAX_PROBES: usize = 64;
const EMPTY: u32 = u32::MAX;
const HEADER_SIZE: usize = 128;
const ROW_SIZE: usize = 80;
const PAGE_ROWS: usize = (4096 - HEADER_SIZE) / ROW_SIZE;
const SATURATED: u32 = 1;
const MISSING_METADATA: u32 = 2;
const UNRESOLVED: u32 = 4;
const PROBE_LIMIT: u32 = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Edge {
    owner: u64,
    entry: u32,
    exit: u32,
    reason: u32,
    coverage: u32,
}

impl Edge {
    fn hash(self) -> usize {
        let mut value = self.owner;
        for part in [self.entry, self.exit, self.reason, self.coverage] {
            value ^= u64::from(part);
            value = value.wrapping_mul(0x100_0000_01b3);
            value ^= value >> 32;
        }
        value as usize
    }
}

#[derive(Clone, Copy)]
struct Pending {
    token: u64,
    epoch: u64,
    owner: u64,
    entry: u32,
}

#[derive(Clone, Copy, Default)]
struct Metadata {
    blocks: u32,
    instructions: u32,
    wasm_bytes: u32,
    tier: u32,
    present: u32,
}

struct Row {
    edge: Edge,
    count: u64,
    retired: u64,
    work: u64,
    minimum: u32,
    maximum: u32,
    metadata: Metadata,
}

pub(in crate::process) struct ExecutionCensus32 {
    generation: u64,
    capacity: u32,
    collecting: bool,
    pending: Option<Pending>,
    rows: Vec<Row>,
    slots: Vec<u32>,
    entries: u64,
    finalized: u64,
    retired: u64,
    work: u64,
    dropped: u64,
    dropped_retired: u64,
    dropped_work: u64,
    unresolved: u64,
    zero_child: u64,
    flags: u32,
}

fn add(counter: &mut u64, amount: u64, flags: &mut u32) {
    if let Some(value) = counter.checked_add(amount) {
        *counter = value;
    } else {
        *counter = u64::MAX;
        *flags |= SATURATED;
    }
}

impl ExecutionCensus32 {
    fn new(generation: u64, capacity: u32) -> Result<Self, HostError> {
        let mut rows = Vec::new();
        rows.try_reserve_exact(capacity as usize)
            .map_err(|_| HostError::Infrastructure)?;
        let slot_count = (capacity as usize * 2).next_power_of_two();
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(slot_count)
            .map_err(|_| HostError::Infrastructure)?;
        slots.resize(slot_count, EMPTY);
        Ok(Self {
            generation,
            capacity,
            collecting: true,
            pending: None,
            rows,
            slots,
            entries: 0,
            finalized: 0,
            retired: 0,
            work: 0,
            dropped: 0,
            dropped_retired: 0,
            dropped_work: 0,
            unresolved: 0,
            zero_child: 0,
            flags: 0,
        })
    }

    fn abandon_pending(&mut self) {
        if self.pending.take().is_some() {
            add(&mut self.unresolved, 1, &mut self.flags);
            self.flags |= UNRESOLVED;
        }
    }

    fn enter(&mut self, pending: Pending) {
        self.abandon_pending();
        add(&mut self.entries, 1, &mut self.flags);
        self.pending = Some(pending);
    }

    fn finalize(
        &mut self,
        token: u64,
        epoch: u64,
        exit: u32,
        reason: u32,
        coverage: u32,
        retired: u32,
        work: u32,
        metadata: impl FnOnce(u64) -> Option<Metadata>,
    ) {
        let Some(pending) = self.pending else {
            self.flags |= UNRESOLVED;
            return;
        };
        if pending.token != token || pending.epoch != epoch {
            self.abandon_pending();
            return;
        }
        self.pending = None;
        add(&mut self.finalized, 1, &mut self.flags);
        add(&mut self.retired, u64::from(retired), &mut self.flags);
        add(&mut self.work, u64::from(work), &mut self.flags);
        let edge = Edge {
            owner: pending.owner,
            entry: pending.entry,
            exit,
            reason,
            coverage,
        };
        let mask = self.slots.len() - 1;
        let hash = edge.hash();
        for probe in 0..MAX_PROBES.min(self.slots.len()) {
            let slot = hash.wrapping_add(probe) & mask;
            let index = self.slots[slot];
            if index == EMPTY {
                if self.rows.len() == self.capacity as usize {
                    self.drop_work(retired, work);
                    return;
                }
                let metadata = metadata(edge.owner).unwrap_or_else(|| {
                    self.flags |= MISSING_METADATA;
                    Metadata::default()
                });
                let index = self.rows.len() as u32;
                self.rows.push(Row {
                    edge,
                    count: 1,
                    retired: u64::from(retired),
                    work: u64::from(work),
                    minimum: retired,
                    maximum: retired,
                    metadata,
                });
                self.slots[slot] = index;
                return;
            }
            let row = &mut self.rows[index as usize];
            if row.edge == edge {
                add(&mut row.count, 1, &mut self.flags);
                add(&mut row.retired, u64::from(retired), &mut self.flags);
                add(&mut row.work, u64::from(work), &mut self.flags);
                row.minimum = row.minimum.min(retired);
                row.maximum = row.maximum.max(retired);
                return;
            }
        }
        self.flags |= PROBE_LIMIT;
        self.drop_work(retired, work);
    }

    fn drop_work(&mut self, retired: u32, work: u32) {
        add(&mut self.dropped, 1, &mut self.flags);
        add(
            &mut self.dropped_retired,
            u64::from(retired),
            &mut self.flags,
        );
        add(&mut self.dropped_work, u64::from(work), &mut self.flags);
    }

    fn page(&self, offset: u32) -> Result<[u8; 4096], HostError> {
        if self.collecting {
            return Err(HostError::Call(CallError::Busy));
        }
        if offset as usize > self.rows.len() {
            return Err(HostError::InvalidRequest);
        }
        let mut output = [0; 4096];
        header::write(&mut output, *b"R3CE");
        put64(&mut output, 16, self.generation);
        let length = (self.rows.len() - offset as usize).min(PAGE_ROWS);
        for (at, value) in [
            (24, 2),
            (28, self.capacity),
            (32, self.rows.len() as u32),
            (36, offset),
            (40, length as u32),
            (44, ROW_SIZE as u32),
            (120, self.flags),
        ] {
            header::write_u32(&mut output, at, value);
        }
        for (index, value) in [
            self.entries,
            self.finalized,
            self.retired,
            self.work,
            self.dropped,
            self.dropped_retired,
            self.dropped_work,
            self.unresolved,
            self.zero_child,
        ]
        .into_iter()
        .enumerate()
        {
            put64(&mut output, 48 + index * 8, value);
        }
        for (index, row) in self.rows[offset as usize..][..length].iter().enumerate() {
            let output = &mut output[HEADER_SIZE + index * ROW_SIZE..][..ROW_SIZE];
            put64(output, 0, row.edge.owner);
            for (at, value) in [
                (8, row.edge.entry),
                (12, row.edge.exit),
                (16, row.edge.reason),
                (20, row.edge.coverage),
                (48, row.minimum),
                (52, row.maximum),
                (56, row.metadata.blocks),
                (60, row.metadata.instructions),
                (64, row.metadata.wasm_bytes),
                (68, row.metadata.tier),
                (72, row.metadata.present),
            ] {
                header::write_u32(output, at, value);
            }
            put64(output, 24, row.count);
            put64(output, 32, row.retired);
            put64(output, 40, row.work);
        }
        Ok(output)
    }
}

fn put64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

impl EngineInstance {
    fn guard_execution_census(&self, key: u64) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        if self.execution_frame.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(())
    }

    pub fn begin_execution_census(&mut self, key: u64, capacity: u32) -> Result<(), HostError> {
        self.guard_execution_census(key)?;
        if self
            .execution_census
            .as_ref()
            .is_some_and(|census| census.collecting)
        {
            return Err(HostError::Call(CallError::Busy));
        }
        if !(1..=MAX_CAPACITY).contains(&capacity) {
            return Err(HostError::InvalidRequest);
        }
        let generation = self
            .execution_census_generation
            .checked_add(1)
            .ok_or(HostError::GenerationExhausted)?;
        let census = ExecutionCensus32::new(generation, capacity)?;
        self.execution_census = Some(census);
        self.execution_census_generation = generation;
        Ok(())
    }

    pub fn stop_execution_census(&mut self, key: u64) -> Result<(), HostError> {
        self.guard_execution_census(key)?;
        let census = self
            .execution_census
            .as_mut()
            .ok_or(HostError::InvalidRequest)?;
        census.abandon_pending();
        census.collecting = false;
        Ok(())
    }

    pub fn execution_census_page(&self, key: u64, offset: u32) -> Result<[u8; 4096], HostError> {
        self.guard_execution_census(key)?;
        self.execution_census
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .page(offset)
    }

    #[inline]
    pub(in crate::process) fn entered_execution_census_child(&mut self, epoch: u64) {
        let Some(census) = self
            .execution_census
            .as_mut()
            .filter(|census| census.collecting)
        else {
            return;
        };
        let Some(frame) = self.execution_frame else {
            return;
        };
        let Some((admitted_epoch, installed)) = frame.last else {
            return;
        };
        if admitted_epoch != epoch {
            return;
        }
        let entry = header::read_u32(&self.arena, STATE_OFFSET + EIP_OFFSET);
        census.enter(Pending {
            token: frame.token,
            epoch,
            owner: installed.unit_id,
            entry,
        });
    }

    #[inline]
    pub(in crate::process) fn sealed_execution_census_child(&mut self, epoch: u64) {
        if self
            .execution_census
            .as_ref()
            .is_none_or(|census| !census.collecting)
        {
            return;
        }
        let Some(frame) = self.execution_frame else {
            return;
        };
        let reason = header::read_u32(self.arena(), EXIT_OFFSET + 16);
        let retired = header::read_u32(self.arena(), EXIT_OFFSET + 20);
        let work = header::read_u32(self.arena(), WORK_CONSUMED_OFFSET).max(retired);
        let exit = header::read_u32(self.arena(), STATE_OFFSET + EIP_OFFSET);
        self.finalize_execution_census(
            frame.token,
            epoch,
            exit,
            reason,
            if reason == 3 { 1 } else { 2 },
            retired,
            work,
        );
    }

    #[inline]
    pub(in crate::process) fn finished_execution_census_receipt(
        &mut self,
        token: u64,
        receipt: ExecutionReceipt32,
    ) {
        let Some(census) = self
            .execution_census
            .as_mut()
            .filter(|census| census.collecting)
        else {
            return;
        };
        if receipt.kind == 0 {
            census.abandon_pending();
            add(&mut census.zero_child, 1, &mut census.flags);
        } else if receipt.kind == 1 {
            let retired = receipt.child_retired;
            let work = header::read_u32(&self.arena, WORK_CONSUMED_OFFSET).max(retired);
            let exit = header::read_u32(&self.arena, STATE_OFFSET + EIP_OFFSET);
            self.finalize_execution_census(
                token,
                receipt.epoch,
                exit,
                receipt.reason,
                3,
                retired,
                work,
            );
        } else {
            census.abandon_pending();
        }
    }

    fn finalize_execution_census(
        &mut self,
        token: u64,
        epoch: u64,
        exit: u32,
        reason: u32,
        coverage: u32,
        retired: u32,
        work: u32,
    ) {
        let memory = self.memory.as_ref();
        let resident = self.resident.as_ref();
        self.execution_census.as_mut().unwrap().finalize(
            token,
            epoch,
            exit,
            reason,
            coverage,
            retired,
            work,
            |owner| {
                let region = resident?.get_retained_raw(memory?, owner).ok()?;
                let metadata = region.metadata();
                Some(Metadata {
                    blocks: u32::try_from(metadata.blocks).ok()?,
                    instructions: u32::try_from(metadata.instructions).ok()?,
                    wasm_bytes: u32::try_from(region.retained_wasm_byte_len()).ok()?,
                    tier: region.tier() as u32,
                    present: 1,
                })
            },
        );
    }
}
