#[path = "../../../support/cpu/arena.rs"]
mod support_arena;

#[path = "../../../support/cpu/prefix/admission.rs"]
mod support_prefix_admission;

#[path = "../../../support/cpu/fetch/native.rs"]
mod support_fetch;

#[path = "support/capacity.rs"]
mod support_capacity;

#[path = "register/native/extensions.rs"]
mod dbt_extensions;

#[path = "control/native/gate.rs"]
mod dbt_gate;

#[path = "control/native/indirect.rs"]
mod dbt_indirect;

#[path = "register/native/lea.rs"]
mod dbt_lea;

#[path = "register/native/logical.rs"]
mod dbt_logical;

#[path = "memory/native/read.rs"]
mod dbt_memory;

#[path = "memory/native/binary.rs"]
mod dbt_memory_binary;

#[path = "memory/native/compare.rs"]
mod dbt_memory_compare;

#[path = "memory/native/extensions.rs"]
mod dbt_memory_extensions;

#[path = "memory/native/not.rs"]
mod dbt_memory_not;

#[path = "memory/native/stack.rs"]
mod dbt_memory_stack;

#[path = "memory/native/unary.rs"]
mod dbt_memory_unary;

#[path = "memory/native/write.rs"]
mod dbt_memory_write;

#[path = "stack/native/operations.rs"]
mod dbt_stack;

#[path = "stack/native/values.rs"]
mod dbt_stack_values;

#[path = "register/native/unary.rs"]
mod dbt_unary;
