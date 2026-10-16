#[path = "../../../support/cpu/fetch/native.rs"]
mod support_fetch;

#[path = "../../../support/cpu/prefix/admission.rs"]
mod support_prefix_admission;

#[path = "entries/native/admission.rs"]
mod dbt_entries;

#[path = "bounded/native/admission.rs"]
mod dbt_region;

#[path = "resident/native/registry.rs"]
mod dbt_resident;

#[path = "dispatch/native/table.rs"]
mod dbt_table_dispatch;

#[path = "capacity/native.rs"]
mod module_capacity;
