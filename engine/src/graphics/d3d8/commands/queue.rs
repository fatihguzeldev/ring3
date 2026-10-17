use super::{Command, CommandError, CommandLimits, CommandPlan, Progress};
use crate::{
    abi::graphics::d3d8::{
        self as codec, BATCH_HEADER_SIZE, BatchHeader, CodecError, FIRST_FRAGMENT,
        FRAGMENT_HEADER_SIZE, Fragment, LAST_FRAGMENT,
    },
    graphics::BatchId,
};

struct PreparedCommand {
    command: u64,
    opcode: u16,
    range: std::ops::Range<usize>,
}

struct Pending {
    commands: Vec<PreparedCommand>,
    index: usize,
    offset: usize,
}

pub struct PreparedGroup {
    epoch: u64,
    sequence: u64,
    previous_command: u64,
    packet_length: usize,
    commands: Vec<PreparedCommand>,
    bytes: Vec<u8>,
    record_bytes: usize,
}

pub struct CommandQueue {
    epoch: u64,
    limits: CommandLimits,
    bytes: Vec<u8>,
    fragments: u32,
    first_command: u64,
    last_command: u64,
    sequence: u64,
    command_sequence: u64,
    sealed: Option<BatchId>,
    pending: Option<Pending>,
    staging: Vec<u8>,
}

