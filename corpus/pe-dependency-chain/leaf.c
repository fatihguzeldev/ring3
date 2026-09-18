__declspec(dllexport) int ring3_leaf(void) {
    volatile int value = 40;
    return value + 1;
}
