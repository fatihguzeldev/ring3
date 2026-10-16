from pathlib import Path
import re
import shutil
import struct

def generate(source, toolchain, work, host_cc, run):
    generated = work / "generated"
    for name in ["compiler", "override", "psdk"]:
        (generated / name).mkdir(parents=True)
    run([host_cc, "-O2", str(source / "sdk/tools/hpp/hpp.c"), "-o", str(work / "hpp")])
    for name in ["winnt", "ntdef", "devioctl"]:
        run([str(work / "hpp"), name + ".template.h", str(generated / "psdk" / (name + ".h"))], cwd=source / "sdk/include/xdk")
    shutil.copytree(toolchain / "lib/clang/23/include", generated / "compiler", dirs_exist_ok=True)
    shutil.copyfile(source / "sdk/include/wine/winternl.h", generated / "override/winternl.h")
    run([str(toolchain / "bin/i686-w64-mingw32-widl"), "--win32", "-h", "-I" + str(source / "sdk/include/psdk"), "-o", str(generated / "override/wtypes.h"), str(source / "sdk/include/psdk/wtypes.idl")])

def compile_offsets(source, toolchain, work, command, run):
    lines = (source / "sdk/include/asm/ks386.template.h").read_text().splitlines()
    names = ["EXCEPTION_RECORD_EXCEPTION_FLAGS", "EXCEPTION_POINTERS_EXCEPTION_RECORD", "EXCEPTION_POINTERS_CONTEXT_RECORD", "TbThreadLocalStoragePointer"]
    selected = [line for line in lines if line.startswith("OFFSET(") and (line.startswith("OFFSET(Jb") or any(name in line for name in names))]
    names = [line[7:line.index(",")] for line in selected]
    body = "#include <intrin.h>\n#include <stddef.h>\n#include <setjmp.h>\n#include <windows.h>\n#include <winternl.h>\n#define OFFSET(name,type,field) offsetof(type,field)\nconst unsigned values[]={\n" + "\n".join(selected) + "\n};\n"
    path = work / "generated/offsets.c"
    path.write_text(body)
    obj = path.with_suffix(".obj")
    run(command + [str(path), "-o", str(obj)])
    data = obj.read_bytes()
    values = None
    for index in range(struct.unpack_from("<H", data, 2)[0]):
        at = 20 + index * 40
        if data[at:at + 8].rstrip(b"\0") == b".rdata":
            raw = struct.unpack_from("<I", data, at + 20)[0]
            values = struct.unpack_from("<" + "I" * len(names), data, raw)
    if values is None:
        raise RuntimeError("compiler-produced offset table is absent")
    constants = dict(zip(names, values))
    (work / "generated/ks386.inc").write_text("\n".join(f"{name} = {value}" for name, value in constants.items()) + "\n")
    assert constants["TbThreadLocalStoragePointer"] == 0x2c
    (work / "generated/tls-array.s").write_text(f".globl __tls_array\n.set __tls_array, {constants['TbThreadLocalStoragePointer']}\n")
    return constants

def vtables(source, work):
    original = (source / "dll/win32/msvcrt/i386/cpp_vtables.s").read_text().split("#if 0")[0]
    rows = [("type_info", "__dummyname_type_info", False)]
    rows += [(name, symbol, True) for name, symbol in re.findall(r"^DEFINE_EXCEPTION_VTABLE\s+(\w+),\s*(\S+)\s*$", original, re.M)]
    assert [name for name, _, _ in rows] == ["type_info", "exception", "bad_typeid", "bad_cast", "__non_rtti_object"]
    lines = ['.section .rdata,"dr"']
    for name, symbol, what in rows:
        lines += [".long _" + name + "_rtti", ".globl _" + name + "_vtable", "_" + name + "_vtable:", '.globl "' + symbol + '"', '"' + symbol + '":', ".long ___thiscall_" + name + "_vector_dtor"]
        if what:
            lines += [".long ___thiscall_exception_what"]
    path = work / "generated/vtables.s"
    path.write_text("\n".join(lines) + "\n")
    return path
