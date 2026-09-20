import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { locateTools, root, run, target } from "./shared.mjs";

const output = join(target, "guest-dll");
mkdirSync(output, { recursive: true });
const tools = locateTools();
for (const name of ["demo", "caller", "delayed"]) {
  run(tools.clang, ["--target=i686-pc-windows-msvc", "-O0", "-ffreestanding",
    "-fno-stack-protector", "-c", join(root, `corpus/guest-dll/${name}.c`),
    "-o", `${name}.obj`], output);
}
const flags = ["-flavor", "link", "/subsystem:console", "/machine:x86",
  "/nodefaultlib", "/fixed", "/dynamicbase:no", "/nxcompat", "/safeseh:no", "/timestamp:0"];
run(tools.lld, [...flags, "/dll", "/entry:attach@12", "/base:0x50000000",
  `/def:${join(root, "corpus/guest-dll/demo.def")}`, "/out:demo.dll", "/implib:demo.lib", "demo.obj"], output);
run(tools.lld, ["-flavor", "link", "/lib", "/machine:x86",
  `/def:${join(root, "corpus/windows-api/kernel32.def")}`, "/out:kernel32.lib"], output);
run(tools.lld, [...flags, "/entry:entry", "/base:0x400000", "/out:caller.exe",
  "caller.obj", "demo.lib", "kernel32.lib"], output);
run(tools.lld, [...flags, "/entry:entry", "/base:0x400000", "/out:delayed.exe",
  "/delayload:demo.dll", "delayed.obj", "demo.lib", "kernel32.lib"], output);
console.log(join(output, "caller.exe"));
console.log(join(output, "demo.dll"));
console.log(join(output, "delayed.exe"));
