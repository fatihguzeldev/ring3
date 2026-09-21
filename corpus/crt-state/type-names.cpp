extern "C" __declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

class type_info {
public:
    void *vtable;
    char *cached;
    char raw[64];
    __declspec(dllimport) const char *name() const;
};

static type_info types[] = {
    {0, 0, ".?AVWidget@engine@@"},
    {0, 0, ".?AUNode@inner@outer@@"},
    {0, 0, ".?AT_Value2@@"},
    {0, 0, ".?AW4Mode@engine@@"},
};
static const char *expected[] = {
    "class engine::Widget", "struct outer::inner::Node", "union _Value2", "enum engine::Mode",
};

extern "C" void entry() {
    for (int i = 0; i < 4; ++i) {
        const char *name = types[i].name();
        if (!name || types[i].cached != name || types[i].name() != name) ExitProcess(1);
        int j = 0;
        while (expected[i][j]) {
            if (name[j] != expected[i][j]) ExitProcess(2);
            ++j;
        }
        if (name[j]) ExitProcess(3);
    }
    ExitProcess(42);
}
