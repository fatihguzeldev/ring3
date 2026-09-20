typedef void (__cdecl *initializer)(void);
__declspec(dllimport) void __cdecl _initterm(initializer*, initializer*);
__declspec(dllimport) unsigned int __cdecl _controlfp(unsigned int, unsigned int);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static void first(void);
static void second(void);
static void third(void);
static void nested(void);
static initializer table[] = { first, 0, second, 0 };
static initializer inner[] = { nested };
volatile unsigned int input = 13;
volatile unsigned int value;

static void first(void) {
    value = input;
    table[3] = third;
}

static void second(void) {
    value += 5;
    _initterm(inner, inner + 1);
}

static void third(void) {
    value += value;
}

static void nested(void) {
    if (_controlfp(0, 0) != 0x0009001f) ExitProcess(1);
    value += 3;
}

void entry(void) {
    _initterm(table, table + 4);
    ExitProcess(value);
}
