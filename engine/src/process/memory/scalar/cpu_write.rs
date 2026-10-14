use crate::{
    abi::{
        arena::HELPER_OFFSET,
        memory_helper::{CpuScalarOperation, HELPER_SIZE, encode_cpu_paging_scalar_result},
    },
    cpu::x86::{CpuPagingError32, PagingAccess32},
    memory::GuestAddress,
    process::{EngineInstance, HostError, StoreCompletion},
};

enum Mutation {
    Store(u32),
    Exchange(u32),
    CompareExchange(u32, u32),
    ExchangeAdd(u32),
    BitSet(u32),
    BitReset(u32),
    Or(u32),
    And(u32),
    Xor(u32),
}

impl EngineInstance {
    pub fn cpu_store8(&mut self, address: u32, value: u32) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<1>(None, address, Mutation::Store(value))
    }

    pub fn cpu_store_resident8(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<1>(Some((key, id)), address, Mutation::Store(value))
    }

    pub fn cpu_store16(&mut self, address: u32, value: u32) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<2>(None, address, Mutation::Store(value))
    }

    pub fn cpu_store_resident16(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<2>(Some((key, id)), address, Mutation::Store(value))
    }

    pub fn cpu_store32(&mut self, address: u32, value: u32) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(None, address, Mutation::Store(value))
    }

    pub fn cpu_store_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(Some((key, id)), address, Mutation::Store(value))
    }

    pub fn cpu_exchange8(
        &mut self,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<1>(None, address, Mutation::Exchange(value))
    }

    pub fn cpu_exchange_resident8(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<1>(Some((key, id)), address, Mutation::Exchange(value))
    }

    pub fn cpu_exchange16(
        &mut self,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<2>(None, address, Mutation::Exchange(value))
    }

    pub fn cpu_exchange_resident16(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<2>(Some((key, id)), address, Mutation::Exchange(value))
    }

    pub fn cpu_exchange32(
        &mut self,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(None, address, Mutation::Exchange(value))
    }

    pub fn cpu_exchange_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(Some((key, id)), address, Mutation::Exchange(value))
    }

    pub fn cpu_compare_exchange8(
        &mut self,
        address: u32,
        expected: u32,
        desired: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<1>(None, address, Mutation::CompareExchange(expected, desired))
    }

    pub fn cpu_compare_exchange_resident8(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        expected: u32,
        desired: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<1>(
            Some((key, id)),
            address,
            Mutation::CompareExchange(expected, desired),
        )
    }

    pub fn cpu_compare_exchange16(
        &mut self,
        address: u32,
        expected: u32,
        desired: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<2>(None, address, Mutation::CompareExchange(expected, desired))
    }

    pub fn cpu_compare_exchange_resident16(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        expected: u32,
        desired: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<2>(
            Some((key, id)),
            address,
            Mutation::CompareExchange(expected, desired),
        )
    }

    pub fn cpu_compare_exchange32(
        &mut self,
        address: u32,
        expected: u32,
        desired: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(None, address, Mutation::CompareExchange(expected, desired))
    }

    pub fn cpu_compare_exchange_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        expected: u32,
        desired: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(
            Some((key, id)),
            address,
            Mutation::CompareExchange(expected, desired),
        )
    }

    pub fn cpu_exchange_add8(
        &mut self,
        address: u32,
        source: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<1>(None, address, Mutation::ExchangeAdd(source))
    }

    pub fn cpu_exchange_add_resident8(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        source: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<1>(Some((key, id)), address, Mutation::ExchangeAdd(source))
    }

    pub fn cpu_exchange_add16(
        &mut self,
        address: u32,
        source: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<2>(None, address, Mutation::ExchangeAdd(source))
    }

    pub fn cpu_exchange_add_resident16(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        source: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<2>(Some((key, id)), address, Mutation::ExchangeAdd(source))
    }

    pub fn cpu_exchange_add32(
        &mut self,
        address: u32,
        source: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(None, address, Mutation::ExchangeAdd(source))
    }

    pub fn cpu_exchange_add_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        source: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(Some((key, id)), address, Mutation::ExchangeAdd(source))
    }

    pub fn cpu_bit_test_set32(
        &mut self,
        address: u32,
        bit: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(None, address, Mutation::BitSet(bit))
    }

    pub fn cpu_bit_test_set_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        bit: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(Some((key, id)), address, Mutation::BitSet(bit))
    }

    pub fn cpu_bit_test_reset32(
        &mut self,
        address: u32,
        bit: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(None, address, Mutation::BitReset(bit))
    }

    pub fn cpu_bit_test_reset_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        bit: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(Some((key, id)), address, Mutation::BitReset(bit))
    }

    pub fn cpu_atomic_or_u32(
        &mut self,
        address: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(None, address, Mutation::Or(mask))
    }

    pub fn cpu_atomic_or_resident_u32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(Some((key, id)), address, Mutation::Or(mask))
    }

    pub fn cpu_atomic_and_u32(
        &mut self,
        address: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(None, address, Mutation::And(mask))
    }

    pub fn cpu_atomic_and_resident_u32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(Some((key, id)), address, Mutation::And(mask))
    }

    pub fn cpu_atomic_xor_u32(
        &mut self,
        address: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(None, address, Mutation::Xor(mask))
    }

    pub fn cpu_atomic_xor_resident_u32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.cpu_scalar_mutation::<4>(Some((key, id)), address, Mutation::Xor(mask))
    }

    fn cpu_scalar_mutation<const WIDTH: u32>(
        &mut self,
        binding: Option<(u64, u64)>,
        address: u32,
        mutation: Mutation,
    ) -> Result<StoreCompletion, HostError> {
        let width = WIDTH;
        match binding {
            Some((key, id)) => self.guard_material_resident(key, id)?,
            None => self.guard_material(self.key, self.generation())?,
        }
        let at = GuestAddress(address);
        let operation = if matches!(&mutation, Mutation::Store(_)) {
            CpuScalarOperation::Store
        } else {
            CpuScalarOperation::Modify
        };
        let preflight = if operation == CpuScalarOperation::Store {
            self.preflight_cpu_paging32(at, width, PagingAccess32::Write)
        } else {
            self.preflight_cpu_modify32(at, width)
        };
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let version = memory.code_validation_version();
        let result = preflight.and_then(|()| {
            (match (mutation, width) {
                (Mutation::Store(value), 1 | 2 | 4) => memory
                    .cpu_write(at, &value.to_le_bytes()[..width as usize])
                    .map(|()| 0),
                (Mutation::Exchange(value), 1) => memory.exchange8(at, value as u8).map(u32::from),
                (Mutation::Exchange(value), 2) => {
                    memory.cpu_exchange16(at, value as u16).map(u32::from)
                }
                (Mutation::Exchange(value), 4) => memory.cpu_exchange32(at, value),
                (Mutation::CompareExchange(expected, desired), 1) => memory
                    .compare_exchange8(at, expected as u8, desired as u8)
                    .map(u32::from),
                (Mutation::CompareExchange(expected, desired), 2) => memory
                    .cpu_compare_exchange16(at, expected as u16, desired as u16)
                    .map(u32::from),
                (Mutation::CompareExchange(expected, desired), 4) => {
                    memory.cpu_compare_exchange32(at, expected, desired)
                }
                (Mutation::ExchangeAdd(source), 1) => {
                    memory.exchange_add8(at, source as u8).map(u32::from)
                }
                (Mutation::ExchangeAdd(source), 2) => {
                    memory.cpu_exchange_add16(at, source as u16).map(u32::from)
                }
                (Mutation::ExchangeAdd(source), 4) => memory.cpu_exchange_add32(at, source),
                (Mutation::BitSet(bit), 4) => memory.cpu_bit_test_set32(at, bit),
                (Mutation::BitReset(bit), 4) => memory.cpu_bit_test_reset32(at, bit),
                (Mutation::Or(mask), 4) => memory.cpu_atomic_or_u32(at, mask),
                (Mutation::And(mask), 4) => memory.cpu_atomic_and_u32(at, mask),
                (Mutation::Xor(mask), 4) => memory.cpu_atomic_xor_u32(at, mask),
                _ => unreachable!(
                    "scalar mutation and const width selected by typed public wrappers"
                ),
            })
            .map_err(CpuPagingError32::Memory)
        });
        let changed = result.is_ok() && version != memory.code_validation_version();
        encode_cpu_paging_scalar_result(
            at,
            width,
            operation,
            result,
            &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)?;
        self.cpu_store_currentness(changed, binding.map(|(_, id)| id))
    }
}

#[cfg(test)]
#[path = "cpu_write/tests.rs"]
mod tests;
