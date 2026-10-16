use crate::loader::{SystemExport32, SystemExportTarget32};
macro_rules! resource_apis{
    ($( $name:ident=$id:expr => $words:expr ),+ $(,)?)=>{
        #[derive(Clone,Copy,Debug,PartialEq,Eq)]
        #[repr(u32)]
        pub enum ResourceApi32{$($name=$id),+}
        impl ResourceApi32{
            pub const ALL:&'static[Self]=&[$(Self::$name),+];
            pub fn from_id(id:u32)->Option<Self>{match id{$($id=>Some(Self::$name),)+_=>None}}
            pub fn id(self)->u32{self as u32}
            pub fn name(self)->&'static str{match self{$(Self::$name=>stringify!($name)),+}}
            pub fn stack_words(self)->u32{match self{$(Self::$name=>$words),+}}
        }
    }
}
resource_apis! {
    FindResourceA=0x00130001=>3,FindResourceW=0x00130002=>3,
    FindResourceExA=0x00130003=>4,FindResourceExW=0x00130004=>4,
    LoadResource=0x00130005=>2,LockResource=0x00130006=>1,
    SizeofResource=0x00130007=>2,FreeResource=0x00130008=>1,
    EnumResourceTypesA=0x00130009=>3,EnumResourceTypesW=0x0013000a=>3,
    EnumResourceNamesA=0x0013000b=>4,EnumResourceNamesW=0x0013000c=>4,
    EnumResourceLanguagesA=0x0013000d=>5,EnumResourceLanguagesW=0x0013000e=>5,
    EnumResourceTypesExA=0x0013000f=>5,EnumResourceTypesExW=0x00130010=>5,
    EnumResourceNamesExA=0x00130011=>6,EnumResourceNamesExW=0x00130012=>6,
    EnumResourceLanguagesExA=0x00130013=>7,EnumResourceLanguagesExW=0x00130014=>7,
    LoadStringA=0x00130015=>4,LoadStringW=0x00130016=>4,
    UnlockResource=0x00130017=>1,
    FormatMessageA=0x00130018=>7,FormatMessageW=0x00130019=>7,
}
impl ResourceApi32 {
    pub fn wide(self) -> bool {
        self.name().ends_with('W')
    }
    pub fn is_enumeration(self) -> bool {
        self.name().starts_with("EnumResource")
    }
    pub fn is_string(self) -> bool {
        matches!(self, Self::LoadStringA | Self::LoadStringW)
    }
    pub fn is_message(self) -> bool {
        matches!(self, Self::FormatMessageA | Self::FormatMessageW)
    }
}
fn export(api: ResourceApi32) -> SystemExport32 {
    SystemExport32 {
        name: Some(api.name().into()),
        ordinal: None,
        target: SystemExportTarget32::Gate(api.id()),
    }
}
pub fn kernel32_resource_exports() -> Vec<SystemExport32> {
    ResourceApi32::ALL
        .iter()
        .copied()
        .filter(|api| !api.is_string())
        .map(export)
        .collect()
}
pub fn user32_resource_exports() -> Vec<SystemExport32> {
    ResourceApi32::ALL
        .iter()
        .copied()
        .filter(|api| api.is_string())
        .map(export)
        .collect()
}
