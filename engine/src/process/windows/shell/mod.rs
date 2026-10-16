mod arguments;
mod calls;
mod context;
mod folders;

pub(in crate::process) use context::ShellRuntime32;

#[cfg(test)]
mod tests;
