#[derive(Debug)]
pub(crate) struct ConsoleControl32 {
    handlers: [u32; 64],
    count: usize,
    ignore_ctrl_c: bool,
}
pub(crate) enum PreparedConsoleControl32 {
    Ignore(bool),
    Add(u32),
    Remove(usize),
}
impl ConsoleControl32 {
    pub(crate) const fn new() -> Self {
        Self {
            handlers: [0; 64],
            count: 0,
            ignore_ctrl_c: false,
        }
    }
    pub(crate) fn prepare(
        &self,
        procedure: u32,
        add: bool,
    ) -> Result<PreparedConsoleControl32, u32> {
        if procedure == 0 {
            return Ok(PreparedConsoleControl32::Ignore(add));
        }
        if add {
            if self.count == self.handlers.len() {
                Err(8)
            } else {
                Ok(PreparedConsoleControl32::Add(procedure))
            }
        } else {
            self.handlers[..self.count]
                .iter()
                .rposition(|value| *value == procedure)
                .map(PreparedConsoleControl32::Remove)
                .ok_or(87)
        }
    }
    pub(crate) fn commit(&mut self, prepared: PreparedConsoleControl32) {
        match prepared {
            PreparedConsoleControl32::Ignore(ignore) => self.ignore_ctrl_c = ignore,
            PreparedConsoleControl32::Add(procedure) => {
                self.handlers[self.count] = procedure;
                self.count += 1;
            }
            PreparedConsoleControl32::Remove(index) => {
                self.handlers.copy_within(index + 1..self.count, index);
                self.count -= 1;
                self.handlers[self.count] = 0;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handler_registration_preserves_duplicates_order_and_failure_neutral_capacity() {
        let mut context = ConsoleControl32::new();
        for procedure in [11, 22, 11] {
            let prepared = context.prepare(procedure, true).unwrap();
            context.commit(prepared);
        }
        context.commit(context.prepare(11, false).unwrap());
        assert_eq!(&context.handlers[..context.count], &[11, 22]);
        assert!(matches!(context.prepare(33, false), Err(87)));
        context.commit(context.prepare(0, true).unwrap());
        assert!(context.ignore_ctrl_c);
        for _ in context.count..64 {
            context.commit(context.prepare(44, true).unwrap());
        }
        assert!(matches!(context.prepare(55, true), Err(8)));
        assert_eq!(context.count, 64);
        context.commit(context.prepare(0, false).unwrap());
        assert!(!context.ignore_ctrl_c);
    }
}
