use std::io::{Read, Write};

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: run_pe32 <file.exe> [work-limit] [frame.ppm]")?;
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
    let mut process =
        Process32::load(&bytes, 16_384).map_err(|error| format!("load failed: {error:?}"))?;
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
        return Err("guest stopped before exiting or reaching a breakpoint".into());
    }
    Ok(())
}
