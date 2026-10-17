mod arrays;
mod calls;
mod context;
mod references;
mod scratch;
mod strings;
mod variants;

pub(in crate::process) use arrays::PreparedAutomationArrays32;
pub(in crate::process) use context::AutomationRuntime32;

#[cfg(test)]
mod tests;
