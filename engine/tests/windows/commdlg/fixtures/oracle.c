#define WIN32_LEAN_AND_MEAN
#define _WIN32_WINNT 0x0601
#include <windows.h>
#include <commdlg.h>
#include <dlgs.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>

struct probe {
    const char *name;
    BOOL wide, cancel, veto, timeout;
    HWND root, edit;
    void *ofn;
    const WCHAR *chosen_w;
    const char *chosen_a;
    UINT initialized, file_ok, same_ofn, events[64], count;
    UINT cancel_posted;
    DWORD observed_flags;
    WORD observed_file, observed_extension;
    UINT control_edit, control_ok, control_cancel;
    UINT edit_candidates, edit_id, edit_parent_id, admitted;
    UINT init_done, folder_done, action_posted, button_ready, activated, clicks;
    UINT_PTR action_timer;
    BOOL after_veto;
};

static int rows;
static struct probe *active;

static void stage(const char *name, const char *point, DWORD value) {
    fprintf(stderr, "COMDLG_STAGE %s %s %lu\n", name, point, value);
    fflush(stderr);
}

static void row(const char *name) {
    if (rows++) putchar(',');
    printf("{\"case\":\"%s\"", name);
}

static void wide_units(const WCHAR *text, size_t limit) {
    size_t i;
    putchar('[');
    for (i = 0; i < limit; ++i) {
        if (i) putchar(',');
        printf("%u", (unsigned)text[i]);
        if (!text[i]) break;
    }
    putchar(']');
}

static void narrow_units(const char *text, size_t limit) {
    size_t i;
    putchar('[');
    for (i = 0; i < limit; ++i) {
        if (i) putchar(',');
        printf("%u", (unsigned)(BYTE)text[i]);
        if (!text[i]) break;
    }
    putchar(']');
}

static BOOL CALLBACK cancel_window(HWND window, LPARAM unused) {
    WCHAR name[32];
    (void)unused;
    if (GetClassNameW(window, name, 32) && !lstrcmpW(name, L"#32770"))
        PostMessageW(window, WM_COMMAND, IDCANCEL, 0);
    return TRUE;
}

struct edit_search { HWND root, edit; UINT count; };
static BOOL CALLBACK filename_edit(HWND window, LPARAM data) {
    struct edit_search *search = (struct edit_search *)data;
    WCHAR class_name[32];
    HWND ancestor;
    UINT depth;
    if (!GetClassNameW(window, class_name, 32) || lstrcmpiW(class_name, L"Edit")) return TRUE;
    ancestor = window;
    for (depth = 0; ancestor && ancestor != search->root && depth < 32; ++depth) {
        int id = GetDlgCtrlID(ancestor);
        if (id == edt1 || id == cmb13) {
            search->edit = window;
            ++search->count;
            return TRUE;
        }
        ancestor = GetParent(ancestor);
    }
    return TRUE;
}
static BOOL admit_filename(struct probe *p) {
    struct edit_search search;
    WCHAR wide[512];
    char narrow[512];
    BOOL accepted;
    memset(&search, 0, sizeof(search));
    search.root = p->root;
    EnumChildWindows(p->root, filename_edit, (LPARAM)&search);
    p->edit_candidates = search.count;
    stage(p->name, "edit-candidates", search.count);
    if (search.count != 1 || !search.edit) return FALSE;
    p->edit = search.edit;
    p->edit_id = (UINT)GetDlgCtrlID(search.edit);
    p->edit_parent_id = (UINT)GetDlgCtrlID(GetParent(search.edit));
    if (p->wide) {
        accepted = SetWindowTextW(search.edit, p->chosen_w);
        if (!accepted || !GetWindowTextW(search.edit, wide, 512) || lstrcmpW(wide, p->chosen_w)) return FALSE;
    } else {
        accepted = SetWindowTextA(search.edit, p->chosen_a);
        if (!accepted || !GetWindowTextA(search.edit, narrow, 512) || lstrcmpA(narrow, p->chosen_a)) return FALSE;
    }
    p->admitted = 1;
    stage(p->name, "edit-admitted", p->edit_id);
    return TRUE;
}

