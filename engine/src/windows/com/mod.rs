mod identity;
mod registry;

pub use identity::{
    ComError32, ComIdentity32, ComObjectId32, IID_IUNKNOWN, InterfaceBinding32, InterfaceId,
    ReferenceState32, ReferenceTransition32, ReleaseOutcome32,
};
pub use registry::ComRegistry32;
