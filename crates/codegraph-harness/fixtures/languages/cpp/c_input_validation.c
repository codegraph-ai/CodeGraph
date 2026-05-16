// Test fixture for C-specific input_validation patterns (T5-4 Tier 0).
//
// The input_validation detector matches a parameter when it appears
// as the FIRST argument of a dangerous op (op(param, ...)), or in
// array-index position, or via certain other adjacency forms. The
// fixture follows that contract: each vulnerable function uses its
// parameter as the immediate first arg of the dangerous C op.
//
// Expected behaviour:
// - exec_with_user_cmd → system(user_cmd)         → fires
// - run_via_popen      → popen(user_cmd, "r")     → fires
// - launch_exec        → execvp(user_prog, ...)   → fires
// - copy_user_path     → strcat(user_path, "/")   → fires
// - format_into_user   → sprintf(user_buf, ...)   → fires
// - read_user_buf      → gets(user_buf)           → fires
// - export_env         → setenv(user_key, ...)    → fires
// - safe_with_strlen   → strlen-gated before strcat → does NOT fire

#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>

extern char *argv_static[];

void exec_with_user_cmd(const char *user_cmd) {
    system(user_cmd);
}

void run_via_popen(const char *user_cmd) {
    FILE *f = popen(user_cmd, "r");
    (void)f;
}

void launch_exec(const char *user_prog) {
    execvp(user_prog, argv_static);
}

void copy_user_path(char *user_path) {
    strcat(user_path, "/suffix");
}

void format_into_user(char *user_buf) {
    sprintf(user_buf, "fmt=%d", 42);
}

void read_user_buf(char *user_buf) {
    gets(user_buf);
}

void export_env(const char *user_key) {
    setenv(user_key, "value", 1);
}

void safe_with_strlen(char *user_path) {
    if (strlen(user_path) >= 64) return;
    strcat(user_path, "/suffix");
}