static BOOL click_ok(struct probe *p) {
    HWND button = GetDlgItem(p->root, IDOK);
    HWND control = p->edit;
    WCHAR class_name[32];
    if (!button || !IsWindow(button)) return FALSE;
    if (control) {
        while (GetParent(control) && GetParent(control) != p->root) control = GetParent(control);
        if (!GetClassNameW(control, class_name, 32)) return FALSE;
        SendMessageW(p->root, WM_COMMAND,
            MAKEWPARAM(GetDlgCtrlID(control), !lstrcmpiW(class_name, L"Edit") ? EN_CHANGE : CBN_EDITCHANGE),
            (LPARAM)control);
    }
    p->button_ready = IsWindowEnabled(button) && IsWindowVisible(button);
    SetActiveWindow(p->root);
    p->activated = GetActiveWindow() == p->root;
    stage(p->name, "button-ready", p->button_ready);
    stage(p->name, "active", p->activated);
    if (!p->button_ready || !p->activated) return FALSE;
    SetFocus(button);
    ++p->clicks;
    stage(p->name, "post-button-click", p->clicks);
    SendMessageW(button, BM_CLICK, 0, 0);
    return TRUE;
}

static void CALLBACK timeout(HWND unused_window, UINT message, UINT_PTR id, DWORD tick) {
    (void)unused_window; (void)message; (void)tick;
    KillTimer(NULL, id);
    if (active) { active->timeout = TRUE; stage(active->name, "watchdog", tick); }
    EnumThreadWindows(GetCurrentThreadId(), cancel_window, 0);
}

static void CALLBACK action_timer(HWND unused_window, UINT message, UINT_PTR id, DWORD tick) {
    struct probe *p = active;
    (void)unused_window; (void)message; (void)tick;
    KillTimer(NULL, id);
    if (!p || p->action_timer != id) return;
    p->action_timer = 0;
    stage(p->name, p->after_veto ? "action-after-veto" : "action-initial", id);
    if ((!p->after_veto && !admit_filename(p)) || !click_ok(p))
        PostMessageW(p->root, WM_COMMAND, IDCANCEL, 0);
}

static BOOL schedule_action(struct probe *p, BOOL after_veto) {
    if (p->action_timer) return FALSE;
    p->after_veto = after_veto;
    p->action_timer = SetTimer(NULL, 0, 1, action_timer);
    stage(p->name, "schedule-action", (DWORD)p->action_timer);
    return p->action_timer != 0;
}

