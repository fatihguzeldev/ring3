import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { locateTools, root, run, target } from "./shared.mjs";

const output = join(target, "pe32-calls");
mkdirSync(output, { recursive: true });
const tools = locateTools();
run(tools.clang, ["--target=i686-pc-windows-msvc", "-c",
  join(root, "corpus/pe32-calls/calls.s"), "-o", "calls.obj"], output);
run(tools.lld, ["-flavor", "link", "/entry:entry", "/subsystem:console",
  "/machine:x86", "/nodefaultlib", "/base:0x400000", "/fixed",
  "/dynamicbase:no", "/nxcompat", "/safeseh:no", "/timestamp:0",
  "/out:calls.exe", "calls.obj"], output);
console.log(join(output, "calls.exe"));
