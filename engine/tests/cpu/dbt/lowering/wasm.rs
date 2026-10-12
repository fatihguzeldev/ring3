#[path = "../../../support/wasm.rs"]
mod support_wasm;

#[path = "register/wasm/extensions.rs"]
mod dbt_extensions_wasm;

#[path = "control/wasm/gate.rs"]
mod dbt_gate_wasm;

#[path = "control/wasm/indirect.rs"]
mod dbt_indirect_wasm;

#[path = "register/wasm/lea.rs"]
mod dbt_lea_wasm;

#[path = "register/wasm/logical.rs"]
mod dbt_logical_wasm;

#[path = "memory/wasm/binary.rs"]
mod dbt_memory_binary_wasm;

#[path = "memory/wasm/compare.rs"]
mod dbt_memory_compare_wasm;

#[path = "memory/wasm/extensions.rs"]
mod dbt_memory_extensions_wasm;

#[path = "memory/wasm/not.rs"]
mod dbt_memory_not_wasm;

#[path = "memory/wasm/stack.rs"]
mod dbt_memory_stack_wasm;

#[path = "memory/wasm/unary.rs"]
mod dbt_memory_unary_wasm;

#[path = "memory/wasm/read.rs"]
mod dbt_memory_wasm;

#[path = "memory/wasm/write.rs"]
mod dbt_memory_write_wasm;

#[path = "stack/wasm/values.rs"]
mod dbt_stack_values_wasm;

#[path = "stack/wasm/operations.rs"]
mod dbt_stack_wasm;

#[path = "register/wasm/unary.rs"]
mod dbt_unary_wasm;

#[path = "integration/wasm/core.rs"]
mod dbt_wasm;
