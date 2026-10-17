use super::*;
impl EngineInstance {
    pub(super) fn execute_process_atom_call(
        &mut self,
        api: ProcessApi32,
        args: &[u32],
    ) -> Result<ProcessReply32, HostError> {
        use ProcessApi32::*;
        let error = |code| ProcessReply32 {
            value: 0,
            error: Some(code),
        };
        if api == GlobalDeleteAtom {
            return Ok(match self.process_runtime.atoms.delete(args[0] as u16) {
                Ok(()) => 0.into(),
                Err(code) => error(code),
            });
        }
        let cp = self.windows_ansi_code_page();
        if api == GlobalGetAtomNameA {
            let atom = args[0] as u16;
            let name = if (1..0xc000).contains(&atom) {
                format!("#{atom}")
            } else if let Some(name) = self.process_runtime.atoms.name(atom) {
                name.to_owned()
            } else {
                return Ok(error(6));
            };
            if args[2] as i32 <= 0 {
                return Ok(error(234));
            }
            let mut bytes = buffers::encoded(&name, false, cp);
            let truncated = bytes.len() >= args[2] as usize;
            bytes.truncate((args[2] - 1) as usize);
            let count = bytes.len() as u32;
            bytes.push(0);
            buffers::write(self.memory.as_mut().unwrap(), args[1], &bytes).map_err(guest)?;
            return Ok(if truncated { error(234) } else { count.into() });
        }
        if args[0] <= 0xffff {
            return Ok(if (1..0xc000).contains(&args[0]) {
                args[0].into()
            } else {
                error(87)
            });
        }
        let name = buffers::string(self.memory()?, args[0], false, cp).map_err(guest)?;
        if name.is_empty() || buffers::encoded(&name, false, cp).len() > 255 {
            return Ok(error(87));
        }
        if let Some(number) = name
            .strip_prefix('#')
            .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        {
            return Ok(match number.parse::<u32>() {
                Ok(value) if (1..0xc000).contains(&value) => value.into(),
                _ => error(87),
            });
        }
        Ok(if api == GlobalFindAtomA {
            self.process_runtime
                .atoms
                .find(&name)
                .map_or_else(|| error(2), |atom| u32::from(atom).into())
        } else {
            self.process_runtime
                .atoms
                .add(name)
                .map_or_else(error, |atom| u32::from(atom).into())
        })
    }
}
