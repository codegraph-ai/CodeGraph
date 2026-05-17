// Test fixture for find_similar mode=cleanup_pattern (Shape B,
// CVE-2026-46333). The mode is a candidate-pool filter that keeps
// only functions whose name matches the teardown regex AND whose
// body has ≥2 clear-sites. Used by bounty sweep work to find
// cleanup_order_asymmetry candidates without shipping a full
// inter-procedural detector.
//
// Fixture layout:
//   - do_exit_like      = the ANCHOR (target of find_similar)
//   - cleanup_resources = teardown sibling, ≥2 clear-sites, MATCHES
//   - destroy_session   = teardown sibling, ≥2 clear-sites, MATCHES
//   - close_one_fd      = teardown name but only 1 clear-site, EXCLUDED
//   - compute_hash      = non-teardown name, EXCLUDED
//   - parse_input       = non-teardown name, EXCLUDED

#include <stddef.h>

struct mm_struct { int dumpable; };
struct fd_array { int count; };

struct task {
    struct mm_struct *mm;
    struct fd_array *fds;
    int *creds;
    int pid;
};

static void mmput(struct mm_struct *mm) { (void)mm; }
static void close_fd_array(struct fd_array *f) { (void)f; }
static void kfree(void *p) { (void)p; }

// ANCHOR: kernel-style do_exit. Clears mm, fds, creds — three clear-sites.
// This is the function the bounty sweep would target as the
// semantic anchor for Shape B candidates.
void do_exit_like(struct task *t) {
    mmput(t->mm);
    t->mm = NULL;
    close_fd_array(t->fds);
    t->fds = NULL;
    kfree(t->creds);
    t->creds = NULL;
}

// MATCH: teardown name + 2 clear-sites.
void cleanup_resources(struct task *t) {
    kfree(t->creds);
    t->creds = NULL;
    t->fds = NULL;
}

// MATCH: teardown name + 2 clear-sites.
void destroy_session(struct task *t) {
    mmput(t->mm);
    t->mm = NULL;
    t->fds = NULL;
}

// EXCLUDED: teardown name but only 1 clear-site.
void close_one_fd(struct task *t) {
    close_fd_array(t->fds);
}

// EXCLUDED: non-teardown name (regex doesn't match).
int compute_hash(struct task *t) {
    return t->pid * 31;
}

// EXCLUDED: non-teardown name.
int parse_input(const char *buf) {
    return buf ? 1 : 0;
}
