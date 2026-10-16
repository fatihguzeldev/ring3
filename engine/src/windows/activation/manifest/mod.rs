mod identity;
mod parser;
mod records;
mod types;
mod validation;

pub use identity::AssemblyIdentity32;
pub use types::{
    AssemblyDependency32, AssemblyFile32, ComClass32, ComInterface32, Manifest32,
    ManifestSetting32, RequestedLevel32, TypeLibrary32, VersionRedirect32, WindowClass32,
};

pub(in crate::windows::activation) use identity::version;
pub(in crate::windows::activation) use parser::decode;