static UINT_PTR hook(HWND window, UINT message, LPARAM parameter, BOOL wide) {
    struct probe *p;
    if (message == WM_INITDIALOG) {
        if (wide) p = (struct probe *)((OPENFILENAMEW *)parameter)->lCustData;
        else p = (struct probe *)((OPENFILENAMEA *)parameter)->lCustData;
        SetWindowLongPtrW(window, GWLP_USERDATA, (LONG_PTR)p);
        ++p->initialized;
        stage(p->name, "init-dialog", p->initialized);
        p->same_ofn += (void *)parameter == p->ofn;
        p->root = GetParent(window);
        p->control_edit = GetDlgItem(p->root, edt1) != NULL;
        p->control_ok = GetDlgItem(p->root, IDOK) != NULL;
        p->control_cancel = GetDlgItem(p->root, IDCANCEL) != NULL;
        if (p->cancel) {
            p->cancel_posted = PostMessageW(p->root, WM_COMMAND, IDCANCEL, 0) != 0;
            stage(p->name, "post-cancel", p->cancel_posted);
        }
        return 0;
    }
    p = (struct probe *)GetWindowLongPtrW(window, GWLP_USERDATA);
    if (!p) return 0;
    if (message != WM_NOTIFY) return 0;
    if (p->count < 64) p->events[p->count++] = ((NMHDR *)parameter)->code;
    stage(p->name, "notify", ((NMHDR *)parameter)->code);
    if (((NMHDR *)parameter)->code == CDN_INITDONE) p->init_done = 1;
    if (((NMHDR *)parameter)->code == CDN_FOLDERCHANGE) p->folder_done = 1;
    if (((NMHDR *)parameter)->code == CDN_INITDONE && !p->cancel && !p->action_posted) {
        p->action_posted = 1;
        if (!schedule_action(p, FALSE)) PostMessageW(p->root, WM_COMMAND, IDCANCEL, 0);
    }
    if (((NMHDR *)parameter)->code == CDN_FILEOK) {
        ++p->file_ok;
        if (wide) {
            OPENFILENAMEW *ofn = ((OFNOTIFYW *)parameter)->lpOFN;
            p->same_ofn += ofn == p->ofn;
            p->observed_flags = ofn->Flags;
            p->observed_file = ofn->nFileOffset;
            p->observed_extension = ofn->nFileExtension;
        } else {
            OPENFILENAMEA *ofn = ((OFNOTIFYA *)parameter)->lpOFN;
            p->same_ofn += ofn == p->ofn;
            p->observed_flags = ofn->Flags;
            p->observed_file = ofn->nFileOffset;
            p->observed_extension = ofn->nFileExtension;
        }
        if (p->veto && p->file_ok == 1) {
            SetWindowLongPtrW(window, DWLP_MSGRESULT, 1);
            if (!schedule_action(p, TRUE)) PostMessageW(p->root, WM_COMMAND, IDCANCEL, 0);
            return 1;
        }
    }
    return 0;
}

static UINT_PTR CALLBACK hook_w(HWND window, UINT message, WPARAM word, LPARAM parameter) {
    (void)word;
    return hook(window, message, parameter, TRUE);
}

static UINT_PTR CALLBACK hook_a(HWND window, UINT message, WPARAM word, LPARAM parameter) {
    (void)word;
    return hook(window, message, parameter, FALSE);
}

static void describe_probe(struct probe *p, BOOL ok, DWORD error, DWORD last_error) {
    UINT i;
    row(p->name);
    printf(",\"ok\":%u,\"extended\":%lu,\"lastError\":%lu,\"timeout\":%u,"
        "\"initialized\":%u,\"fileOk\":%u,\"sameOfn\":%u,\"controls\":[%u,%u,%u],"
        "\"hookFlags\":%lu,\"hookOffsets\":[%u,%u],\"events\":[",
        ok != 0, error, last_error, p->timeout != 0, p->initialized, p->file_ok,
        p->same_ofn, p->control_edit, p->control_ok, p->control_cancel,
        p->observed_flags, p->observed_file, p->observed_extension);
    for (i = 0; i < p->count; ++i) {
        if (i) putchar(',');
        printf("%u", p->events[i]);
    }
    printf("],\"editCandidates\":%u,\"editId\":%u,\"editParentId\":%u,\"admitted\":%u,"
        "\"buttonReady\":%u,\"activated\":%u,\"clicks\":%u,\"ready\":[%u,%u],\"cancelPosted\":%u",
        p->edit_candidates, p->edit_id, p->edit_parent_id, p->admitted,
        p->button_ready, p->activated, p->clicks, p->init_done, p->folder_done, p->cancel_posted);
}

