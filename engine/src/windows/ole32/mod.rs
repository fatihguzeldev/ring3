mod apartment;
mod catalog;
mod classes;
mod marshaling;
mod result;
pub(crate) mod stream;

pub use apartment::Apartments32;
pub(crate) use apartment::{ApartmentId32, ApartmentTarget32};
pub use catalog::{FactoryMethod32, Ole32Api32, ole32_module};
pub use classes::{IID_ICLASSFACTORY, KnownClass32};
pub use result::HResult32;

pub(crate) use marshaling::{IID_IMARSHAL, IID_ISEQUENTIALSTREAM, IID_ISTREAM};
pub use marshaling::{RpcChannelMethod32, StreamMethod32};

pub(crate) mod rpc;
