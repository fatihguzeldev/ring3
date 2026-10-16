mod api;
mod buffer;
mod capture;
mod device;
pub mod effects;
mod spatial;
mod types;

pub use api::{FactoryGate, InterfaceKind, Method, MethodGate, system_module};
pub use spatial::{Buffer3D, Listener3D, Vector3};
pub use types::{BufferDescription, HResult, SpatialAlgorithm, WaveFormat};

pub use buffer::{Buffer, LockSpan};
pub use capture::{CaptureProgress, CaptureRequest, CaptureRing, PreparedCaptureWrite};
pub use device::Device;

#[cfg(test)]
mod tests;
