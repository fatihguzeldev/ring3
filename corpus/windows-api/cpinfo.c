typedef struct {
    unsigned int MaxCharSize;
    unsigned char DefaultChar[2];
    unsigned char LeadByte[12];
} CpInfo;
_Static_assert(sizeof(CpInfo) == 20, "i686 cpinfo size");
_Static_assert(__builtin_offsetof(CpInfo, DefaultChar) == 4, "default character offset");
_Static_assert(__builtin_offsetof(CpInfo, LeadByte) == 6, "lead byte offset");

__declspec(dllimport) unsigned int __stdcall GetACP(void);
__declspec(dllimport) unsigned int __stdcall GetOEMCP(void);
__declspec(dllimport) int __stdcall GetCPInfo(unsigned int, CpInfo *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    unsigned int pages[5] = {0, 1, 3, GetACP(), GetOEMCP()};
    union { CpInfo info; unsigned int words[5]; } image;
    SetLastError(77);
    for (unsigned int i = 0; i < 5; ++i) {
        for (unsigned int j = 0; j < 5; ++j) image.words[j] = 0xa5a5a5a5;
        if (!GetCPInfo(pages[i], &image.info)) ExitProcess(1);
        if (image.info.MaxCharSize != 1 || image.words[1] != 63 || image.words[2] || image.words[3] || image.words[4] != 0xa5a50000) ExitProcess(2);
    }
    if (GetLastError() != 77) ExitProcess(3);
    if (GetCPInfo(437, 0) || GetLastError() != 87) ExitProcess(4);
    ExitProcess(42);
}
