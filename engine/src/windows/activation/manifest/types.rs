use super::AssemblyIdentity32;
use crate::windows::com::ServerThreadingModel32;

#[derive(Clone, Debug)]
pub struct Manifest32 {
    pub identity: AssemblyIdentity32,
    pub no_inherit: bool,
    pub no_inheritable: bool,
    pub dependencies: Vec<AssemblyDependency32>,
    pub files: Vec<AssemblyFile32>,
    pub interfaces: Vec<ComInterface32>,
    pub requested_level: Option<RequestedLevel32>,
    pub supported_os: Vec<[u8; 16]>,
    pub settings: Vec<ManifestSetting32>,
}
#[derive(Clone, Debug)]
pub struct ManifestSetting32 {
    pub namespace: String,
    pub name: String,
    pub value: String,
}
#[derive(Clone, Debug)]
pub struct AssemblyDependency32 {
    pub identity: AssemblyIdentity32,
    pub optional: bool,
    pub redirects: Vec<VersionRedirect32>,
}
#[derive(Clone, Copy, Debug)]
pub struct VersionRedirect32 {
    pub minimum: [u16; 4],
    pub maximum: [u16; 4],
    pub target: [u16; 4],
}
#[derive(Clone, Debug)]
pub struct AssemblyFile32 {
    pub name: String,
    pub classes: Vec<ComClass32>,
    pub interfaces: Vec<ComInterface32>,
    pub type_libraries: Vec<TypeLibrary32>,
    pub window_classes: Vec<WindowClass32>,
}
#[derive(Clone, Debug)]
pub struct ComClass32 {
    pub clsid: [u8; 16],
    pub threading: ServerThreadingModel32,
    pub typelib: Option<[u8; 16]>,
    pub progids: Vec<String>,
    pub description: Option<String>,
    pub misc_status: [u32; 5],
    pub misc_mask: u8,
}
#[derive(Clone, Debug)]
pub struct ComInterface32 {
    pub iid: [u8; 16],
    pub name: Option<String>,
    pub proxy: Option<[u8; 16]>,
    pub typelib: Option<[u8; 16]>,
    pub base: Option<[u8; 16]>,
    pub methods: Option<u32>,
    pub threading: ServerThreadingModel32,
}
#[derive(Clone, Debug)]
pub struct TypeLibrary32 {
    pub id: [u8; 16],
    pub version: [u16; 2],
    pub resource: u16,
    pub flags: u16,
    pub help_directory: String,
}
#[derive(Clone, Debug)]
pub struct WindowClass32 {
    pub name: String,
    pub versioned: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestedLevel32 {
    AsInvoker { ui_access: bool },
    HighestAvailable { ui_access: bool },
    RequireAdministrator { ui_access: bool },
}
