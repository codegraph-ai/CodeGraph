// Test fixture for search_path per-callsite dedup. A single function
// calls dlopen four times with different relative-path arguments.
// Before T5-5 #1, only the first call site was reported and the
// other three were silently dropped (dedup by enclosing function).
// After the fix, each call site fires its own finding.

#include <dlfcn.h>

void *load_all_plugins(void) {
    void *a = dlopen("libplugin_a.so", RTLD_LAZY);
    void *b = dlopen("libplugin_b.so", RTLD_LAZY);
    void *c = dlopen("libplugin_c.so", RTLD_LAZY);
    void *d = dlopen("libplugin_d.so", RTLD_LAZY);
    (void)a; (void)b; (void)c; (void)d;
    return d;
}
