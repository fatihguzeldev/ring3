mod interfaces;
mod message;
pub(crate) use interfaces::{IID_IPSFACTORYBUFFER, IID_IRPCCHANNELBUFFER};
#[cfg(test)]
mod tests;

pub(crate) use message::RpcMessage32;
