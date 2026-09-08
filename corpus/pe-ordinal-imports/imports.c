__declspec(dllimport) int ring3_probe(void);
void entry(void) {
    volatile int result = ring3_probe();
    __debugbreak();
    for (;;) {}
}
