// Test fixture for Shape A (CVE-2026-46333) — fail-open on null
// precondition in a decision-class function.
//
// Pattern: a function whose name matches the decision-class regex
// (e.g. `*_may_access`) has an `if (!precondition) return 0;` early
// exit that skips a downstream security check (`get_dumpable`,
// `cap_*`, etc.). Modelled on `__ptrace_may_access` from the CVE
// commit `31e62c2ebbfd` ("ptrace: slightly saner 'get_dumpable()'
// logic", Linus, 2026-05-14).
//
// Heuristic 3 (downstream check token after the early return) must
// fire for each positive — `get_dumpable` / `cap_admin` are the
// canonical tokens. The negative function omits the downstream
// check so the detector stays silent.

#include <stddef.h>

#define SUID_DUMP_USER 1

struct mm_struct {
    int dumpable;
    int owner_uid;
};

struct cred {
    int uid;
    int gid;
};

struct task_struct {
    struct mm_struct *mm;
    struct cred *creds;
    int pid;
};

static int get_dumpable(struct mm_struct *mm) { return mm->dumpable; }
static int cap_admin(struct cred *c) { return c->uid == 0; }

// POSITIVE: classic CVE-2026-46333 shape — single-line null-precondition
// early return on `!task->mm` followed by a downstream `get_dumpable` check.
int __ptrace_may_access(struct task_struct *task) {
    if (!task->mm) return 0;
    int dumpable = get_dumpable(task->mm);
    if (dumpable != SUID_DUMP_USER) return -1;
    return 0;
}

// POSITIVE: multi-line block form. Same logical bug, different syntax.
int check_admin_access(struct task_struct *task) {
    if (task->creds == NULL) {
        return 0;
    }
    if (!cap_admin(task->creds)) return -1;
    return 0;
}

// POSITIVE: `*_authorized` decision name.
int is_authorized(struct task_struct *task) {
    if (!task->mm) return 1;
    return cap_admin(task->creds);
}

// NEGATIVE — fail-CLOSED: returns -EPERM on missing precondition.
// Should NOT fire even though function name is decision-class.
int may_access_safe(struct task_struct *task) {
    if (!task->mm) return -1;
    int dumpable = get_dumpable(task->mm);
    return dumpable == SUID_DUMP_USER ? 0 : -1;
}

// NEGATIVE — no downstream check after the early return. The early
// return guards a benign "no work to do" case, not a security check.
// Heuristic 3 should suppress this.
int can_use_buf(struct task_struct *task) {
    if (!task->mm) return 0;
    return 0;
}
