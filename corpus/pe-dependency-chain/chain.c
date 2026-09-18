__declspec(dllimport) int ring3_middle(void);
void entry(void) {
    volatile int result = ring3_middle();
    __debugbreak();
    for (;;) {}
}
