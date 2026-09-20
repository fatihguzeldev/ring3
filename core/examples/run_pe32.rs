use std::io::{Read, Write};

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).peekable();
    let diagnostic = args.peek().is_some_and(|value| value == "--diagnostic");
    if diagnostic {
        args.next();
    }
    let path = args
        .next()
        .ok_or("usage: run_pe32 [--diagnostic] <file.exe> [work-limit] [frame.ppm]")?;
    let limit = args
        .next()
        .map_or(Ok(1_000_000), |value| value.parse::<u64>())?;
    let frame_path = args.next();
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
    let mut process = if diagnostic {
        eprintln!(
            "diagnostic mode: unknown imports are stop addresses; missing dlls are not initialized"
        );
        Process32::load_diagnostic(&bytes, 16_384)
    } else {
        Process32::load(&bytes, 16_384)
    }
    .map_err(|error| format!("load failed: {error:?}"))?;
    let result = process.run(limit);
    println!(
        "{:?}; eip={:#010x}; instructions={}; api_calls={}; eax={:#010x}; eflags={:#010x}",
        result.reason,
        process.cpu.eip,
        result.instructions,
        result.api_calls,
        process.cpu.register(Register32::Eax),
        process.cpu.eflags
    );
    if let Some(path) = frame_path {
        let frame = process
            .take_frame()
            .ok_or("guest has not presented a frame")?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        let mut output = std::io::BufWriter::new(file);
        writeln!(output, "P6\n{} {}\n255", frame.width, frame.height)?;
        for pixel in frame.rgba.chunks_exact(4) {
            output.write_all(&pixel[..3])?;
        }
        output.flush()?;
        println!("exported {}x{} guest frame", frame.width, frame.height);
    }
    if !matches!(
        result.reason,
        ProcessStop::Exited(_) | ProcessStop::Stopped(StopReason::Breakpoint)
    ) {
        let mut context = Vec::new();
        for offset in 0..16 {
            let Some(address) = process.cpu.eip.checked_add(offset) else {
                break;
            };
            let mut byte = [0];
            if process.memory.fetch(u64::from(address), &mut byte).is_err() {
                break;
            }
            context.push(byte[0]);
        }
        println!("instruction_bytes={context:02x?}");
        return Err("guest stopped before exiting or reaching a breakpoint".into());
    }
    Ok(())
}
