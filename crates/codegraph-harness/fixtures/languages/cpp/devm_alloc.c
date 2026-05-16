// Test fixture for resource_leaks devm_* false-positive fix.
//
// devm_kzalloc / devm_kmalloc are kernel managed allocators —
// memory is automatically freed when the underlying device is
// detached. They are NOT leaks even though there's no explicit
// devm_kfree. Pre-fix, `body.contains("kmalloc(")` substring-matched
// inside `devm_kmalloc(` and fired a "kmalloc without kfree" finding
// for these idiomatic kernel patterns.
//
// Post-fix, identifier-boundary check on the open match rejects the
// inner substring and only flags genuinely-managed manual kmalloc.

void *devm_kzalloc(void *dev, unsigned long size, int flags);
void *devm_kmalloc(void *dev, unsigned long size, int flags);
void *kmalloc(unsigned long size, int flags);
void kfree(const void *p);

// SAFE: managed allocation, no leak.
void *probe_safe(void *dev) {
    return devm_kzalloc(dev, 64, 0);
}

// SAFE: managed allocation with manual-style sibling.
void *probe_safe_sibling(void *dev) {
    return devm_kmalloc(dev, 32, 0);
}

// LEAK: bare kmalloc without kfree — real leak, should fire.
void *probe_real_leak(void) {
    void *p = kmalloc(16, 0);
    return p;
}
