use crate::loader::{SystemExport32, SystemExportTarget32};
macro_rules! apis {
    ($($name:ident=$id:expr=>$words:expr),+$(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum RawInputApi32 {$($name=$id),+}
        impl RawInputApi32 {
            pub const ALL: &'static [Self] = &[$(Self::$name),+];
            pub fn from_id(id:u32)->Option<Self>{match id {$($id=>Some(Self::$name),)+ _=>None}}
            pub fn id(self)->u32{self as u32}
            pub fn name(self)->&'static str{match self{$(Self::$name=>stringify!($name)),+}}
            pub fn stack_words(self)->u32{match self{$(Self::$name=>$words),+}}
        }
    }
}
apis! {
    RegisterRawInputDevices=0x0014_0001=>3,
    GetRegisteredRawInputDevices=0x0014_0002=>3,
    GetRawInputData=0x0014_0003=>5,
    GetRawInputBuffer=0x0014_0004=>3,
    GetRawInputDeviceList=0x0014_0005=>3,
    GetRawInputDeviceInfoA=0x0014_0006=>4,
    GetRawInputDeviceInfoW=0x0014_0007=>4,
    DefRawInputProc=0x0014_0008=>3,
}
impl RawInputApi32 {
    pub fn failure_value(self) -> u32 {
        if matches!(self, Self::RegisterRawInputDevices) {
            0
        } else {
            u32::MAX
        }
    }
}
pub fn raw_input_exports() -> impl Iterator<Item = SystemExport32> {
    RawInputApi32::ALL
        .iter()
        .copied()
        .map(|api| SystemExport32 {
            name: Some(api.name().into()),
            ordinal: None,
            target: SystemExportTarget32::Gate(api.id()),
        })
}
