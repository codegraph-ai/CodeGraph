// Rust Shape B fixture (bounty 2026-05-17 calibration). Rust
// user-space teardowns don't use kernel-style kfree/mmput/fput — they
// use `.take()`, `self.x = None`, `mem::replace`, and method-call
// teardowns like `.shutdown()` / `.close()` / `.drop()` on field
// receivers. Without language-specific clear-site patterns the
// cleanup_pattern mode returned 0 matches across runc/openvmm/kata.
//
// Fixture layout:
//   - close_session         = anchor (3 clear-sites: shutdown, close, take)
//   - cleanup_resources     = sibling teardown (2 clear-sites)
//   - drop                  = Drop impl with 2 clear-sites
//   - close_one_fd          = teardown name but only 1 clear-site → EXCLUDED
//   - compute_hash          = non-teardown name → EXCLUDED

#![allow(dead_code)]

struct Channel;
struct Backend;
struct Token;

impl Channel {
    fn shutdown(&mut self) {}
    fn close(&mut self) {}
}
impl Backend {
    fn destroy(&mut self) {}
}

pub struct Session {
    channel: Option<Channel>,
    backend: Option<Backend>,
    token: Option<Token>,
}

// ANCHOR: teardown clearing three attrs via mixed shapes —
// method-call shutdown + close, then field nulling via take().
pub fn close_session(s: &mut Session) {
    if let Some(c) = s.channel.as_mut() {
        c.shutdown();
    }
    if let Some(c) = s.channel.as_mut() {
        c.close();
    }
    s.token.take();
}

// MATCH: teardown name + 2 clear-sites (destroy + take).
pub fn cleanup_resources(s: &mut Session) {
    if let Some(b) = s.backend.as_mut() {
        b.destroy();
    }
    s.channel.take();
}

impl Drop for Session {
    // MATCH: Drop impl body has 2 clear-sites (take + = None).
    fn drop(&mut self) {
        self.channel.take();
        self.backend = None;
    }
}

// EXCLUDED: teardown name but only 1 clear-site.
pub fn close_one_fd(s: &mut Session) {
    s.token.take();
}

// EXCLUDED: non-teardown name.
pub fn compute_hash(s: &Session) -> u32 {
    if s.channel.is_some() { 1 } else { 0 }
}