static void dialog_w(const char *name, BOOL save, const WCHAR *chosen, DWORD flags,
    DWORD capacity, BOOL cancel, BOOL veto, DWORD size) {
    OPENFILENAMEW ofn;
    struct probe p;
    WCHAR file[512], title[512], before[MAX_PATH], after[MAX_PATH];
    const WCHAR filter[] = L"Text files\0*.txt;*.log\0All files\0*.*\0\0";
    UINT_PTR timer;
    BOOL ok;
    DWORD error, last_error;
    memset(&ofn, 0, sizeof(ofn));
    memset(&p, 0, sizeof(p));
    memset(file, 0xcc, sizeof(file)); file[0] = 0;
    memset(title, 0xcc, sizeof(title));
    p.name = name; p.wide = TRUE; p.cancel = cancel; p.veto = veto;
    p.ofn = &ofn; p.chosen_w = chosen;
    if (!cancel && capacity > (DWORD)lstrlenW(chosen)) lstrcpyW(file, chosen);
    ofn.lStructSize = size;
    ofn.lpstrFilter = filter; ofn.nFilterIndex = 1;
    ofn.lpstrFile = file; ofn.nMaxFile = capacity;
    ofn.lpstrFileTitle = title; ofn.nMaxFileTitle = 512;
    ofn.lpstrDefExt = L"txt";
    ofn.Flags = flags | OFN_EXPLORER | OFN_ENABLEHOOK;
    ofn.lpfnHook = hook_w; ofn.lCustData = (LPARAM)&p;
    GetCurrentDirectoryW(MAX_PATH, before);
    active = &p;
    timer = SetTimer(NULL, 0, 20000, timeout);
    if (!timer) ExitProcess(2);
    SetLastError(0x12345678);
    stage(name, "enter-wide-api", size);
    ok = save ? GetSaveFileNameW(&ofn) : GetOpenFileNameW(&ofn);
    last_error = GetLastError(); error = CommDlgExtendedError();
    stage(name, "api-return", ok != 0);
    stage(name, "extended-error", error);
    KillTimer(NULL, timer);
    if (p.action_timer) { KillTimer(NULL, p.action_timer); p.action_timer = 0; } active = NULL;
    GetCurrentDirectoryW(MAX_PATH, after);
    describe_probe(&p, ok, error, last_error);
    printf(",\"flags\":%lu,\"filter\":%lu,\"offsets\":[%u,%u],\"requiredWord\":%u,\"file\":",
        ofn.Flags, ofn.nFilterIndex, ofn.nFileOffset, ofn.nFileExtension, (unsigned)(WORD)file[0]);
    wide_units(file, 512);
    printf(",\"title\":"); wide_units(title, 512);
    printf(",\"cwdBefore\":"); wide_units(before, MAX_PATH);
    printf(",\"cwdAfter\":"); wide_units(after, MAX_PATH);
    putchar('}');
    SetCurrentDirectoryW(before);
    fflush(stdout);
    if (p.timeout || (!p.cancel && (!p.admitted || !p.button_ready || !p.activated))) {
        stage(name, "fail-closed", p.timeout); ExitProcess(2);
    }
}

