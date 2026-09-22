typedef unsigned int u32;
typedef int result;
typedef struct { void **methods; } object;
typedef result (__stdcall *create_device)(object *, u32, u32, u32, u32, u32 *, object **);
typedef result (__stdcall *clear_target)(object *, u32, const int *, u32, u32, float, u32);
typedef result (__stdcall *present_frame)(object *, void *, void *, u32, void *);
typedef u32 (__stdcall *release_object)(object *);
typedef u32 (__stdcall *adapter_count)(object *);
#pragma pack(push, 4)
typedef struct {
    u32 data1;
    unsigned short data2;
    unsigned short data3;
    unsigned char data4[8];
} guid;
typedef struct {
    char driver[512];
    char description[512];
    unsigned long long driver_version;
    u32 vendor_id;
    u32 device_id;
    u32 subsystem_id;
    u32 revision;
    guid device_identifier;
    u32 whql_level;
} adapter_identifier;
#pragma pack(pop)
typedef result (__stdcall *get_adapter_identifier)(object *, u32, u32, adapter_identifier *);

__declspec(dllimport) object *__stdcall Direct3DCreate8(u32);
__declspec(dllimport) u32 __stdcall GetDesktopWindow(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(u32);

u32 presentation[13] = {320, 200, 22, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0};
int left[4] = {24, 24, 152, 176};
int right[4] = {168, 24, 296, 176};
int stripe[4] = {0, 88, 320, 112};
object *device;
adapter_identifier identifier;
int _fltused = 0;

void entry(void) {
    object *root = Direct3DCreate8(120);
    if (root == 0) ExitProcess(1);
    if (((adapter_count)root->methods[4])(root) != 1) ExitProcess(8);
    if (sizeof(identifier) != 1068) ExitProcess(9);
    if (((get_adapter_identifier)root->methods[5])(root, 0, 2, &identifier) != 0) ExitProcess(10);
    if (identifier.driver[0] != 'r' || identifier.driver[4] != '3' || identifier.driver[5] != 0 ||
        identifier.description[0] != 'R' || identifier.description[28] != 'r' ||
        identifier.description[29] != 0 || identifier.driver_version != 0 ||
        identifier.vendor_id != 0 || identifier.device_id != 0 || identifier.subsystem_id != 0 ||
        identifier.revision != 0 || identifier.device_identifier.data1 != 0 ||
        identifier.device_identifier.data4[7] != 0 || identifier.whql_level != 0) ExitProcess(11);
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
