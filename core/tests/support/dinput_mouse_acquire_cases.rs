use ring3_core::execution::Process32;

pub fn process() -> Process32 {
    let mut p = super::dinput_acquire_cases::process_with_format(
        5,
        super::dinput_mouse_format_cases::standard,
    );
    p.memory
        .write(
            u64::from(super::dinput_acquire_cases::DATA + 64),
            &super::dinput_mouse_cases::MOUSE,
        )
        .unwrap();
    p
}

pub fn imported_mouse_acquisition_across_budgets() {
    super::dinput_acquire_cases::imported_acquisition_across_budgets(process);
}
