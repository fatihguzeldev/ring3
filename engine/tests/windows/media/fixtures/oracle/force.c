#define DIRECTINPUT_VERSION 0x0800
#define INITGUID
#include <windows.h>
#include <dinput.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

_Static_assert(sizeof(void *) == 4, "this oracle requires a native 32-bit build");
_Static_assert(sizeof(DIEFFECT) == 56, "literal pe32 effect layout");

static int inspect_only;

static BOOL CALLBACK inspect_effect(const DIFILEEFFECT *file, void *context) {
    const DIEFFECT *effect = file->lpDiEffect;
    unsigned *count = context;
    ++*count;
    printf("callback size=%lu flags=%08lx axes=%lu payload=%lu\n",
           (unsigned long)effect->dwSize, (unsigned long)effect->dwFlags,
           (unsigned long)effect->cAxes, (unsigned long)effect->cbTypeSpecificParams);
    if (IsEqualGUID(&file->GuidEffect, &GUID_CustomForce) &&
        effect->cbTypeSpecificParams == sizeof(DICUSTOMFORCE)) {
        const DICUSTOMFORCE *custom = effect->lpvTypeSpecificParams;
        printf("custom channels=%lu period=%lu samples=%lu values=",
               (unsigned long)custom->cChannels, (unsigned long)custom->dwSamplePeriod,
               (unsigned long)custom->cSamples);
        if (!inspect_only && custom->cSamples <= 8 && custom->rglForceData) {
            for (DWORD i = 0; i < custom->cSamples; ++i)
                printf("%s%ld", i ? "," : "", (long)custom->rglForceData[i]);
        }
        MEMORY_BASIC_INFORMATION region = {0};
        SIZE_T queried = VirtualQuery(custom->rglForceData, &region, sizeof(region));
        printf(" pointer=%08lx mode=%s queried=%lu state=%08lx protection=%08lx\n",
               (unsigned long)(uintptr_t)custom->rglForceData,
               inspect_only ? "fresh" : "writer", (unsigned long)queried,
               (unsigned long)region.State, (unsigned long)region.Protect);
    }
    return DIENUM_CONTINUE;
}
static void dump_file(const char *path) {
    FILE *file = fopen(path, "rb");
    if (!file) return;
    printf("file %s hex=", path);
    unsigned count = 0;
    for (int byte; (byte = fgetc(file)) != EOF && count < 65536; ++count)
        printf("%02x", (unsigned)byte);
    printf(" bytes=%u\n", count);
    fclose(file);
}
static void probe(IDirectInputDevice8A *device, GUID kind, void *payload,
                  DWORD payload_size, const char *path) {
    DWORD axes[2] = {DIDFT_ABSAXIS, DIDFT_ABSAXIS | DIDFT_MAKEINSTANCE(1)};
    LONG direction[2] = {1, 0};
    DIEFFECT effect = {0};
    effect.dwSize = sizeof(effect);
    effect.dwFlags = DIEFF_OBJECTIDS | DIEFF_CARTESIAN;
    effect.dwDuration = 10000;
    effect.dwGain = 10000;
    effect.dwTriggerButton = DIEB_NOTRIGGER;
    effect.cAxes = 2;
    effect.rgdwAxes = axes;
    effect.rglDirection = direction;
    effect.cbTypeSpecificParams = payload_size;
    effect.lpvTypeSpecificParams = payload;
    DIFILEEFFECT file = {0};
    file.dwSize = sizeof(file);
    file.GuidEffect = kind;
    file.lpDiEffect = &effect;
    strcpy(file.szFriendlyName, "ring3 self-authored force");
    HRESULT result = device->lpVtbl->WriteEffectToFile(device, path, 1, &file, 0);
    printf("write %s hr=%08lx\n", path, (unsigned long)result);
    if (SUCCEEDED(result)) {
        unsigned count = 0;
        result = device->lpVtbl->EnumEffectsInFile(device, path, inspect_effect, &count, 0);
        printf("read %s hr=%08lx callbacks=%u\n", path, (unsigned long)result, count);
        dump_file(path);
    }
}
int main(int argc, char **argv) {
    HMODULE module = LoadLibraryA("dinput8.dll");
    if (!module) return 2;
    typedef HRESULT (WINAPI *Create8)(HINSTANCE, DWORD, REFIID, void **, void *);
    Create8 create = (Create8)GetProcAddress(module, "DirectInput8Create");
    if (!create) return 3;
    IDirectInput8A *input = NULL;
    HRESULT result = create(GetModuleHandleA(NULL), 0x800, &IID_IDirectInput8A, (void **)&input, NULL);
    printf("factory hr=%08lx\n", (unsigned long)result);
    if (FAILED(result)) return 4;
    IDirectInputDevice8A *device = NULL;
    result = input->lpVtbl->CreateDevice(input, &GUID_SysKeyboard, &device, NULL);
    printf("device hr=%08lx\n", (unsigned long)result);
    if (FAILED(result)) {input->lpVtbl->Release(input); return 5;}
    if (argc == 2 && strcmp(argv[1], "--inspect-existing") == 0) {
        const char *paths[] = {"ring3-authored-constant.ffe", "ring3-authored-custom.ffe", "ring3-authored-custom-mono.ffe"};
        inspect_only = 1;
        for (unsigned index = 0; index < sizeof(paths) / sizeof(paths[0]); ++index) {
            unsigned count = 0;
            result = device->lpVtbl->EnumEffectsInFile(device, paths[index], inspect_effect, &count, 0);
            printf("freshread %s hr=%08lx callbacks=%u\n", paths[index], (unsigned long)result, count);
        }
        device->lpVtbl->Release(device);
        input->lpVtbl->Release(input);
        FreeLibrary(module);
        return 0;
    }
    DICONSTANTFORCE constant = {4000};
    LONG samples[8] = {1000, -1000, 2000, -2000, 3000, -3000, 4000, -4000};
    DICUSTOMFORCE custom = {2, 1000, 8, samples};
    probe(device, GUID_ConstantForce, &constant, sizeof(constant), "ring3-authored-constant.ffe");
    probe(device, GUID_CustomForce, &custom, sizeof(custom), "ring3-authored-custom.ffe");
    custom.cChannels = 1;
    custom.dwSamplePeriod = 7000;
    custom.cSamples = 4;
    probe(device, GUID_CustomForce, &custom, sizeof(custom), "ring3-authored-custom-mono.ffe");
    device->lpVtbl->Release(device);
    input->lpVtbl->Release(input);
    FreeLibrary(module);
    return 0;
}
