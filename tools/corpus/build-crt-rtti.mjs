import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { locateTools, root, run, target } from "./shared.mjs";

const output = join(target, "crt-rtti");
mkdirSync(output, { recursive: true });
const tools = locateTools();
for (const [module, definition] of [
  ["kernel32", "corpus/windows-api/kernel32.def"],
  ["msvcrt", "corpus/crt-rtti/msvcrt.def"],
]) {
  run(tools.lld, ["-flavor", "link", "/lib", "/machine:x86",
    `/def:${join(root, definition)}`, `/out:${module}.lib`], output);
}
run(tools.clang, ["--target=i686-pc-windows-msvc", "-O0", "-ffreestanding",
  "-fno-stack-protector", "-fno-exceptions", "-c",
  join(root, "corpus/crt-rtti/casts.cpp"), "-o", "casts.obj"], output);
run(tools.lld, ["-flavor", "link", "/entry:entry", "/subsystem:console",
  "/machine:x86", "/nodefaultlib", "/base:0x400000", "/fixed",
  "/dynamicbase:no", "/nxcompat", "/safeseh:no", "/timestamp:0",
  "/out:casts.exe", "casts.obj", "kernel32.lib", "msvcrt.lib"], output);
console.log(join(output, "casts.exe"));
