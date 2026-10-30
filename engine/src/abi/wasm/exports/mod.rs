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

mod avx;
mod calls;
#[cfg(feature = "observability")]
mod census;
mod clock;
mod core;
mod cpu;
mod detection;
mod events;
mod execution;
mod image;
mod independent;
mod instance;
mod media;
mod memory;
mod modules;
#[cfg(feature = "observability")]
mod observability;
mod paging;
mod windows;
mod xstate;
