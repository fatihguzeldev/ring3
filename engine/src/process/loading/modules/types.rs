use crate::loader::ModuleId32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleRuntimeConfig32 {
    pub stack_base: u32,
    pub stack_pages: u32,
    pub runtime_base: u32,
}

impl Default for ModuleRuntimeConfig32 {
    fn default() -> Self {
        Self {
            stack_base: 0x7e00_0000,
            stack_pages: 4,
            runtime_base: 0x7f00_0000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleInitializerKind32 {
    TlsCallback,
    DllMain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleInitializer32 {
    pub token: u64,
    pub module: ModuleId32,
    pub entry: u32,
    pub return_pc: u32,
    pub return_id: u32,
    pub return_esp: u32,
    pub reason: u32,
    pub kind: ModuleInitializerKind32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleOperation32 {
    Pending(ModuleInitializer32),
    NeedBytes {
        request: crate::files::FileReadRequest32,
    },
    Loaded {
        handle: u32,
    },
    Released,
    ExportResolved {
        address: u32,
    },
    InitializationFailed,
}