static void dialog_a(const char *name, BOOL save, const char *chosen, DWORD flags, DWORD capacity) {
    OPENFILENAMEA ofn;
    struct probe p;
    char file[512], title[512];
    const char filter[] = "Text files\0*.txt;*.log\0All files\0*.*\0\0";
    UINT_PTR timer;
    BOOL ok;
    DWORD error, last_error;
    memset(&ofn, 0, sizeof(ofn)); memset(&p, 0, sizeof(p));
    memset(file, 0xcc, sizeof(file)); file[0] = 0;
    memset(title, 0xcc, sizeof(title));
    p.name = name; p.ofn = &ofn; p.chosen_a = chosen;
    if (capacity > (DWORD)lstrlenA(chosen)) lstrcpyA(file, chosen);
    ofn.lStructSize = sizeof(ofn); ofn.lpstrFilter = filter; ofn.nFilterIndex = 1;
    ofn.lpstrFile = file; ofn.nMaxFile = capacity;
    ofn.lpstrFileTitle = title; ofn.nMaxFileTitle = 512;
    ofn.lpstrDefExt = "txt"; ofn.Flags = flags | OFN_EXPLORER | OFN_ENABLEHOOK;
    ofn.lpfnHook = hook_a; ofn.lCustData = (LPARAM)&p;
    active = &p; timer = SetTimer(NULL, 0, 20000, timeout);
    if (!timer) ExitProcess(2);
    SetLastError(0x12345678);
    stage(name, "enter-ansi-api", sizeof(ofn));
    ok = save ? GetSaveFileNameA(&ofn) : GetOpenFileNameA(&ofn);
    last_error = GetLastError(); error = CommDlgExtendedError();
    stage(name, "api-return", ok != 0);
    stage(name, "extended-error", error);
    KillTimer(NULL, timer);
    if (p.action_timer) { KillTimer(NULL, p.action_timer); p.action_timer = 0; } active = NULL;
    describe_probe(&p, ok, error, last_error);
    printf(",\"flags\":%lu,\"filter\":%lu,\"offsets\":[%u,%u],\"requiredWord\":%u,\"file\":",
        ofn.Flags, ofn.nFilterIndex, ofn.nFileOffset, ofn.nFileExtension, *(WORD *)file);
    narrow_units(file, 512); printf(",\"title\":"); narrow_units(title, 512); putchar('}');
    fflush(stdout);
    if (p.timeout || (!p.cancel && (!p.admitted || !p.button_ready || !p.activated))) {
        stage(name, "fail-closed", p.timeout); ExitProcess(2);
    }
}

struct thread_errors { DWORD before, after; };
static DWORD WINAPI invalid_size_in_thread(void *out) {
    OPENFILENAMEW ofn;
    struct thread_errors *errors = (struct thread_errors *)out;
    errors->before = CommDlgExtendedError();
    memset(&ofn, 0, sizeof(ofn));
    GetOpenFileNameW(&ofn);
    errors->after = CommDlgExtendedError();
    return 0;
}

