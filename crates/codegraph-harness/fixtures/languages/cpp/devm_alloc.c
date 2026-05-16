// Test fixture for resource_leaks devm_*/managed-allocator + factory
// pattern fixes.
//
// devm_kzalloc / devm_kmalloc are kernel managed allocators — memory
// is automatically freed when the underlying device is detached.
// They are NOT leaks even without an explicit devm_kfree. Pre-fix,
// `body.contains("kmalloc(")` substring-matched inside `devm_kmalloc(`
// and fired false-positive findings.
//
// Factory-pattern functions allocate and RETURN the pointer —
// ownership transfers to the caller, so a missing free is the
// contract, not a leak. Pre-fix, every `kmalloc()` that ended in
// `return p;` was flagged.
//
// Post-fix only the genuine leak fires (allocates, never returns,
// never frees).

void *devm_kzalloc(void *dev, unsigned long size, int flags);
void *devm_kmalloc(void *dev, unsigned long size, int flags);
void *kmalloc(unsigned long size, int flags);
void kfree(const void *p);
void do_something(void *p);

// SAFE: managed allocation, no leak.
void *probe_safe(void *dev) {
    return devm_kzalloc(dev, 64, 0);
}

// SAFE: managed allocation with manual-style sibling name.
void *probe_safe_sibling(void *dev) {
    return devm_kmalloc(dev, 32, 0);
}

// SAFE: factory pattern — allocates and returns, caller owns.
void *make_record(void) {
    void *p = kmalloc(16, 0);
    return p;
}

// LEAK: bare kmalloc, never returned, never freed — real leak.
void use_and_drop(void) {
    void *p = kmalloc(24, 0);
    do_something(p);
}
