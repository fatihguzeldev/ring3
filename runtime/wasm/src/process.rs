use std::time::Duration;

use ring3_core::execution::{
    FileContents, FileContentsMode, FileMetadata, Frame, GuestModule, PostedMessage, Process32,
    ProcessOptions, ProcessStop, StopReason,
};
use serde_json::{Value, json};

pub const MAX_INPUT: usize = 128 * 1024 * 1024 + 8;

struct File {
    path: String,
    size: u64,
    role: String,
    contents: Vec<u8>,
}

#[derive(Default)]
pub struct Session {
    files: Vec<File>,
    process: Option<Process32>,
    pub frame: Option<Frame>,
    reason: Option<ProcessStop>,
    instructions: u64,
    api_calls: u64,
}

fn integer(value: &Value, name: &str) -> Result<u64, String> {
    value[name]
        .as_u64()
        .ok_or_else(|| format!("invalid {name}"))
}

fn word(value: &Value, name: &str) -> Result<u32, String> {
    u32::try_from(integer(value, name)?).map_err(|_| format!("invalid {name}"))
}

fn json_input(input: &[u8]) -> Result<Value, String> {
    if input.len() > 16 * 1024 * 1024 {
        return Err("control input exceeds limit".into());
    }
    serde_json::from_slice(input).map_err(|error| error.to_string())
}

impl Session {
    pub fn command(
        &mut self,
        operation: u32,
        argument: u32,
        input: Vec<u8>,
    ) -> Result<Value, String> {
        match operation {
            0 => self.configure(&json_input(&input)?)?,
            1 => self.content(argument, input)?,
            2 => self.start()?,
            3 => self.run(argument, integer(&json_input(&input)?, "elapsedMs")?)?,
            4 => self.supply(input)?,
            5 => self.post(&json_input(&input)?)?,
            6 => (),
            _ => return Err("unknown operation".into()),
        }
        Ok(self.snapshot())
    }

    fn configure(&mut self, value: &Value) -> Result<(), String> {
        if self.process.is_some() {
            return Err("a process is already loaded".into());
        }
        let entries = value["files"].as_array().ok_or("missing files")?;
        if entries.is_empty() || entries.len() > 100_000 {
            return Err("file count exceeds limit".into());
        }
        let mut paths = std::collections::BTreeSet::new();
        let mut files = Vec::with_capacity(entries.len());
        for entry in entries {
            let path = entry["path"].as_str().ok_or("missing path")?;
            let role = entry["role"].as_str().ok_or("missing file role")?;
            if path.len() > 32767
                || !path.is_ascii()
                || path.contains('\0')
                || !paths.insert(path.to_ascii_lowercase())
                || !matches!(role, "executable" | "module" | "deferred" | "data")
            {
                return Err("invalid or duplicate file declaration".into());
            }
            files.push(File {
                path: path.into(),
                size: integer(entry, "size")?,
                role: role.into(),
                contents: Vec::new(),
            });
        }
        if files
            .iter()
            .filter(|file| file.role == "executable")
            .count()
            != 1
        {
            return Err("exactly one executable is required".into());
        }
        self.files = files;
        Ok(())
    }

    fn content(&mut self, index: u32, input: Vec<u8>) -> Result<(), String> {
        if self.process.is_some() {
            return Err("process already loaded".into());
        }
        let loaded: usize = self.files.iter().map(|file| file.contents.len()).sum();
        let file = self
            .files
            .get_mut(index as usize)
            .ok_or("invalid file index")?;
        if file.role == "data"
            || input.len() as u64 != file.size
            || input.len() > MAX_INPUT - 8
            || loaded - file.contents.len() + input.len() > 256 * 1024 * 1024
        {
            return Err("invalid module contents or limit exceeded".into());
        }
        file.contents = input;
        Ok(())
    }

