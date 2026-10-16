use std::{
    io::{self, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Mutex, OnceLock},
};

const MAX_DIAGNOSTIC: usize = 4096;
const WORKER_SOURCE: &str = r#"
const fs = require('node:fs');
const header = Buffer.alloc(4);
function receive(bytes, endAllowed = false) {
  let offset = 0;
  while (offset < bytes.length) {
    const count = fs.readSync(0, bytes, offset, bytes.length - offset, null);
    if (count === 0) {
      if (endAllowed && offset === 0) return false;
      throw new Error('truncated validation request');
    }
    offset += count;
  }
  return true;
}
function send(bytes) {
  let offset = 0;
  while (offset < bytes.length) {
    offset += fs.writeSync(1, bytes, offset, bytes.length - offset);
  }
}
while (receive(header, true)) {
  const bytes = Buffer.alloc(header.readUInt32LE(0));
  receive(bytes);
  try {
    if (!WebAssembly.validate(bytes)) throw new Error('WebAssembly.validate rejected module');
    new WebAssembly.Module(bytes);
    send(Buffer.from([0]));
  } catch (error) {
    const diagnostic = Buffer.from(String(error)).subarray(0, 4096);
    const reply = Buffer.alloc(5);
    reply[0] = 1;
    reply.writeUInt32LE(diagnostic.length, 1);
    send(reply);
    send(diagnostic);
  }
}
"#;

struct Validator {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}

impl Validator {
    fn start() -> io::Result<Self> {
        let mut child = Command::new("node")
            .args(["-e", WORKER_SOURCE])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let input = child.stdin.take().expect("piped validator input");
        let output = child.stdout.take().expect("piped validator output");
        Ok(Self {
            child,
            input,
            output,
        })
    }

    fn validate(&mut self, wasm: &[u8]) -> Result<(), String> {
        if let Some(status) = self.child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!("validation worker exited: {status}"));
        }
        let length = u32::try_from(wasm.len()).expect("module request fits u32");
        self.input
            .write_all(&length.to_le_bytes())
            .and_then(|()| self.input.write_all(wasm))
            .map_err(|error| format!("validation request: {error}"))?;
        let mut status = [0];
        self.output
            .read_exact(&mut status)
            .map_err(|error| format!("validation acknowledgement: {error}"))?;
        match status[0] {
            0 => Ok(()),
            1 => {
                let mut length = [0; 4];
                self.output
                    .read_exact(&mut length)
                    .map_err(|error| format!("validation diagnostic length: {error}"))?;
                let length = u32::from_le_bytes(length) as usize;
                if length > MAX_DIAGNOSTIC {
                    return Err("validation diagnostic exceeds protocol bound".to_owned());
                }
                let mut diagnostic = [0; MAX_DIAGNOSTIC];
                self.output
                    .read_exact(&mut diagnostic[..length])
                    .map_err(|error| format!("validation diagnostic: {error}"))?;
                Err(String::from_utf8_lossy(&diagnostic[..length]).into_owned())
            }
            other => Err(format!("invalid validation acknowledgement: {other}")),
        }
    }
}

impl Drop for Validator {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// one worker per native test binary retains every module validation and compilation.
// process exit closes its request pipe, so an idle worker exits without a watcher.
pub(super) fn validate(wasm: &[u8]) {
    static VALIDATOR: OnceLock<Mutex<Validator>> = OnceLock::new();
    let validator = VALIDATOR.get_or_init(|| {
        Mutex::new(Validator::start().expect("start WebAssembly validation worker"))
    });
    let result = validator
        .lock()
        .expect("WebAssembly validation worker lock")
        .validate(wasm);
    if let Err(error) = result {
        panic!("{error}");
    }
}

#[test]
fn validator_rejects_a_malformed_frame_then_validates_the_next_module() {
    let mut validator = Validator::start().unwrap();
    let module = b"\0asm\x01\0\0\0";
    validator.validate(module).unwrap();
    assert!(validator.validate(b"invalid wasm").is_err());
    validator.validate(module).unwrap();
}
