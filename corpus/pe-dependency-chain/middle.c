__declspec(dllimport) int ring3_leaf(void);
__declspec(dllexport) int ring3_middle(void) {
    return ring3_leaf() + 1;
}