    fn start(&mut self) -> Result<(), String> {
        if self.process.is_some() {
            return Err("process already loaded".into());
        }
        let executable = self
            .files
            .iter()
            .find(|file| file.role == "executable")
            .ok_or("no executable")?;
        if self.files.iter().any(|file| {
            file.role != "data"
                && (file.contents.is_empty() || file.contents.len() as u64 != file.size)
        }) {
            return Err("module contents have not been supplied".into());
        }
        let name = executable
            .path
            .rsplit('\\')
            .next()
            .ok_or("invalid executable path")?;
        let command_line = format!("\"{name}\"");
        let files: Vec<_> = self
            .files
            .iter()
            .map(|file| FileMetadata {
                path: file.path.as_bytes(),
                size: file.size,
            })
            .collect();
        let contents: Vec<_> = self
            .files
            .iter()
            .filter(|file| file.role != "data")
            .map(|file| FileContents {
                path: file.path.as_bytes(),
                bytes: &file.contents,
            })
            .collect();
        let modules = self.modules("module");
        let deferred = self.modules("deferred");
        let process = Process32::load_with_options(
            &executable.contents,
            32768,
            ProcessOptions {
                image_path: executable.path.as_bytes(),
                command_line: command_line.as_bytes(),
                files: &files,
                file_contents: &contents,
                modules: &modules,
                deferred_modules: &deferred,
                file_contents_mode: FileContentsMode::OnDemand {
                    cache_bytes: 1024 * 1024 * 1024,
                },
                diagnostic_imports: true,
                ..ProcessOptions::default()
            },
        )
        .map_err(|error| format!("load failed: {error:?}"))?;
        self.process = Some(process);
        self.reason = Some(ProcessStop::Stopped(StopReason::InstructionLimit));
        for file in &mut self.files {
            file.contents = Vec::new();
        }
        Ok(())
    }

    fn modules(&self, role: &str) -> Vec<GuestModule<'_>> {
        self.files
            .iter()
            .filter(|file| file.role == role)
            .map(|file| GuestModule {
                name: file
                    .path
                    .rsplit('\\')
                    .next()
                    .expect("split always has one item"),
                bytes: &file.contents,
            })
            .collect()
    }

    fn process(&mut self) -> Result<&mut Process32, String> {
        self.process
            .as_mut()
            .ok_or_else(|| "no loaded process".into())
    }

    fn run(&mut self, budget: u32, elapsed_ms: u64) -> Result<(), String> {
        if budget == 0 || budget > 100_000 {
            return Err("invalid execution budget".into());
        }
        let process = self.process()?;
        process
            .set_elapsed_time(Duration::from_millis(elapsed_ms))
            .map_err(|error| format!("clock: {error:?}"))?;
        let result = process.run(u64::from(budget));
        self.frame = process.take_frame();
        self.instructions += result.instructions;
        self.api_calls += result.api_calls;
        self.reason = Some(result.reason);
        Ok(())
    }

    fn supply(&mut self, mut input: Vec<u8>) -> Result<(), String> {
        let token = input.get(..8).ok_or("missing request token")?;
        let token = u64::from_le_bytes(token.try_into().expect("eight bytes"));
        let process = self.process()?;
        input.drain(..8);
        let bytes = input.into_boxed_slice();
        process
            .supply_file_contents(token, bytes)
            .map_err(|error| format!("file supply: {error:?}"))?;
        self.reason = Some(ProcessStop::Stopped(StopReason::InstructionLimit));
        Ok(())
    }

    fn post(&mut self, value: &Value) -> Result<(), String> {
        let message = PostedMessage {
            hwnd: word(value, "hwnd")?,
            message: word(value, "message")?,
            wparam: word(value, "wparam")?,
            lparam: word(value, "lparam")?,
            time: word(value, "time")?,
            point: [0, 0],
        };
        self.process()?
            .post_message(message)
            .map_err(|error| format!("message: {error:?}"))?;
        if self.reason == Some(ProcessStop::WaitingForMessage) {
            self.reason = Some(ProcessStop::Stopped(StopReason::InstructionLimit));
        }
        Ok(())
    }

    fn snapshot(&self) -> Value {
        let state = match self.reason.as_ref() {
            None => "ready",
            Some(ProcessStop::Stopped(StopReason::InstructionLimit)) => "running",
            Some(ProcessStop::WaitingForMessage | ProcessStop::WaitingForSynchronization) => {
                "waiting"
            }
            Some(ProcessStop::FileContentsRequired) => "file",
            Some(ProcessStop::Exited(_)) => "exited",
            Some(_) => "stopped",
        };
        let pending = self.process.as_ref().and_then(Process32::pending_file_contents).map(|file| json!({
            "id": file.id.to_string(), "path": String::from_utf8_lossy(file.path), "size": file.size
        }));
        let windows: Vec<_> = self.process.as_ref().map(Process32::window_snapshots).unwrap_or_default().iter().map(|window| json!({
            "hwnd":window.hwnd, "parent":window.parent, "id":window.id, "class":window.class,
            "style":window.style, "title":window.title, "active":window.active
        })).collect();
        json!({"state":state, "reason":format!("{:?}",self.reason),
            "eip":self.process.as_ref().map_or(0, |process|process.cpu.eip),
            "instructions":self.instructions.to_string(), "apiCalls":self.api_calls.to_string(),
            "pending":pending,"windows":windows,
            "frame":self.frame.as_ref().map(|frame|json!({"width":frame.width,"height":frame.height}))})
    }
}
