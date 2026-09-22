extern "C" __declspec(dllimport) void __stdcall ExitProcess(unsigned int);

// the fixture owns this unused vtable so generated rtti needs no crt data import.
extern "C" void* type_info_vtable[] __asm__("??_7type_info@@6B@") = {nullptr};

struct Base {
    virtual int value() { return 1; }
};

struct Side {
    virtual int side() { return 2; }
};

struct Derived : Base, Side {
    int value() override { return 3; }
    int side() override { return 4; }
};

struct Other : Base {
    int value() override { return 5; }
};

extern "C" void entry() {
    Derived object;
    Base* base = &object;
    if (dynamic_cast<Derived*>(base) != &object) ExitProcess(1);
    if (dynamic_cast<Side*>(base) != static_cast<Side*>(&object)) ExitProcess(2);
    if (dynamic_cast<Other*>(base) != nullptr) ExitProcess(3);
    ExitProcess(0);
}
