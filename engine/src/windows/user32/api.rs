use crate::{
    loader::{SystemExport32, SystemExportTarget32, SystemModule32},
    windows::CallingConvention32,
};

macro_rules! api_catalog {
    ($( $variant:ident = $id:literal, $words:literal; )+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum User32Api32 { $( $variant = $id, )+ }

        impl User32Api32 {
            pub fn from_id(id: u32) -> Option<Self> { match id { $( $id => Some(Self::$variant), )+ _ => None } }
            pub fn name(self) -> &'static str { match self { $( Self::$variant => stringify!($variant), )+ } }
            pub fn stack_words(self) -> u32 { match self { $( Self::$variant => $words, )+ } }
            pub fn catalog() -> impl ExactSizeIterator<Item = Self> { [$( Self::$variant, )+].into_iter() }
        }
    };
}

api_catalog! {
    RegisterClassA = 0x0002_0001, 1;
    RegisterClassW = 0x0002_0002, 1;
    RegisterClassExA = 0x0002_0003, 1;
    RegisterClassExW = 0x0002_0004, 1;
    UnregisterClassA = 0x0002_0005, 2;
    UnregisterClassW = 0x0002_0006, 2;
    CreateWindowExA = 0x0002_0007, 12;
    CreateWindowExW = 0x0002_0008, 12;
    DestroyWindow = 0x0002_0009, 1;
    GetClientRect = 0x0002_000a, 2;
    GetWindowRect = 0x0002_000b, 2;
    IsWindow = 0x0002_000c, 1;
    GetFocus = 0x0002_000d, 0;
    SetFocus = 0x0002_000e, 1;
    GetCapture = 0x0002_000f, 0;
    SetCapture = 0x0002_0010, 1;
    ReleaseCapture = 0x0002_0011, 0;
    ShowWindow = 0x0002_0012, 2;
    MoveWindow = 0x0002_0013, 6;
    SetWindowPos = 0x0002_0014, 7;
    PeekMessageA = 0x0002_0015, 5;
    PeekMessageW = 0x0002_0016, 5;
    GetMessageA = 0x0002_0017, 4;
    GetMessageW = 0x0002_0018, 4;
    TranslateMessage = 0x0002_0019, 1;
    DispatchMessageA = 0x0002_001a, 1;
    DispatchMessageW = 0x0002_001b, 1;
    PostMessageA = 0x0002_001c, 4;
    PostMessageW = 0x0002_001d, 4;
    PostQuitMessage = 0x0002_001e, 1;
    DefWindowProcA = 0x0002_001f, 4;
    DefWindowProcW = 0x0002_0020, 4;
    GetWindowLongA = 0x0002_0021, 2;
    GetWindowLongW = 0x0002_0022, 2;
    SetWindowLongA = 0x0002_0023, 3;
    SetWindowLongW = 0x0002_0024, 3;
    CallWindowProcA = 0x0002_0025, 5;
    CallWindowProcW = 0x0002_0026, 5;
    SendMessageA = 0x0002_0027, 4;
    SendMessageW = 0x0002_0028, 4;
}

impl User32Api32 {
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn convention(self) -> CallingConvention32 {
        CallingConvention32::Stdcall
    }
    pub fn resolve(module: &str, symbol: &str) -> Option<Self> {
        if !module.eq_ignore_ascii_case("user32.dll") {
            return None;
        }
        Self::catalog().find(|api| api.name() == symbol)
    }
    pub fn wide(self) -> bool {
        self.name().ends_with('W')
    }
}

pub fn user32_module(preferred_base: u32) -> SystemModule32 {
    SystemModule32 {
        name: "user32.dll".into(),
        preferred_base,
        exports: User32Api32::catalog()
            .map(|api| SystemExport32 {
                name: Some(api.name().into()),
                ordinal: None,
                target: SystemExportTarget32::Gate(api.id()),
            })
            .collect(),
    }
}