impl CommandQueue {
    pub fn new(epoch: u64, limits: CommandLimits) -> Result<Self, CommandError> {
        if epoch == 0 {
            return Err(CommandError::InvalidEpoch);
        }
        limits.validate()?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(limits.batch_bytes as usize)
            .map_err(|_| CommandError::Allocation)?;
        bytes.resize(BATCH_HEADER_SIZE, 0);
        Ok(Self {
            epoch,
            limits,
            bytes,
            fragments: 0,
            first_command: 0,
            last_command: 0,
            sequence: 0,
            command_sequence: 0,
            sealed: None,
            pending: None,
            staging: Vec::new(),
        })
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn limits(&self) -> CommandLimits {
        self.limits
    }
    pub fn is_draining(&self) -> bool {
        self.pending.is_some()
    }
    pub fn draining_command(&self) -> Option<u64> {
        let pending = self.pending.as_ref()?;
        let index = pending.index.min(pending.commands.len().saturating_sub(1));
        pending.commands.get(index).map(|command| command.command)
    }
    pub fn is_sealed(&self) -> bool {
        self.sealed.is_some()
    }

    pub fn preflight(&self, command: &Command<'_>) -> Result<CommandPlan, CommandError> {
        if self.sealed.is_some() || self.pending.is_some() {
            return Err(CommandError::Backpressure);
        }
        self.command_sequence
            .checked_add(1)
            .ok_or(CommandError::IdentityExhausted)?;
        let (opcode, payload_bytes) = codec::payload_size(command).map_err(error)?;
        if payload_bytes > self.limits.pending_bytes as usize {
            return Err(CommandError::Capacity);
        }
        let fragments = if payload_bytes + FRAGMENT_HEADER_SIZE <= self.remaining() {
            0
        } else {
            let first = self.remaining().saturating_sub(FRAGMENT_HEADER_SIZE);
            let capacity =
                self.limits.batch_bytes as usize - BATCH_HEADER_SIZE - FRAGMENT_HEADER_SIZE;
            1 + payload_bytes.saturating_sub(first).div_ceil(capacity)
        };
        self.sequence
            .checked_add(fragments.max(1) as u64)
            .ok_or(CommandError::IdentityExhausted)?;
        Ok(CommandPlan {
            payload_bytes,
            opcode,
            draining: fragments != 0,
        })
    }

    pub fn submit(&mut self, command: &Command<'_>) -> Result<Progress, CommandError> {
        let plan = self.preflight(command)?;
        let identity = self.command_sequence + 1;
        if plan.payload_bytes + FRAGMENT_HEADER_SIZE <= self.remaining() {
            codec::append_command(command, identity, &mut self.bytes).map_err(error)?;
            self.record(identity);
            self.command_sequence = identity;
            return Ok(Progress::Queued);
        }
        let mut commands = Vec::new();
        commands
            .try_reserve_exact(1)
            .map_err(|_| CommandError::Allocation)?;
        commands.push(PreparedCommand {
            command: identity,
            opcode: plan.opcode,
            range: 0..plan.payload_bytes,
        });
        self.staging.clear();
        self.staging
            .try_reserve(plan.payload_bytes)
            .map_err(|_| CommandError::Allocation)?;
        codec::append_payload(command, &mut self.staging).map_err(error)?;
        self.pending = Some(Pending {
            commands,
            index: 0,
            offset: 0,
        });
        self.command_sequence = identity;
        self.pump()?;
        Ok(Progress::Draining)
    }

    pub fn prepare(&self, commands: &[Command<'_>]) -> Result<PreparedGroup, CommandError> {
        if self.sealed.is_some() || self.pending.is_some() {
            return Err(CommandError::Backpressure);
        }
        if commands.is_empty() {
            return Err(CommandError::InvalidCommand);
        }
        if commands.len() > self.limits.pending_bytes as usize / FRAGMENT_HEADER_SIZE {
            return Err(CommandError::Capacity);
        }
        let mut items = Vec::new();
        items
            .try_reserve_exact(commands.len())
            .map_err(|_| CommandError::Allocation)?;
        let mut length = 0usize;
        let mut record_bytes = 0usize;
        for (index, command) in commands.iter().enumerate() {
            let (opcode, payload_bytes) = codec::payload_size(command).map_err(error)?;
            let end = length
                .checked_add(payload_bytes)
                .filter(|end| *end <= self.limits.pending_bytes as usize)
                .ok_or(CommandError::Capacity)?;
            let identity = self
                .command_sequence
                .checked_add(index as u64 + 1)
                .ok_or(CommandError::IdentityExhausted)?;
            items.push(PreparedCommand {
                command: identity,
                opcode,
                range: length..end,
            });
            length = end;
            record_bytes = record_bytes
                .checked_add(payload_bytes + FRAGMENT_HEADER_SIZE)
                .filter(|length| *length <= self.limits.pending_bytes as usize)
                .ok_or(CommandError::Capacity)?;
        }
        let capacity = self.limits.batch_bytes as usize - BATCH_HEADER_SIZE - FRAGMENT_HEADER_SIZE;
        let maximum_packets = items
            .iter()
            .try_fold(1u64, |count, command| {
                count.checked_add(command.range.len().div_ceil(capacity).max(1) as u64)
            })
            .ok_or(CommandError::IdentityExhausted)?;
        self.sequence
            .checked_add(maximum_packets)
            .ok_or(CommandError::IdentityExhausted)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| CommandError::Allocation)?;
        for command in commands {
            codec::append_payload(command, &mut bytes).map_err(error)?;
        }
        Ok(PreparedGroup {
            epoch: self.epoch,
            sequence: self.sequence,
            previous_command: self.command_sequence,
            packet_length: self.bytes.len(),
            commands: items,
            bytes,
            record_bytes,
        })
    }

    pub fn can_commit(&self, prepared: &PreparedGroup) -> Result<(), CommandError> {
        if prepared.epoch != self.epoch
            || prepared.sequence != self.sequence
            || prepared.previous_command != self.command_sequence
            || prepared.packet_length != self.bytes.len()
            || self.sealed.is_some()
            || self.pending.is_some()
        {
            return Err(CommandError::StalePreparation);
        }
        Ok(())
    }

    pub fn commit(&mut self, prepared: PreparedGroup) -> Result<Progress, CommandError> {
        self.can_commit(&prepared)?;
        let last_command = prepared.commands.last().unwrap().command;
        if prepared.record_bytes <= self.remaining() {
            for command in &prepared.commands {
                codec::append_fragment(
                    &mut self.bytes,
                    Fragment {
                        opcode: command.opcode,
                        flags: FIRST_FRAGMENT | LAST_FRAGMENT,
                        command: command.command,
                        offset: 0,
                        total: command.range.len() as u32,
                        bytes: &prepared.bytes[command.range.clone()],
                    },
                )
                .map_err(error)?;
                self.record(command.command);
            }
            self.command_sequence = last_command;
            return Ok(Progress::Queued);
        }
        self.staging = prepared.bytes;
        self.pending = Some(Pending {
            commands: prepared.commands,
            index: 0,
            offset: 0,
        });
        self.command_sequence = last_command;
        self.pump()?;
        Ok(Progress::Draining)
    }

    pub fn seal(&mut self) -> Result<Option<BatchId>, CommandError> {
        if let Some(id) = self.sealed {
            return Ok(Some(id));
        }
        if self.fragments == 0 {
            return Ok(None);
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(CommandError::IdentityExhausted)?;
        let id = BatchId {
            epoch: self.epoch,
            sequence,
        };
        let header = BatchHeader {
            length: self.bytes.len() as u32,
            fragments: self.fragments,
            epoch: id.epoch,
            sequence: id.sequence,
            first_command: self.first_command,
            last_command: self.last_command,
        };
        codec::write_batch_header(&mut self.bytes, header).map_err(error)?;
        self.sequence = sequence;
        self.sealed = Some(id);
        Ok(Some(id))
    }

    pub fn peek(&self) -> Option<(BatchId, &[u8])> {
        self.sealed.map(|id| (id, self.bytes.as_slice()))
    }

    pub fn copy(&self, output: &mut [u8]) -> Result<usize, CommandError> {
        let (_, bytes) = self.peek().ok_or(CommandError::EmptyBatch)?;
        if output.len() < bytes.len() {
            return Err(CommandError::OutputTooSmall);
        }
        output[..bytes.len()].copy_from_slice(bytes);
        Ok(bytes.len())
    }

    pub fn copy_range(&self, offset: u32, output: &mut [u8]) -> Result<usize, CommandError> {
        let (_, bytes) = self.peek().ok_or(CommandError::EmptyBatch)?;
        let end = (offset as usize)
            .checked_add(output.len())
            .filter(|end| *end <= bytes.len())
            .ok_or(CommandError::InvalidRange)?;
        output.copy_from_slice(&bytes[offset as usize..end]);
        Ok(output.len())
    }

    pub fn acknowledge(&mut self, id: BatchId) -> Result<Progress, CommandError> {
        if self.sealed != Some(id) {
            return Err(CommandError::StaleBatch);
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.index < pending.commands.len())
        {
            self.sequence
                .checked_add(1)
                .ok_or(CommandError::IdentityExhausted)?;
        }
        self.clear_packet();
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.index == pending.commands.len())
        {
            self.pending = None;
            self.staging.clear();
        } else if self.pending.is_some() {
            self.pump()?;
        }
        Ok(if self.pending.is_some() {
            Progress::Draining
        } else {
            Progress::Queued
        })
    }

    pub fn reset(&mut self, epoch: u64) -> Result<(), CommandError> {
        if epoch == 0 || epoch <= self.epoch {
            return Err(CommandError::InvalidEpoch);
        }
        self.clear_packet();
        self.pending = None;
        self.staging.clear();
        self.sequence = 0;
        self.command_sequence = 0;
        self.epoch = epoch;
        Ok(())
    }

    fn remaining(&self) -> usize {
        self.limits.batch_bytes as usize - self.bytes.len()
    }

    fn record(&mut self, command: u64) {
        if self.fragments == 0 {
            self.first_command = command;
        }
        self.last_command = command;
        self.fragments += 1;
    }

    fn pump(&mut self) -> Result<(), CommandError> {
        while self.remaining() > FRAGMENT_HEADER_SIZE
            && self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.index < pending.commands.len())
        {
            let pending = self.pending.as_ref().unwrap();
            let command = &pending.commands[pending.index];
            let available = self.remaining() - FRAGMENT_HEADER_SIZE;
            let end = pending
                .offset
                .saturating_add(available)
                .min(command.range.len());
            let flags = (if pending.offset == 0 {
                FIRST_FRAGMENT
            } else {
                0
            }) | (if end == command.range.len() {
                LAST_FRAGMENT
            } else {
                0
            });
            let fragment = Fragment {
                opcode: command.opcode,
                flags,
                command: command.command,
                offset: pending.offset as u32,
                total: command.range.len() as u32,
                bytes: &self.staging
                    [command.range.start + pending.offset..command.range.start + end],
            };
            let identity = command.command;
            let complete = end == command.range.len();
            codec::append_fragment(&mut self.bytes, fragment).map_err(error)?;
            let pending = self.pending.as_mut().unwrap();
            if complete {
                pending.index += 1;
                pending.offset = 0;
            } else {
                pending.offset = end;
            }
            self.record(identity);
        }
        self.seal()?;
        Ok(())
    }

    fn clear_packet(&mut self) {
        self.bytes.truncate(BATCH_HEADER_SIZE);
        self.bytes.fill(0);
        self.fragments = 0;
        self.first_command = 0;
        self.last_command = 0;
        self.sealed = None;
    }
}

fn error(error: CodecError) -> CommandError {
    match error {
        CodecError::Length => CommandError::InvalidCommand,
        CodecError::Allocation => CommandError::Allocation,
        _ => CommandError::InvalidCommand,
    }
}
