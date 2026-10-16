mod parser;
mod paths;

use super::{ActivationError32 as E, manifest::AssemblyIdentity32};

#[derive(Clone, Debug)]
pub struct BindingRedirect32 {
    pub identity: AssemblyIdentity32,
    pub minimum: [u16; 4],
    pub maximum: [u16; 4],
    pub target: [u16; 4],
}
impl BindingRedirect32 {
    pub fn applies(&self, identity: &AssemblyIdentity32) -> bool {
        let mut reference = self.identity.clone();
        reference.version = identity.version;
        identity.satisfies(&reference) && (self.minimum..=self.maximum).contains(&identity.version)
    }
}
#[derive(Clone, Debug)]
pub struct BindingPolicy32 {
    pub identity: Option<AssemblyIdentity32>,
    pub publisher: bool,
    pub redirects: Vec<BindingRedirect32>,
    pub private_paths: Vec<Vec<u16>>,
}
impl Default for BindingPolicy32 {
    fn default() -> Self {
        Self {
            identity: None,
            publisher: true,
            redirects: Vec::new(),
            private_paths: Vec::new(),
        }
    }
}
impl BindingPolicy32 {
    pub fn redirected(&self, identity: &AssemblyIdentity32) -> Result<Option<[u16; 4]>, E> {
        let mut version = None;
        for redirect in &self.redirects {
            if redirect.applies(identity) {
                if version.is_some_and(|version| version != redirect.target) {
                    return Err(E::ManifestFormat);
                }
                version = Some(redirect.target);
            }
        }
        Ok(version)
    }
}
