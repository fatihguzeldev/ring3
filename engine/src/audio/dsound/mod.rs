mod api;
mod buffer;
mod device;
mod spatial;
mod types;

pub use api::{FactoryGate, InterfaceKind, Method, MethodGate, system_module};
pub use spatial::{Buffer3D, Listener3D, Vector3};
pub use types::{BufferDescription, HResult, WaveFormat};

pub use buffer::{Buffer, LockSpan};
pub use device::Device;

#[cfg(test)]
mod tests;
