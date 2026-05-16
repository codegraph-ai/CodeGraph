// Test fixture for C-specific error_exposure patterns (T5-4 Tier 0).
//
// For a finding to fire, the detector needs ALL of:
// 1. An error-handler context (`errno`, `if (ret < 0)`, `goto err`, etc.)
// 2. A response/output sink (`write`, `dprintf`, `send`, `fprintf`, ...)
// 3. An exposure pattern (`strerror(errno)`, `__FILE__`, `backtrace_*`, ...)
//
// Each handler below combines all three so the detector fires once
// per function. The clean_response function omits the exposure
// pattern and should stay silent.

#include <errno.h>
#include <string.h>
#include <stdio.h>
#include <unistd.h>
#include <execinfo.h>

void handle_strerror_leak(int client_fd) {
    int ret = -1;
    if (ret < 0) {
        // strerror(errno) translates the system error → may reveal
        // filesystem layout; dprintf sends it straight to the client.
        dprintf(client_fd, "Error: %s\n", strerror(errno));
    }
}

void handle_file_macro_leak(int client_fd) {
    if (errno != 0) {
        // __FILE__ leaks the build host's source path.
        dprintf(client_fd, "Failed at %s:%d\n", __FILE__, __LINE__);
    }
}

void handle_perror_then_send(int sockfd, char *buf, int len) {
    if (errno) {
        perror("op failed");        // exposure pattern
        send(sockfd, buf, len, 0);  // response sink
    }
}

void handle_backtrace_dump(int client_fd) {
    void *frames[64];
    int n = 64;
    if (errno) {
        // Direct stack-trace dump to client fd.
        backtrace_symbols_fd(frames, n, client_fd);
    }
}

// SAFE: no exposure pattern; returns generic message.
void clean_response(int client_fd) {
    if (errno) {
        dprintf(client_fd, "Internal error\n");
    }
}
