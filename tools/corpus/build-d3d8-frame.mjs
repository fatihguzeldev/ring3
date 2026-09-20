import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { locateTools, root, run, target } from "./shared.mjs";

const output = join(target, "d3d8-frame");
mkdirSync(output, { recursive: true });
const tools = locateTools();
run(tools.clang, ["--target=i686-pc-windows-msvc", "-O0", "-ffreestanding",
  "-fno-stack-protector", "-mno-sse", "-c", join(root, "corpus/d3d8-frame/frame.c"),
  "-o", "frame.obj"], output);
for (const [name, source] of [
  ["d3d8", "d3d8-frame/d3d8.def"], ["user32", "d3d8-frame/user32.def"],
  ["kernel32", "windows-api/kernel32.def"],
]) {
  run(tools.lld, ["-flavor", "link", "/lib", "/machine:x86",
    `/def:${join(root, "corpus", source)}`, `/out:${name}.lib`], output);
}
run(tools.lld, ["-flavor", "link", "/entry:entry", "/subsystem:console",
  "/machine:x86", "/nodefaultlib", "/base:0x400000", "/fixed",
  "/dynamicbase:no", "/nxcompat", "/safeseh:no", "/timestamp:0",
  "/out:frame.exe", "frame.obj", "d3d8.lib", "user32.lib", "kernel32.lib"], output);
console.log(join(output, "frame.exe"));
