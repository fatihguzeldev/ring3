typedef unsigned int u32;
typedef int result;
typedef struct { void **methods; } object;
typedef result (__stdcall *create_device)(object *, u32, u32, u32, u32, u32 *, object **);
typedef result (__stdcall *clear_target)(object *, u32, const int *, u32, u32, float, u32);
typedef result (__stdcall *present_frame)(object *, void *, void *, u32, void *);
typedef u32 (__stdcall *release_object)(object *);
typedef u32 (__stdcall *adapter_count)(object *);
typedef result (__stdcall *get_display_mode)(object *, u32, u32 *);
typedef result (__stdcall *check_format)(object *, u32, u32, u32, u32, u32, u32);
typedef result (__stdcall *create_texture)(object *, u32, u32, u32, u32, u32, u32, object **);
typedef u32 (__stdcall *get_level_count)(object *);
typedef result (__stdcall *get_level_desc)(object *, u32, u32 *);
typedef result (__stdcall *lock_rect)(object *, u32, u32 *, const int *, u32);
typedef result (__stdcall *unlock_rect)(object *, u32);
typedef result (__stdcall *set_vertex_shader)(object *, u32);
typedef result (__stdcall *draw_primitive_up)(object *, u32, u32, const void *, u32);
typedef struct { float x, y, z, rhw; u32 diffuse; } color_vertex;
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
typedef result (__stdcall *get_device_caps)(object *, u32, u32, u32 *);

__declspec(dllimport) object *__stdcall Direct3DCreate8(u32);
__declspec(dllimport) u32 __stdcall GetDesktopWindow(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(u32);

u32 presentation[13] = {320, 200, 22, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0};
int left[4] = {24, 24, 152, 176};
int right[4] = {168, 24, 296, 176};
int stripe[4] = {0, 88, 320, 112};
color_vertex triangle[3] = {
    {40.0f, 40.0f, 0.0f, 1.0f, 0xff00ff00},
    {80.0f, 40.0f, 0.0f, 1.0f, 0xff00ff00},
    {40.0f, 80.0f, 0.0f, 1.0f, 0xff00ff00},
};
object *device;
object *texture;
adapter_identifier identifier;
u32 caps[53];
u32 current_mode[4];
u32 texture_desc[8];
u32 locked_rect_data[2];
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
    caps[0] = 0x12345678;
    if ((u32)((get_device_caps)root->methods[13])(root, 0, 2, caps) != 0x8876086a ||
        caps[0] != 0x12345678) ExitProcess(12);
    if ((u32)((get_device_caps)root->methods[13])(root, 0, 3, caps) != 0x8876086a ||
        caps[0] != 0x12345678) ExitProcess(13);
    if (((get_device_caps)root->methods[13])(root, 0, 1, caps) != 0) ExitProcess(14);
    if (caps[0] != 1 || caps[1] != 0 || caps[2] != 0 || caps[3] != 0x00080000 ||
        caps[4] != 0 || caps[7] != 0x400 || caps[45] != 4096 ||
        caps[47] != 1 || caps[48] != 256 || caps[52] != 0) ExitProcess(15);
    if (((get_display_mode)root->methods[8])(root, 0, current_mode) != 0 ||
        current_mode[0] != 640 || current_mode[1] != 480 ||
        current_mode[2] != 0 || current_mode[3] != 22) ExitProcess(33);
    u32 desktop = GetDesktopWindow();
    presentation[6] = desktop;
    result status = ((create_device)root->methods[15])(
        root, 0, 1, desktop, 0x20, presentation, &device);
    if (status != 0) ExitProcess(2);
    if (((check_format)root->methods[10])(root, 0, 1, 22, 0, 3, 22) != 0) ExitProcess(16);
    if (((create_texture)device->methods[20])(
        device, 4, 2, 0, 0, 22, 1, &texture) != 0) ExitProcess(17);
    if (((get_level_count)texture->methods[13])(texture) != 3) ExitProcess(18);
    if (((get_level_desc)texture->methods[14])(texture, 1, texture_desc) != 0 ||
        texture_desc[0] != 22 || texture_desc[1] != 3 || texture_desc[3] != 1 ||
        texture_desc[4] != 8 || texture_desc[6] != 2 || texture_desc[7] != 1) ExitProcess(19);
    if (((lock_rect)texture->methods[16])(texture, 0, locked_rect_data, 0, 0) != 0 ||
        locked_rect_data[0] != 16 || locked_rect_data[1] == 0) ExitProcess(20);
    ((u32 *)locked_rect_data[1])[0] = 0x00112233;
    if (((unlock_rect)texture->methods[17])(texture, 0) != 0) ExitProcess(21);
    if (((lock_rect)texture->methods[16])(texture, 0, locked_rect_data, 0, 0) != 0 ||
        ((u32 *)locked_rect_data[1])[0] != 0x00112233) ExitProcess(22);
    if (((unlock_rect)texture->methods[17])(texture, 0) != 0) ExitProcess(23);
    if (((release_object)texture->methods[2])(texture) != 0) ExitProcess(24);
    if (((check_format)root->methods[10])(root, 0, 1, 22, 0, 3, 21) != 0) ExitProcess(25);
    if (((create_texture)device->methods[20])(
        device, 2, 1, 1, 0, 21, 1, &texture) != 0) ExitProcess(26);
    if (((get_level_desc)texture->methods[14])(texture, 0, texture_desc) != 0 ||
        texture_desc[0] != 21 || texture_desc[4] != 8) ExitProcess(27);
    if (((lock_rect)texture->methods[16])(texture, 0, locked_rect_data, 0, 0) != 0 ||
        locked_rect_data[0] != 8) ExitProcess(28);
    ((u32 *)locked_rect_data[1])[0] = 0x7f112233;
    if (((unlock_rect)texture->methods[17])(texture, 0) != 0) ExitProcess(29);
    if (((lock_rect)texture->methods[16])(texture, 0, locked_rect_data, 0, 0) != 0 ||
        ((u32 *)locked_rect_data[1])[0] != 0x7f112233) ExitProcess(30);
    if (((unlock_rect)texture->methods[17])(texture, 0) != 0) ExitProcess(31);
    if (((release_object)texture->methods[2])(texture) != 0) ExitProcess(32);
    clear_target clear = (clear_target)device->methods[36];
    if (clear(device, 0, 0, 1, 0xff102030, 0.0f, 0) != 0) ExitProcess(3);
    if (clear(device, 1, left, 1, 0xffe86c42, 0.0f, 0) != 0) ExitProcess(4);
    if (clear(device, 1, right, 1, 0xff42bad1, 0.0f, 0) != 0) ExitProcess(5);
    if (clear(device, 1, stripe, 1, 0xfff4d35e, 0.0f, 0) != 0) ExitProcess(6);
    if (((set_vertex_shader)device->methods[76])(device, 0x44) != 0) ExitProcess(34);
    if (((draw_primitive_up)device->methods[72])(
        device, 4, 1, triangle, sizeof(triangle[0])) != 0) ExitProcess(35);
    if (((present_frame)device->methods[15])(device, 0, 0, 0, 0) != 0) ExitProcess(7);
    ((release_object)device->methods[2])(device);
    ((release_object)root->methods[2])(root);
    ExitProcess(0);
}
