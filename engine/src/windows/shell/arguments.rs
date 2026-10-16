use super::ShellError32 as E;

pub(crate) fn command_line_arguments(
    command: &[u16],
    executable: &[u16],
) -> Result<Vec<Vec<u16>>, E> {
    if command.len() > 32767 || executable.len() > 32767 {
        return Err(E::Capacity);
    }
    if command.is_empty() {
        return Ok(vec![executable.to_vec()]);
    }
    let mut args = Vec::new();
    args.try_reserve(command.len() / 2 + 1)
        .map_err(|_| E::Allocation)?;
    let mut at = 0;
    let whitespace = |c: u16| c == 32 || c == 9;
    if whitespace(command[0]) {
        args.push(Vec::new());
    } else {
        let mut program = Vec::new();
        program
            .try_reserve(command.len())
            .map_err(|_| E::Allocation)?;
        if command[0] == 34 {
            at = 1;
            while at < command.len() && command[at] != 34 {
                program.push(command[at]);
                at += 1;
            }
            if at < command.len() {
                at += 1;
            }
        } else {
            while at < command.len() && !whitespace(command[at]) {
                program.push(command[at]);
                at += 1;
            }
        }
        args.push(program);
    }
    while at < command.len() {
        while at < command.len() && whitespace(command[at]) {
            at += 1;
        }
        if at == command.len() {
            break;
        }
        let mut arg = Vec::new();
        arg.try_reserve((command.len() - at).min(64))
            .map_err(|_| E::Allocation)?;
        let mut quoted = false;
        while at < command.len() && (quoted || !whitespace(command[at])) {
            let start = at;
            while at < command.len() && command[at] == 92 {
                at += 1;
            }
            let slashes = at - start;
            arg.try_reserve(slashes + 1).map_err(|_| E::Allocation)?;
            if at < command.len() && command[at] == 34 {
                arg.extend(std::iter::repeat_n(92, slashes / 2));
                if slashes % 2 == 1 {
                    arg.push(34);
                    at += 1;
                } else {
                    at += 1;
                    if quoted && command.get(at) == Some(&34) {
                        arg.push(34);
                        at += 1;
                    } else {
                        quoted = !quoted;
                    }
                }
            } else {
                arg.extend(std::iter::repeat_n(92, slashes));
                if at < command.len() && (quoted || !whitespace(command[at])) {
                    arg.push(command[at]);
                    at += 1;
                }
            }
        }
        args.push(arg);
    }
    Ok(args)
}