int main(void) {
    WCHAR temporary[MAX_PATH], directory[MAX_PATH], existing[MAX_PATH], chosen[MAX_PATH];
    WCHAR original[MAX_PATH], title[64];
    char chosen_a[512], narrow[64];
    OPENFILENAMEW invalid;
    HANDLE file, thread;
    struct thread_errors thread_error;
    DWORD main_before, main_after;
    int capacity;
    stage("main", "start", sizeof(void *));
    if (!GetTempPathW(MAX_PATH, temporary) || !GetTempFileNameW(temporary, L"r3d", 0, directory)) return 2;
    if (!DeleteFileW(directory) || !CreateDirectoryW(directory, NULL)) return 2;
    lstrcpyW(existing, directory); lstrcatW(existing, L"\\present.txt");
    file = CreateFileW(existing, GENERIC_WRITE, FILE_SHARE_READ, NULL, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE || !CloseHandle(file)) return 2;
    GetCurrentDirectoryW(MAX_PATH, original);
    printf("{\"schema\":1,\"component\":\"commdlg-files\",\"pointerBytes\":%u,\"ansi\":%u,\"ofnBytes\":[%u,%u],"
        "\"offsets\":[%u,%u,%u,%u,%u],\"rows\":[", (unsigned)sizeof(void *), GetACP(),
        (unsigned)sizeof(OPENFILENAMEA), (unsigned)sizeof(OPENFILENAMEW),
        (unsigned)offsetof(OPENFILENAMEW, lpstrFile), (unsigned)offsetof(OPENFILENAMEW, nFileOffset),
        (unsigned)offsetof(OPENFILENAMEW, lpfnHook), (unsigned)offsetof(OPENFILENAMEW, pvReserved),
        (unsigned)OPENFILENAME_SIZE_VERSION_400W);
    stage("main", "temporary-ready", 0);
    memset(&invalid, 0, sizeof(invalid));
    SetLastError(0x12345678);
    { BOOL ok = GetOpenFileNameW(&invalid); DWORD last = GetLastError();
      row("invalid-size-zero"); printf(",\"ok\":%u,\"extended\":%lu,\"lastError\":%lu}", ok != 0, CommDlgExtendedError(), last); }
    main_before = CommDlgExtendedError();
    stage("thread-error-owner", "enter-worker", main_before);
    thread = CreateThread(NULL, 0, invalid_size_in_thread, &thread_error, 0, NULL);
    if (!thread) { stage("thread-error-owner", "create-failed", GetLastError()); return 2; }
    { DWORD wait = WaitForSingleObject(thread, 5000);
      stage("thread-error-owner", "worker-wait", wait);
      if (wait != WAIT_OBJECT_0) return 2; }
    CloseHandle(thread); main_after = CommDlgExtendedError();
    row("thread-error-owner"); printf(",\"mainBefore\":%lu,\"workerBefore\":%lu,\"workerAfter\":%lu,\"mainAfter\":%lu}", main_before, thread_error.before, thread_error.after, main_after);
    dialog_w("cancel-modern", FALSE, existing, 0, 512, TRUE, FALSE, sizeof(OPENFILENAMEW));
    dialog_w("cancel-nt4-size", FALSE, existing, 0, 512, TRUE, FALSE, OPENFILENAME_SIZE_VERSION_400W);
    dialog_w("existing-file", FALSE, existing, OFN_FILEMUSTEXIST, 512, FALSE, FALSE, sizeof(OPENFILENAMEW));
    dialog_w("file-ok-veto-once", FALSE, existing, OFN_FILEMUSTEXIST, 512, FALSE, TRUE, sizeof(OPENFILENAMEW));
    dialog_w("small-file-output", FALSE, existing, OFN_FILEMUSTEXIST, 2, FALSE, FALSE, sizeof(OPENFILENAMEW));
    lstrcpyW(chosen, directory); lstrcatW(chosen, L"\\newfile");
    dialog_w("save-default-extension", TRUE, chosen, OFN_NOTESTFILECREATE | OFN_NOCHANGEDIR, 512, FALSE, FALSE, sizeof(OPENFILENAMEW));
    dialog_w("save-novalidate", TRUE, chosen, OFN_NOTESTFILECREATE | OFN_NOVALIDATE, 512, FALSE, FALSE, sizeof(OPENFILENAMEW));
    dialog_w("open-nochangedir", FALSE, existing, OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR, 512, FALSE, FALSE, sizeof(OPENFILENAMEW));
    if (!WideCharToMultiByte(CP_ACP, 0, existing, -1, chosen_a, 512, NULL, NULL)) return 2;
    dialog_a("ansi-existing-file", FALSE, chosen_a, OFN_FILEMUSTEXIST, 512);
    dialog_a("ansi-small-output", FALSE, chosen_a, OFN_FILEMUSTEXIST, 2);
    for (capacity = 0; capacity <= 12; capacity += 3) {
        short result;
        memset(title, 0xcc, sizeof(title));
        result = GetFileTitleW(L"C:\\directory\\file.txt", capacity ? title : NULL, (WORD)capacity);
        row("file-title-wide"); printf(",\"capacity\":%d,\"result\":%d,\"output\":", capacity, (int)result);
        wide_units(title, 12); putchar('}');
        memset(narrow, 0xcc, sizeof(narrow));
        result = GetFileTitleA("C:\\directory\\file.txt", capacity ? narrow : NULL, (WORD)capacity);
        row("file-title-ansi"); printf(",\"capacity\":%d,\"result\":%d,\"output\":", capacity, (int)result);
        narrow_units(narrow, 12); putchar('}');
    }
    printf("]}\n");
    fflush(stdout);
    stage("main", "cases-complete", rows);
    SetCurrentDirectoryW(original);
    if (!DeleteFileW(existing) || !RemoveDirectoryW(directory)) return 2;
    return 0;
}
