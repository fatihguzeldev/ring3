#[path = "support/constant_load_cases.rs"]
mod constant_load_cases;
#[path = "support/division_executable.rs"]
mod division_executable;
#[path = "support/executable.rs"]
mod executable;
#[path = "support/fpu_wait_executable.rs"]
mod fpu_wait_executable;
#[path = "support/integer_cpu_state.rs"]
mod integer_cpu_state;
#[path = "support/integer_division_cases.rs"]
mod integer_division_cases;
#[path = "support/integer_store_executable.rs"]
mod integer_store_executable;
#[path = "support/nan_compare_cases.rs"]
mod nan_compare_cases;
#[path = "support/register_compare_pop_cases.rs"]
mod register_compare_pop_cases;
#[path = "support/single_register_subtract_cases.rs"]
mod single_register_subtract_cases;
#[path = "support/x87_executable.rs"]
mod x87_executable;
#[path = "support/x87_register_executable.rs"]
mod x87_register_executable;
#[path = "support/x87_roundup_cases.rs"]
mod x87_roundup_cases;
#[path = "support/x87_scaling_executable.rs"]
mod x87_scaling_executable;
#[path = "support/x87_status_executable.rs"]
mod x87_status_executable;
#[path = "support/x87_trigonometry_cases.rs"]
mod x87_trigonometry_cases;

#[test]
fn full_turn_trigonometry() {
    x87_trigonometry_cases::full_turn();
}

#[path = "x87/fpu_wait.rs"]
mod fpu_wait;
#[path = "x87/x87_absolute_value.rs"]
mod x87_absolute_value;
#[path = "x87/x87_arithmetic.rs"]
mod x87_arithmetic;
#[path = "x87/x87_compare_pop_twice.rs"]
mod x87_compare_pop_twice;
#[path = "x87/x87_control.rs"]
mod x87_control;
#[path = "x87/x87_cosine.rs"]
mod x87_cosine;
#[path = "x87/x87_data.rs"]
mod x87_data;
#[path = "x87/x87_divide_pop.rs"]
mod x87_divide_pop;
#[path = "x87/x87_division.rs"]
mod x87_division;
#[path = "x87/x87_exchange.rs"]
mod x87_exchange;
#[path = "x87/x87_integer_add.rs"]
mod x87_integer_add;
#[path = "x87/x87_integer_division.rs"]
mod x87_integer_division;
#[path = "x87/x87_integer_multiply.rs"]
mod x87_integer_multiply;
#[path = "x87/x87_integer_stores.rs"]
mod x87_integer_stores;
#[path = "x87/x87_load_constants.rs"]
mod x87_load_constants;
#[path = "x87/x87_memory_add.rs"]
mod x87_memory_add;
#[path = "x87/x87_memory_subtract.rs"]
mod x87_memory_subtract;
#[path = "x87/x87_nan_compare.rs"]
mod x87_nan_compare;
#[path = "x87/x87_register_add.rs"]
mod x87_register_add;
#[path = "x87/x87_register_compare_pop.rs"]
mod x87_register_compare_pop;
#[path = "x87/x87_register_div.rs"]
mod x87_register_div;
#[path = "x87/x87_register_multiply.rs"]
mod x87_register_multiply;
#[path = "x87/x87_register_stores.rs"]
mod x87_register_stores;
#[path = "x87/x87_register_subtract.rs"]
mod x87_register_subtract;
#[path = "x87/x87_roundup_status.rs"]
mod x87_roundup_status;
#[path = "x87/x87_scaling.rs"]
mod x87_scaling;
#[path = "x87/x87_sine.rs"]
mod x87_sine;
#[path = "x87/x87_single_precision_store.rs"]
mod x87_single_precision_store;
#[path = "x87/x87_single_precision_sum.rs"]
mod x87_single_precision_sum;
#[path = "x87/x87_single_register_subtract.rs"]
mod x87_single_register_subtract;
#[path = "x87/x87_status.rs"]
mod x87_status;
#[path = "x87/x87_store_single.rs"]
mod x87_store_single;
#[path = "x87/x87_tangent.rs"]
mod x87_tangent;
