mod api;
mod context;
mod graph;
mod sections;
mod types;

pub mod manifest;
pub mod policy;
pub use api::{ActivationApi32, kernel32_activation_exports};
pub use context::{
    ActivationContexts32, ActivationScope32, PreparedActivationContext32,
    PreparedActivationGroup32, PreparedActivationPop32, PreparedActivationPush32,
    PreparedActivationScopeExit32, PreparedActivationThreadRetirement32,
};
pub use graph::{ActivationAssembly32, ActivationGraph32};
pub use sections::{
    ActivationSection32, ActivationSectionEntry32, ActivationSectionKey32,
    encode_activation_sections,
};
pub use types::{ActivationConfig32, ActivationError32, ComRedirection32};

#[cfg(test)]
mod tests;
