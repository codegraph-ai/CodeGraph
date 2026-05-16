// Test fixture for search_path macro resolution.
//
// Without macro resolution, `dlopen(QPL_LIBRARY_NAME, ...)` shows up
// as `NonLiteral` and the detector marks it medium severity. With
// resolution, QPL_LIBRARY_NAME expands to "libqpl.so.1" → Relative
// → high severity. This is exactly the DCAP scan pattern that
// motivated T5-4 Tier 0 #2.

#include <dlfcn.h>

#define QPL_LIBRARY_NAME "libqpl.so.1"
constexpr const char* TPM2_LIBRARY = "libtpm2.so";

void *load_qpl(void) {
    // Should be flagged high: macro resolves to a relative path.
    return dlopen(QPL_LIBRARY_NAME, RTLD_LAZY);
}

void *load_tpm2(void) {
    // Should also be flagged high: constexpr resolves to a relative path.
    return dlopen(TPM2_LIBRARY, RTLD_LAZY);
}
