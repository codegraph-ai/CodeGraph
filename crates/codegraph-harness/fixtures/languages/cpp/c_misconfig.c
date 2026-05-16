// Test fixture for C-specific misconfig patterns (T5-4 Tier 0).
// Each function exercises a different misconfiguration class so
// the detector should produce one finding per pattern.

#include <sys/types.h>
#include <sys/stat.h>
#include <unistd.h>
#include <stdio.h>

void permissive_umask(void) {
    umask(0);  // CWE-732: world-writable defaults
}

void world_writable_chmod(void) {
    chmod("/etc/secret", 0777);  // CWE-732
}

void predictable_tmp_fopen(void) {
    FILE *f = fopen("/tmp/myapp.lock", "w");  // CWE-377: symlink race
    (void)f;
}

void predictable_tmp_open(void) {
    int fd = open("/tmp/scratch", 0);  // CWE-377
    (void)fd;
}

void elevate_to_root(void) {
    setuid(0);  // CWE-250
}

void elevate_to_root_group(void) {
    setgid(0);  // CWE-250
}
