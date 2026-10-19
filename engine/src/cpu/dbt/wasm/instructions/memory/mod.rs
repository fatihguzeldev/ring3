mod address;
mod byte_exchange;
mod byte_store;
mod cpu_scalar;
mod exchange;
mod narrow;
pub(in crate::cpu::dbt::wasm) mod read_view;
mod scalar;
mod stack;
mod word_exchange;
mod word_store;

pub(in crate::cpu::dbt::wasm) use cpu_scalar::outlined::build_function as build_read32_validator;

pub(in crate::cpu::dbt::wasm) use address::{
    address_value, checked_address, checked_offset, effective_address, offset_value,
};
pub(in crate::cpu::dbt::wasm) use byte_exchange::{
    compare_exchange_result as compare_exchange_byte_result,
    exchange_add_result as exchange_add_byte_result, exchange_result as exchange_byte_result,
};
pub(in crate::cpu::dbt::wasm) use byte_store::{
    store as store_byte, store_result as store_byte_result,
    store_result_at_address as store_byte_result_at_address,
};
pub(in crate::cpu::dbt::wasm) use exchange::{
    atomic_and_register_result, atomic_and_result, atomic_or_register_result, atomic_or_result,
    atomic_xor_register_result, atomic_xor_result, bit_test_reset_result, bit_test_set_result,
    compare_exchange_result, exchange_add_result, exchange_result,
};
pub(in crate::cpu::dbt::wasm) use narrow::{
    load_at_address as load_narrow_at_address, load_value as load_narrow_value,
};
pub(in crate::cpu::dbt::wasm) use scalar::{
    load, load_modify_at_address, load_modify_result, load_narrow_for_modify,
    load_narrow_modify_at_address, load_result, load_result_at_address, store, store_result,
    store_result_at_address,
};
pub(in crate::cpu::dbt::wasm) use stack::{
    pop_memory, pop_register, pop_return, pop_word_memory, pop_word_register, push_memory,
    push_word_result,
};
pub(in crate::cpu::dbt::wasm) use word_exchange::{
    compare_exchange_result as compare_exchange_word_result,
    exchange_add_result as exchange_add_word_result, exchange_result as exchange_word_result,
};
pub(in crate::cpu::dbt::wasm) use word_store::{
    store_result as store_word_result, store_result_at_address as store_word_result_at_address,
};
