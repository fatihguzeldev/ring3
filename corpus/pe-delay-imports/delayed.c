__declspec(dllimport) int probe(void);

void *__stdcall __delayLoadHelper2(const void *descriptor, void *slot) {
    (void)descriptor;
    (void)slot;
    return (void *)0;
}

int entry(void) { return probe(); }
