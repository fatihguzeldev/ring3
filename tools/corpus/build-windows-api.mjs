import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { locateTools, root, run, target } from "./shared.mjs";

const output = join(target, "windows-api");
mkdirSync(output, { recursive: true });
const tools = locateTools();
for (const library of ["kernel32", "user32", "gdi32"]) {
  run(tools.lld, ["-flavor", "link", "/lib", "/machine:x86",
    `/def:${join(root, `corpus/windows-api/${library}.def`)}`, `/out:${library}.lib`], output);
}
for (const name of ["calls", "modules", "heap", "version", "critical-sections", "tls", "global-memory", "code-pages", "cpinfo", "messages", "clipboard-formats", "process-version", "metrics", "gdi", "colors", "brushes", "cursors", "cursor-position", "local-realloc", "thread-identity", "module-file-name", "resources"]) {
  run(tools.clang, ["--target=i686-pc-windows-msvc", "-O0", "-ffreestanding",
    "-fno-stack-protector", "-c", join(root, `corpus/windows-api/${name}.c`),
    "-o", `${name}.obj`], output);
  const extra = [];
  if (name === "resources") {
    run(tools.clang, ["--target=i686-pc-windows-msvc", "-c",
      join(root, "corpus/windows-api/resources.s"), "-o", "resource-data.obj"], output);
    extra.push("resource-data.obj");
  }
  run(tools.lld, ["-flavor", "link", "/entry:entry",
    name === "process-version" ? "/subsystem:console,5.01" : "/subsystem:console",
    "/machine:x86", "/nodefaultlib", "/base:0x400000", "/fixed",
    "/dynamicbase:no", "/nxcompat", "/safeseh:no", "/timestamp:0",
    `/out:${name}.exe`, `${name}.obj`, ...extra, "kernel32.lib", "user32.lib", "gdi32.lib"], output);
  console.log(join(output, `${name}.exe`));
}
