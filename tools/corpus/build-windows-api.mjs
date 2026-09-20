import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { locateTools, root, run, target } from "./shared.mjs";

const output = join(target, "windows-api");
mkdirSync(output, { recursive: true });
const tools = locateTools();
run(tools.lld, ["-flavor", "link", "/lib", "/machine:x86",
  `/def:${join(root, "corpus/windows-api/kernel32.def")}`, "/out:kernel32.lib"], output);
for (const name of ["calls", "modules"]) {
  run(tools.clang, ["--target=i686-pc-windows-msvc", "-O0", "-ffreestanding",
    "-fno-stack-protector", "-c", join(root, `corpus/windows-api/${name}.c`),
    "-o", `${name}.obj`], output);
  run(tools.lld, ["-flavor", "link", "/entry:entry", "/subsystem:console",
    "/machine:x86", "/nodefaultlib", "/base:0x400000", "/fixed",
    "/dynamicbase:no", "/nxcompat", "/safeseh:no", "/timestamp:0",
    `/out:${name}.exe`, `${name}.obj`, "kernel32.lib"], output);
  console.log(join(output, `${name}.exe`));
}
