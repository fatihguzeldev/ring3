import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tarfile

import configuration as config
import headers
import linking

def digest(path):
    hash = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1048576), b""):
            hash.update(chunk)
    return hash.hexdigest()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--reactos-archive", type=Path, required=True)
    parser.add_argument("--toolchain-archive", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--host-cc", default="clang")
    args = parser.parse_args()
    assert digest(args.reactos_archive) == config.SOURCE_SHA, "ReactOS source archive hash"
    assert digest(args.toolchain_archive) == config.TOOLCHAIN_SHA, "LLVM-MinGW archive hash"
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    for archive in [args.reactos_archive, args.toolchain_archive]:
        with tarfile.open(archive) as file:
            file.extractall(output, filter="data")
    source = output / ("reactos-" + config.COMMIT)
    toolchain = output / config.TOOLCHAIN
    work = output / "build"
    work.mkdir()
    here = Path(__file__).resolve().parent
    repository = here.parents[5]
    inspection_path = repository / "engine/tests/support/pe32/inspection.py"
    spec = importlib.util.spec_from_file_location("pe_inspection", inspection_path)
    inspection = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(inspection)
    commands = []
    def run(command, cwd=None):
        commands.append({"argv": command, "cwd": str(cwd or work)})
        result = subprocess.run(command, cwd=cwd or work, capture_output=True, text=True)
        (work / ("command-" + str(len(commands)) + ".log")).write_text(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError("command failed: " + " ".join(command) + "\n" + result.stdout + result.stderr)
    def compile(command, path, name):
        obj = work / "obj" / (name.replace("/", "_") + ".obj")
        obj.parent.mkdir(exist_ok=True)
        run(command + [str(path), "-o", str(obj)])
        return obj
    headers.generate(source, toolchain, work, args.host_cc, run)
    crt, public, startup_command, authored_command = config.commands(source, toolchain, work)
    constants = headers.compile_offsets(source, toolchain, work, crt, run)
    original = ["dll/win32/msvcrt/" + name for name in config.CRT_UNITS] + config.HELPER_UNITS + config.ASM_UNITS + ["sdk/lib/pseh/i386/seh.s", "sdk/lib/3rdparty/libwine/debug_ros.c"] + ["sdk/lib/crt/startup/" + name for name in config.STARTUP_UNITS]
    source_hashes = {name: digest(source / name) for name in original}
    objects = [compile(crt, source / "dll/win32/msvcrt" / name, name) for name in config.CRT_UNITS]
    helper_objects = {name: compile(crt, source / name, name) for name in config.HELPER_UNITS}
    objects += list(helper_objects.values())
    objects += [compile(public + ["-D_WINE", "-D__WINESRC__", "-include", "stdarg.h"], source / "sdk/lib/3rdparty/libwine/debug_ros.c", "debug_ros")]
    asm = [str(toolchain / "bin/clang"), "-target", "i686-w64-windows-gnu", "-c", "-x", "assembler-with-cpp", "-D_M_IX86", "-I" + str(work / "generated"), "-I" + str(source / "sdk/include/asm")]
    asm_objects = {name: compile(asm, source / name, name) for name in config.ASM_UNITS}
    objects += list(asm_objects.values())
    # llvm's coff assembler rejects masm's inferred PUSH-immediate relocation.
    seh = (source / "sdk/lib/pseh/i386/seh.s").read_text()
    assert seh.count("push offset _unwind_handler") == 1
    adapted = work / "generated/seh.s"
    adapted.write_text(seh.replace("push offset _unwind_handler", ".byte 0x68\n    .long _unwind_handler"))
    objects += [compile(asm, adapted, "seh"), compile(asm, headers.vtables(source, work), "vtables")]
    for name in ["msvcrt.def", "hosted-dll.def"]:
        shutil.copyfile(here / name, work / name)
    linking.link_crt(objects, work, toolchain, run)
    startup = {}
    (work / "generated/pseudo-image-base.h").write_text("extern char __ImageBase;\n")
    for name in config.STARTUP_UNITS:
        flags = []
        if name == "crtdll.c":
            flags += ["-DCRTDLL"]
        if name == "mscmain.c":
            flags += ["-D_pei386_runtime_relocator=_unused_msvc_runtime_relocator"]
        if name == "pseudo-reloc.c":
            flags += ["-D_image_base__=__ImageBase", "-include", str(work / "generated/pseudo-image-base.h")]
        startup[name] = compile(startup_command + flags, source / "sdk/lib/crt/startup" / name, "startup_" + name)
    startup["tls-array"] = compile(asm, work / "generated/tls-array.s", "tls_array")
    startup["chkstk"] = asm_objects["sdk/lib/crt/except/i386/chkstk_asm.s"]
    for name in ["hosted", "hosted-dll"]:
        obj = compile(authored_command, here / (name + ".cpp"), name)
        linking.link_hosted(name, startup, obj, helper_objects["sdk/lib/vcruntime/__std_terminate.c"], work, toolchain, run)
    artifacts = output / "artifacts"
    artifacts.mkdir()
    inventory = {}
    for name in ["msvcrt.dll", "hosted.exe", "hosted-dll.dll"]:
        shutil.copyfile(work / name, artifacts / name)
        inventory[name] = inspection.inspect(artifacts / name)
    assert inventory["hosted.exe"]["tlsDirectory"] != 0
    assert inventory["hosted-dll.dll"]["tlsDirectory"] != 0
    for name in ["hosted.exe", "hosted-dll.dll"]:
        assert set(inventory[name]["imports"]["msvcrt.dll"]) <= set(inventory["msvcrt.dll"]["exports"])
    receipt = {"sourceCommit": config.COMMIT, "sourceUrl": "https://codeload.github.com/reactos/reactos/tar.gz/" + config.COMMIT, "toolchainUrl": "https://github.com/mstorsjo/llvm-mingw/releases/download/20261006/" + config.TOOLCHAIN + ".tar.xz", "licenses": ["ReactOS original CRT files retain LGPL/GPL notices in pinned source", "MinGW hosted startup is public domain", "selected vcruntime helpers are MIT or CC0 as declared by their original source", "LLVM-MinGW distribution retains its bundled licenses"], "producerDirectory": str(here.relative_to(repository)), "inspectionSource": str(inspection_path.relative_to(repository)), "inspectionSha256": digest(inspection_path), "producerSources": {path.name: digest(path) for path in here.iterdir() if path.suffix in [".py", ".cpp", ".def"]}, "sourceArchiveSha256": config.SOURCE_SHA, "toolchainArchiveSha256": config.TOOLCHAIN_SHA, "toolchain": config.TOOLCHAIN, "compiler": subprocess.check_output([str(toolchain / "bin/clang"), "--version"], text=True), "sourceUnits": source_hashes, "authoredSources": {name: digest(here / name) for name in ["hosted.cpp", "hosted-dll.cpp"]}, "offsets": constants, "artifacts": inventory, "expectedFile": "C:\\Temp\\hosted.txt", "expectedBytes": "MDLCaTYbJEFAGX", "expectedExit": 0, "threadOracle": {"childReturn": 42, "primaryTls": 71, "childTls": 23, "dllPrimaryTls": 29, "dllChildTls": 41, "attach": "a", "allocationIo": "T", "flsDestructor": "Y", "detach": "b", "joined": "J"}, "profile": {"crt": "unmodified ReactOS CRT source with function-section GC and explicit public export selection", "startup": "original ReactOS MinGW hosted EXE/DLL startup with MSVC compiler EH and original pseudo-relocator", "compatibility": ["compiler-generated XDK headers and i386 field offsets", "original vtable macro metadata emitted in GAS syntax", "one PUSH relocation emitted with exact32-bit opcode", "exact COFF C++/stdcall import metadata", "compiler TLS offset from original TEB layout"], "limits": ["no proprietary DLL input", "no CRT stub imports", "no whole-CRT or arbitrary-C++ compatibility claim", "no browser SDK claim"]}}
    (artifacts / "provenance.json").write_text(json.dumps(receipt, indent=2) + "\n")
    (work / "commands.json").write_text(json.dumps(commands, indent=2) + "\n")
    print(json.dumps({"artifacts": str(artifacts), "hashes": {name: value["sha256"] for name, value in inventory.items()}}))

if __name__ == "__main__":
    main()
