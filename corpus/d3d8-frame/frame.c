typedef unsigned int u32;
typedef int result;
typedef struct { void **methods; } object;
typedef result (__stdcall *create_device)(object *, u32, u32, u32, u32, u32 *, object **);
typedef result (__stdcall *clear_target)(object *, u32, const int *, u32, u32, float, u32);
typedef result (__stdcall *present_frame)(object *, void *, void *, u32, void *);
typedef u32 (__stdcall *release_object)(object *);
typedef u32 (__stdcall *adapter_count)(object *);

__declspec(dllimport) object *__stdcall Direct3DCreate8(u32);
__declspec(dllimport) u32 __stdcall GetDesktopWindow(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(u32);

u32 presentation[13] = {320, 200, 22, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0};
int left[4] = {24, 24, 152, 176};
int right[4] = {168, 24, 296, 176};
int stripe[4] = {0, 88, 320, 112};
object *device;
int _fltused = 0;

void entry(void) {
    object *root = Direct3DCreate8(120);
    if (root == 0) ExitProcess(1);
    if (((adapter_count)root->methods[4])(root) != 1) ExitProcess(8);
    u32 desktop = GetDesktopWindow();
    presentation[6] = desktop;
    result status = ((create_device)root->methods[15])(
        root, 0, 1, desktop, 0x20, presentation, &device);
    if (status != 0) ExitProcess(2);
    clear_target clear = (clear_target)device->methods[36];
    if (clear(device, 0, 0, 1, 0xff102030, 0.0f, 0) != 0) ExitProcess(3);
    if (clear(device, 1, left, 1, 0xffe86c42, 0.0f, 0) != 0) ExitProcess(4);
    if (clear(device, 1, right, 1, 0xff42bad1, 0.0f, 0) != 0) ExitProcess(5);
    if (clear(device, 1, stripe, 1, 0xfff4d35e, 0.0f, 0) != 0) ExitProcess(6);
    if (((present_frame)device->methods[15])(device, 0, 0, 0, 0) != 0) ExitProcess(7);
    ((release_object)device->methods[2])(device);
    ((release_object)root->methods[2])(root);
    ExitProcess(0);
}
