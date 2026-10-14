macro_rules! export_detection_guards {
    ($($name:ident($($argument:ident: $kind:ty),*);)*) => {
        $(
            #[allow(unsafe_code, clippy::too_many_arguments)]
            #[unsafe(export_name = concat!("ring3_abi_v1_", stringify!($name)))]
            pub extern "C" fn $name($($argument: $kind),*) -> i64 {
                {
                    #![forbid(unsafe_code)]
                    crate::process::wasm::$name($($argument),*)
                }
            }
        )*
    };
}

export_detection_guards! {
    guard_detect(low: u32, high: u32, generation: u32, state: u32, exit: u32, cancel: u32);
    guard_detect_resident(low: u32, high: u32, id_low: u32, id_high: u32, state: u32, exit: u32, cancel: u32);
}

export_process_calls! {
    finish_exception_detection(epoch: i64, reason: u32, detail: u32, address: u32, access: u32, length: u32, debug_origin: u32, restart_pc: u32, restart_retired: u32, restart_flags: u32, restart_shadow: u32, restart_work: u32);
}
