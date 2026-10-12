macro_rules! export_process_calls {
    ($($name:ident($($argument:ident: $kind:ty),* $(,)?);)*) => {
        $(
            #[allow(unsafe_code, clippy::too_many_arguments)]
            #[unsafe(export_name = concat!("ring3_abi_v1_", stringify!($name)))]
            pub extern "C" fn $name($($argument: $kind),*) -> u32 {
                {
                    #![forbid(unsafe_code)]
                    crate::process::wasm::$name($($argument),*)
                }
            }
        )*
    };
}

mod calls;
mod cpu;
mod execution;
mod image;
mod instance;
mod memory;
