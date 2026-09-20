use std::io::Read;

use ring3_core::execution::{Cpu32, Permissions, Register32, StopReason, load_pe32};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: run_pe32 <file.exe> [instruction-limit]")?;
    let limit = args
        .next()
        .map_or(Ok(1_000_000), |value| value.parse::<u64>())?;
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("input exceeds 64 MiB".into());
    }
    let mut image = load_pe32(&bytes, 16_384).map_err(|error| format!("load failed: {error:?}"))?;
    image
        .memory
        .map_zeroed(0x1000_0000, 64 * 1024, Permissions::READ_WRITE)
        .map_err(|error| format!("stack mapping failed: {error:?}"))?;
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Esp, 0x1001_0000);
    let result = cpu.run(&mut image.memory, limit);
    println!(
        "{:?} at {:#010x}; instructions={}; eax={:#010x}; eflags={:#010x}",
        result.reason,
        result.instruction_pointer,
        result.instructions,
        cpu.register(Register32::Eax),
        cpu.eflags
    );
    if result.reason != StopReason::Breakpoint {
        return Err("guest stopped before reaching a breakpoint".into());
    }
    Ok(())
}
