mod continuation;
mod link;

pub fn emit_dispatch() -> Vec<u8> {
    link::emit()
}

#[cfg(test)]
#[path = "dispatch/tests.rs"]
mod tests;
