# ring3

Windows games, in your browser.

Ring3 is a browser-native runtime for Windows games. The goal is to run existing
PC games directly on your device, without installing Windows or streaming from
a remote machine.

A Rust and WebAssembly core executes game code and provides Windows compatibility.
Graphics, sound, and controls connect to WebGPU, Web Audio, and browser input APIs.

Bring your own game files. Ring3 provides the runtime.
