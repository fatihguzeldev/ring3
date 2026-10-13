use super::super::cpu_store;

pub(crate) fn cpu_store8(address: u32, value: u32) -> u32 {
    cpu_store(|instance| instance.cpu_store8(address, value))
}

pub(crate) fn cpu_store_resident8(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    value: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_store_resident8(key, id, address, value)
    })
}

pub(crate) fn cpu_store16(address: u32, value: u32) -> u32 {
    cpu_store(|instance| instance.cpu_store16(address, value))
}

pub(crate) fn cpu_store_resident16(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    value: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_store_resident16(key, id, address, value)
    })
}

pub(crate) fn cpu_store32(address: u32, value: u32) -> u32 {
    cpu_store(|instance| instance.cpu_store32(address, value))
}

pub(crate) fn cpu_store_resident32(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    value: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_store_resident32(key, id, address, value)
    })
}

pub(crate) fn cpu_exchange8(address: u32, value: u32) -> u32 {
    cpu_store(|instance| instance.cpu_exchange8(address, value))
}

pub(crate) fn cpu_exchange_resident8(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    value: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_exchange_resident8(key, id, address, value)
    })
}

pub(crate) fn cpu_exchange16(address: u32, value: u32) -> u32 {
    cpu_store(|instance| instance.cpu_exchange16(address, value))
}

pub(crate) fn cpu_exchange_resident16(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    value: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_exchange_resident16(key, id, address, value)
    })
}

pub(crate) fn cpu_exchange32(address: u32, value: u32) -> u32 {
    cpu_store(|instance| instance.cpu_exchange32(address, value))
}

pub(crate) fn cpu_exchange_resident32(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    value: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_exchange_resident32(key, id, address, value)
    })
}

pub(crate) fn cpu_compare_exchange8(address: u32, expected: u32, desired: u32) -> u32 {
    cpu_store(|instance| instance.cpu_compare_exchange8(address, expected, desired))
}

pub(crate) fn cpu_compare_exchange_resident8(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    expected: u32,
    desired: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_compare_exchange_resident8(key, id, address, expected, desired)
    })
}

pub(crate) fn cpu_compare_exchange16(address: u32, expected: u32, desired: u32) -> u32 {
    cpu_store(|instance| instance.cpu_compare_exchange16(address, expected, desired))
}

pub(crate) fn cpu_compare_exchange_resident16(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    expected: u32,
    desired: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_compare_exchange_resident16(key, id, address, expected, desired)
    })
}

pub(crate) fn cpu_compare_exchange32(address: u32, expected: u32, desired: u32) -> u32 {
    cpu_store(|instance| instance.cpu_compare_exchange32(address, expected, desired))
}

pub(crate) fn cpu_compare_exchange_resident32(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    expected: u32,
    desired: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_compare_exchange_resident32(key, id, address, expected, desired)
    })
}

pub(crate) fn cpu_exchange_add8(address: u32, source: u32) -> u32 {
    cpu_store(|instance| instance.cpu_exchange_add8(address, source))
}

pub(crate) fn cpu_exchange_add_resident8(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    source: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_exchange_add_resident8(key, id, address, source)
    })
}

pub(crate) fn cpu_exchange_add16(address: u32, source: u32) -> u32 {
    cpu_store(|instance| instance.cpu_exchange_add16(address, source))
}

pub(crate) fn cpu_exchange_add_resident16(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    source: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_exchange_add_resident16(key, id, address, source)
    })
}

pub(crate) fn cpu_exchange_add32(address: u32, source: u32) -> u32 {
    cpu_store(|instance| instance.cpu_exchange_add32(address, source))
}

pub(crate) fn cpu_exchange_add_resident32(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    source: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_exchange_add_resident32(key, id, address, source)
    })
}

pub(crate) fn cpu_bit_test_set32(address: u32, bit: u32) -> u32 {
    cpu_store(|instance| instance.cpu_bit_test_set32(address, bit))
}

pub(crate) fn cpu_bit_test_set_resident32(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    bit: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_bit_test_set_resident32(key, id, address, bit)
    })
}

pub(crate) fn cpu_bit_test_reset32(address: u32, bit: u32) -> u32 {
    cpu_store(|instance| instance.cpu_bit_test_reset32(address, bit))
}

pub(crate) fn cpu_bit_test_reset_resident32(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    bit: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_bit_test_reset_resident32(key, id, address, bit)
    })
}

pub(crate) fn cpu_atomic_or_u32(address: u32, mask: u32) -> u32 {
    cpu_store(|instance| instance.cpu_atomic_or_u32(address, mask))
}

pub(crate) fn cpu_atomic_or_resident_u32(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    mask: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_atomic_or_resident_u32(key, id, address, mask)
    })
}

pub(crate) fn cpu_atomic_and_u32(address: u32, mask: u32) -> u32 {
    cpu_store(|instance| instance.cpu_atomic_and_u32(address, mask))
}

pub(crate) fn cpu_atomic_and_resident_u32(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    mask: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_atomic_and_resident_u32(key, id, address, mask)
    })
}

pub(crate) fn cpu_atomic_xor_u32(address: u32, mask: u32) -> u32 {
    cpu_store(|instance| instance.cpu_atomic_xor_u32(address, mask))
}

pub(crate) fn cpu_atomic_xor_resident_u32(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    mask: u32,
) -> u32 {
    cpu_store(|instance| {
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        instance.cpu_atomic_xor_resident_u32(key, id, address, mask)
    })
}
