// Test fixture for search_path wrapper-library detection (T5-5 #4).
// Exercises four dlopen-equivalent wrappers that previously slipped
// past the detector: g_module_open (GLib), lt_dlopen (libltdl),
// ENGINE_load_dynamic (OpenSSL), Tss2_TctiLdr_Initialize (tpm2-tss).
// Each call uses a relative path — all four should fire.

void *g_module_open(const char *path, int flags);
void *lt_dlopen(const char *path);
void *ENGINE_load_dynamic(const char *engine_path);
int Tss2_TctiLdr_Initialize(const char *name_conf, void **context);

void load_via_glib(void) {
    // GLib wrapper around dlopen.
    g_module_open("libplugin.so", 0);
}

void load_via_libltdl(void) {
    // libtool's portable dynamic loader.
    lt_dlopen("libfoo.so");
}

void load_via_openssl(void) {
    // OpenSSL dynamic engine — SO_PATH equivalent passed in directly here.
    ENGINE_load_dynamic("libengine.so");
}

void load_via_tpm2_tss(void) {
    // TCTI module loader — resolved via dlopen on the supplied name.
    void *ctx = 0;
    Tss2_TctiLdr_Initialize("device", &ctx);
}
