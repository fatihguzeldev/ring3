__declspec(dllimport) int ring3_middle(void);
__declspec(dllexport) int ring3_leaf(void) {
    return ring3_middle() + 1;
}
