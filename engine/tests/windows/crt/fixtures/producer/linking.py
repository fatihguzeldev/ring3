import struct
from pathlib import Path

def import_library(work, toolchain, run):
    # short COFF imports need exact C++ names and the actual stdcall public symbol.
    path = work / "msvcrt.lib"
    data = bytearray(path.read_bytes())
    at = 8
    while at < len(data):
        size = int(data[at + 48:at + 58])
        start = at + 60
        member = data[start:start + size]
        if member[:4] == b"\0\0\xff\xff" and member[20:].split(b"\0")[0].startswith(b"??"):
            kind = struct.unpack_from("<H", member, 18)[0] & 3
            struct.pack_into("<H", data, start + 18, (1 << 2) | kind)
        at += 60 + size + (size & 1)
    path.write_bytes(data)
    names = b"__CxxThrowException@8\0msvcrt.dll\0_CxxThrowException\0"
    member = struct.pack("<HHHHIIHH", 0, 0xffff, 0, 0x14c, 0, len(names), 0, 4 << 2) + names
    obj = work / "generated/cxxthrow-import.obj"
    obj.write_bytes(member)
    run([str(toolchain / "bin/llvm-lib"), "/out:" + str(work / "msvcrt-hosted.lib"), str(path), str(obj)])

def link_crt(objects, work, toolchain, run):
    libraries = [toolchain / "i686-w64-mingw32/lib" / ("lib" + name + ".a") for name in ["kernel32", "ntdll", "user32", "advapi32", "mingwex"]]
    libraries += [toolchain / "lib/clang/23/lib/windows/libclang_rt.builtins-i386.a"]
    run([str(toolchain / "bin/lld-link"), "/dll", "/entry:DllMain@12", "/opt:ref", "/opt:icf", "/nodefaultlib", "/machine:x86", "/base:0x60000000", "/safeseh:no", "/timestamp:0", "/def:" + str(work / "msvcrt.def"), "/out:" + str(work / "msvcrt.dll"), "/implib:" + str(work / "msvcrt.lib"), "/errorlimit:0"] + list(map(str, objects + libraries)))
    import_library(work, toolchain, run)

def link_hosted(name, startup, authored, helper, work, toolchain, run):
    objects = [authored, helper] + [obj for key, obj in startup.items() if (key != "crtexe.c" if name == "hosted-dll" else key not in ["crtdll.c", "dllentry.c"])]
    libraries = [work / "msvcrt-hosted.lib", toolchain / "i686-w64-mingw32/lib/libkernel32.a", toolchain / "lib/clang/23/lib/windows/libclang_rt.builtins-i386.a"]
    args = [str(toolchain / "bin/lld-link"), "/machine:x86", "/opt:ref", "/opt:icf", "/nodefaultlib", "/safeseh:no", "/lldmingw", "/timestamp:0", "/errorlimit:0", "/map:" + str(work / (name + ".map"))]
    if name == "hosted-dll":
        args += ["/dll", "/entry:DllMainCRTStartup@12", "/base:0x500000", "/def:" + str(work / "hosted-dll.def"), "/out:" + str(work / "hosted-dll.dll")]
    else:
        args += ["/entry:mainCRTStartup", "/subsystem:console", "/out:" + str(work / "hosted.exe")]
    run(args + list(map(str, objects + libraries)))
