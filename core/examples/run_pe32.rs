use std::io::{Read, Write};

use ring3_core::execution::{
    GuestModule, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

fn read_input(path: &str, limit: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err("input exceeds byte limit".into());
    }
    Ok(bytes)
}

fn basename(path: &str) -> Result<&str, Box<dyn std::error::Error>> {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "input has no usable basename".into())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).peekable();
    let mut diagnostic = false;
    let mut libraries = Vec::new();
    let mut remaining = 128 * 1024 * 1024;
    while let Some(option) = args.peek() {
        match option.as_str() {
            "--diagnostic" => {
                diagnostic = true;
                args.next();
            }
            "--dll" => {
                args.next();
                let path = args.next().ok_or("--dll requires a path")?;
                if libraries.len() == 16 {
                    return Err("at most 16 DLL inputs are supported".into());
                }
                let bytes = read_input(&path, remaining)?;
                remaining -= bytes.len();
                libraries.push((basename(&path)?.to_owned(), bytes));
            }
            _ => break,
        }
    }
    let path = args.next().ok_or(
        "usage: run_pe32 [--diagnostic] [--dll file.dll]... <file.exe> [work-limit] [frame.ppm]",
    )?;
    let limit = args
        .next()
        .map_or(Ok(1_000_000), |value| value.parse::<u64>())?;
    let frame_path = args.next();
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let bytes = read_input(&path, 64 * 1024 * 1024)?;
    let name = basename(&path)?;
    if name.contains('"') {
        return Err("input basename cannot contain a Windows command-line quote".into());
    }
    let image_path = format!("C:\\{name}");
    let command_line = format!("\"{name}\"");
    let modules: Vec<_> = libraries
        .iter()
        .map(|(name, bytes)| GuestModule { name, bytes })
        .collect();
    if diagnostic {
        eprintln!(
            "diagnostic mode: unknown imports are stop addresses; missing dlls are not initialized"
        );
    }
    let mut process = Process32::load_with_options(
        &bytes,
        16_384,
        ProcessOptions {
            image_path: image_path.as_bytes(),
            command_line: command_line.as_bytes(),
            diagnostic_imports: diagnostic,
            modules: &modules,
            ..ProcessOptions::default()
        },
    )
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
        export_frame(&mut process, &path)?;
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

fn export_frame(process: &mut Process32, path: &str) -> Result<(), Box<dyn std::error::Error>> {
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
    Ok(())
}
