use super::{cpu_store, mutate};

pub(crate) fn map(address: u32, pages: u32, permissions: u32) -> u32 {
    mutate(|instance| instance.map(address, pages, permissions))
}

pub(crate) fn protect(address: u32, pages: u32, permissions: u32) -> u32 {
    mutate(|instance| instance.protect(address, pages, permissions))
}

pub(crate) fn unmap(address: u32, pages: u32) -> u32 {
    mutate(|instance| instance.unmap(address, pages))
}

pub(crate) fn upload(address: u32, length: u32) -> u32 {
    mutate(|instance| instance.upload(address, length))
}

pub(crate) fn read8(address: u32) -> u32 {
    mutate(|instance| instance.read8(address))
}

pub(crate) fn read16(address: u32) -> u32 {
    mutate(|instance| instance.read16(address))
}

pub(crate) fn read32(address: u32) -> u32 {
    mutate(|instance| instance.read32(address))
}

pub(crate) fn write8(address: u32, value: u32) -> u32 {
    mutate(|instance| instance.write8(address, value))
}

pub(crate) fn write32(address: u32, value: u32) -> u32 {
    mutate(|instance| instance.write32(address, value))
}

pub(crate) fn write_words32(count: u32) -> u32 {
    mutate(|instance| instance.write_words32(count))
}

pub(crate) fn store8(address: u32, value: u32) -> u32 {
    cpu_store(|instance| instance.store8(address, value))
}

pub(crate) fn store_resident8(
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
        instance.store_resident8(key, id, address, value)
    })
}

pub(crate) fn store16(address: u32, value: u32) -> u32 {
    cpu_store(|instance| instance.store16(address, value))
}

pub(crate) fn store_resident16(
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
        instance.store_resident16(key, id, address, value)
    })
}

pub(crate) fn store32(address: u32, value: u32) -> u32 {
    cpu_store(|instance| instance.store32(address, value))
}

pub(crate) fn store_resident32(
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
        instance.store_resident32(key, id, address, value)
    })
}

pub(crate) fn exchange16(address: u32, value: u32) -> u32 {
    cpu_store(|instance| instance.exchange16(address, value))
}

pub(crate) fn exchange_resident16(
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
        instance.exchange_resident16(key, id, address, value)
    })
}

pub(crate) fn exchange8(address: u32, value: u32) -> u32 {
    cpu_store(|instance| instance.exchange8(address, value))
}

pub(crate) fn exchange_resident8(
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
        instance.exchange_resident8(key, id, address, value)
    })
}

pub(crate) fn exchange32(address: u32, value: u32) -> u32 {
    cpu_store(|instance| instance.exchange32(address, value))
}

pub(crate) fn exchange_resident32(
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
        instance.exchange_resident32(key, id, address, value)
    })
}

pub(crate) fn compare_exchange8(address: u32, expected: u32, desired: u32) -> u32 {
    cpu_store(|instance| instance.compare_exchange8(address, expected, desired))
}

pub(crate) fn compare_exchange_resident8(
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
        instance.compare_exchange_resident8(key, id, address, expected, desired)
    })
}

pub(crate) fn compare_exchange16(address: u32, expected: u32, desired: u32) -> u32 {
    cpu_store(|instance| instance.compare_exchange16(address, expected, desired))
}

pub(crate) fn compare_exchange_resident16(
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
        instance.compare_exchange_resident16(key, id, address, expected, desired)
    })
}

pub(crate) fn compare_exchange32(address: u32, expected: u32, desired: u32) -> u32 {
    cpu_store(|instance| instance.compare_exchange32(address, expected, desired))
}

pub(crate) fn compare_exchange_resident32(
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
        instance.compare_exchange_resident32(key, id, address, expected, desired)
    })
}

pub(crate) fn exchange_add32(address: u32, source: u32) -> u32 {
    cpu_store(|instance| instance.exchange_add32(address, source))
}

pub(crate) fn exchange_add_resident32(
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
        instance.exchange_add_resident32(key, id, address, source)
    })
}

pub(crate) fn exchange_add8(address: u32, source: u32) -> u32 {
    cpu_store(|instance| instance.exchange_add8(address, source))
}

pub(crate) fn exchange_add_resident8(
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
        instance.exchange_add_resident8(key, id, address, source)
    })
}

pub(crate) fn exchange_add16(address: u32, source: u32) -> u32 {
    cpu_store(|instance| instance.exchange_add16(address, source))
}

pub(crate) fn exchange_add_resident16(
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
        instance.exchange_add_resident16(key, id, address, source)
    })
}

pub(crate) fn bit_test_set32(address: u32, bit: u32) -> u32 {
    cpu_store(|instance| instance.bit_test_set32(address, bit))
}

pub(crate) fn bit_test_set_resident32(
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
        instance.bit_test_set_resident32(key, id, address, bit)
    })
}

pub(crate) fn bit_test_reset32(address: u32, bit: u32) -> u32 {
    cpu_store(|instance| instance.bit_test_reset32(address, bit))
}

pub(crate) fn bit_test_reset_resident32(
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
        instance.bit_test_reset_resident32(key, id, address, bit)
    })
}

pub(crate) fn atomic_or_u32(address: u32, mask: u32) -> u32 {
    cpu_store(|instance| instance.atomic_or_u32(address, mask))
}

pub(crate) fn atomic_or_resident_u32(
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
        instance.atomic_or_resident_u32(key, id, address, mask)
    })
}

pub(crate) fn atomic_and_u32(address: u32, mask: u32) -> u32 {
    cpu_store(|instance| instance.atomic_and_u32(address, mask))
}

pub(crate) fn atomic_and_resident_u32(
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
        instance.atomic_and_resident_u32(key, id, address, mask)
    })
}

pub(crate) fn atomic_xor_u32(address: u32, mask: u32) -> u32 {
    cpu_store(|instance| instance.atomic_xor_u32(address, mask))
}

pub(crate) fn atomic_xor_resident_u32(
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
        instance.atomic_xor_resident_u32(key, id, address, mask)
    })
}
