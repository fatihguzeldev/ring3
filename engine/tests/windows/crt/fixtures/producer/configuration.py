from pathlib import Path

COMMIT = "51bfc7037359b594876a349b46b992e363e79838"
SOURCE_SHA = "6072f07007da8a2f232f8378c67e9bdee115beb1e2e8212d375dc2f4b1d57354"
TOOLCHAIN_SHA = "a7a19b11048e89372637d26279cdb7c430ed3cf8c1e224310421312bf986ebf6"
TOOLCHAIN = "llvm-mingw-20261006-ucrt-macos-universal"
CRT_UNITS = ["cpp.c", "except.c", "handler4.c", "undname.c", "except_i386.c", "reactos/misc.c", "ctype.c", "data.c", "dir.c", "environ.c", "errno.c", "exit.c", "file.c", "heap.c", "locale.c", "lock.c", "main.c", "math.c", "mbcs.c", "misc.c", "process.c", "scanf.c", "sincos.c", "string.c", "thread.c", "time.c", "wcs.c", "console.c"]
HELPER_UNITS = ["sdk/lib/crt/mem/memmove.c", "sdk/lib/crt/math/frexp.c", "sdk/lib/crt/math/_dclass.c", "sdk/lib/crt/math/asin.c", "sdk/lib/crt/math/tanh.c", "sdk/lib/vcruntime/_fltused.c", "sdk/lib/vcruntime/__std_terminate.c", "sdk/lib/vcruntime/i386/longjmp.c"]
ASM_UNITS = ["dll/win32/msvcrt/i386/__wine_RtlUnwind.s", "dll/win32/msvcrt/i386/except_i386_asm.s", "dll/win32/msvcrt/i386/exception_ptr.s", "dll/win32/msvcrt/i386/inout.s", "sdk/lib/crt/except/i386/chkstk_asm.s"] + ["sdk/lib/crt/math/i386/" + name + "_asm.s" for name in ["aulldiv", "alldiv", "log10", "floor", "atan", "atan2", "exp", "sqrt", "sin", "cos"]] + ["sdk/lib/vcruntime/i386/" + name + ".s" for name in ["_setjmp", "_setjmp3", "__longjmp_nounwind"]]
STARTUP_UNITS = ["crtexe.c", "crtdll.c", "cinitexe.c", "mscmain.c", "mingw_helpers.c", "natstart.c", "gs_support.c", "tlsmcrt.c", "tlssup.c", "tlsthrd.c", "tlsmthread.c", "_newmode.c", "wildcard.c", "charmax.c", "atonexit.c", "crt_handler.c", "dll_argv.c", "dllargv.c", "dllentry.c", "reactos.c", "_matherr.c", "pesect.c", "pseudo-reloc.c"]

def commands(source, toolchain, work):
    clang = toolchain / "bin/clang"
    common = [str(clang), "-target", "i686-pc-windows-msvc", "-c", "-O1", "-ffunction-sections", "-fdata-sections", "-fno-builtin", "-ffile-prefix-map=" + str(source) + "=reactos-" + COMMIT, "-ffile-prefix-map=" + str(work) + "=ring3-fixture-build", "-fno-omit-frame-pointer", "-Wno-pragma-pack", "-Wno-macro-redefined", "-Wno-main", "-D__i386__=1", "-D_X86_=1", "-D_MSVCR_VER=0", "-DWINVER=0x601", "-D_WIN32_WINNT=0x601", "-D__ASM_USE_THISCALL_WRAPPER", "-D__REACTOS__", "-DWINE_NO_DEBUG_MSGS", "-DCOM_NO_WINDOWS_H", "-DWIN32_LEAN_AND_MEAN", "-D__ATTRIBUTE_SSE__=__attribute__((target(\"sse2\")))"]
    headers = [work / "generated/compiler", work / "generated/override", source / "sdk/include/vcruntime", toolchain / "lib/clang/23/include", source / "sdk/lib/pseh/include", source / "sdk/include/ddk", source / "sdk/include/psdk", work / "generated", source / "sdk/include/wine", source / "sdk/include/reactos", source / "sdk/include", toolchain / "i686-w64-mingw32/include"]
    common += ["-I" + str(path) for path in headers]
    crt = common + ["-I" + str(source / "dll/win32/msvcrt"), "-I" + str(source / "dll/win32/msvcrt/include"), "-D_CRTIMP=", "-DARRAY_SIZE=_countof", "-DI64_MIN=_I64_MIN", "-DI64_MAX=_I64_MAX", "-DUI64_MAX=_UI64_MAX", "-Dx87_asin=asin", "-Dmath_error=_invoke_matherr", "-Dasm_sqrt=sqrt", "-Dasm_sqrtf=sqrtf", "-DRtlUnwind=__wine_RtlUnwind"]
    # msvcrt's private headers must precede the public SDK fallback.
    crt.insert(crt.index("-I" + str(source / "sdk/include/wine")), "-I" + str(source / "dll/win32/msvcrt/include"))
    public = common[:]
    public[1:1] = ["-I" + str(source / "sdk/include/crt")]
    startup = public + ["-I" + str(source / "sdk/lib/crt/include/internal/mingw-w64"), "-D_CRTBLD", "-D_DLL", "-D_MSVCRTEX_", "-D_M_CEE_PURE", "-include", "stdarg.h"]
    authored = public + ["-D_DLL", "-x", "c++", "-fexceptions", "-fcxx-exceptions", "-fno-rtti", "-std=c++17", "-fno-threadsafe-statics", "-include", "stdarg.h"]
    return crt, public, startup, authored
