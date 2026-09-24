use ring3_core::execution::Process32;

pub fn process() -> Process32 {
    let mut p = super::dinput_cooperative_cases::process_with_cooperation([5, 5]);
    p.memory
        .write(
            u64::from(super::dinput_cooperative_cases::DATA + 64),
            &super::dinput_mouse_cases::MOUSE,
        )
        .unwrap();
    p
}

pub fn imported_foreground_mouse_setting_across_budgets() {
    super::dinput_cooperative_cases::imported_setting_across_budgets(process);
}
