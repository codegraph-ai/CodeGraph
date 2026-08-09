// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Makes the LSP `exit` notification actually terminate the process.
//!
//! tower-lsp 0.20's read loop only notices that the service has exited when the
//! *next* message arrives: `exit` is dispatched through `service.call()`, which
//! flips the state to `Exited` but does not break the loop, so
//! `framed_stdin.next().await` then blocks until another message shows up or
//! stdin reaches EOF. Measured against the real engine: `shutdown` answered in
//! 0.00s, the process was still alive 120 seconds after `exit`, and terminated
//! only when stdin closed.
//!
//! Both editor clients hide this by force-killing the engine - vscode-
//! languageclient after its stop timeout, LSP4IJ through
//! `ExecutionManagerImpl.stopProcess`. That is the important detail: the status
//! quo is already an abrupt kill, so returning from `main` a moment after
//! `shutdown` is *gentler* than what happens today, not riskier. It also fixes
//! the case no client covers - anything holding the pipe open after `exit`,
//! such as a supervisor reusing stdio, where the engine would otherwise linger
//! holding an entire graph in memory.
//!
//! The LSP specification says a client must send `exit` after the `shutdown`
//! response, and that no other request is valid in between, so treating
//! `shutdown` as the signal is safe: there is nothing legitimate left to serve.

use std::time::Duration;
use tokio::sync::Notify;

/// Signalled by the backend's `shutdown` handler.
static SHUTDOWN_REQUESTED: Notify = Notify::const_new();

/// How long to keep serving after `shutdown` before giving up on `exit`.
///
/// A compliant client sends `exit` immediately, and tower-lsp handles it
/// without waking the read loop, so this is really just slack for in-flight
/// work to settle before the runtime is dropped.
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// Record that the client asked the server to shut down.
pub fn request_shutdown() {
    signal(&SHUTDOWN_REQUESTED);
}

/// Resolves once `shutdown` has been received and the grace period has passed.
///
/// Intended to be raced against tower-lsp's `serve()` future.
pub async fn wait_for_exit() {
    wait(&SHUTDOWN_REQUESTED).await;
}

/// `notify_one` rather than `notify_waiters`: it stores a permit when nobody is
/// waiting yet, so a `shutdown` that arrives before `main` reaches the waiter
/// still counts. Losing it would reintroduce the hang this module exists to fix.
fn signal(notify: &Notify) {
    notify.notify_one();
}

/// The waiting half, taking its [`Notify`] so the behaviour can be tested
/// without the process-global one - tests share a binary, and a permit stored
/// by one test would otherwise satisfy another's wait.
async fn wait(notify: &Notify) {
    notify.notified().await;
    tracing::info!("[lsp_exit] shutdown received; exiting in {EXIT_GRACE:?}");
    tokio::time::sleep(EXIT_GRACE).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn waits_for_shutdown_before_resolving() {
        // Without a shutdown request this must never resolve, or the engine
        // would quit on its own while a client is still using it.
        let notify = Notify::new();
        tokio::select! {
            () = wait(&notify) => panic!("resolved without a shutdown request"),
            () = tokio::time::sleep(EXIT_GRACE * 100) => {}
        }
    }

    #[tokio::test(start_paused = true)]
    async fn resolves_after_shutdown_plus_grace() {
        let notify = Notify::new();
        signal(&notify);

        tokio::time::timeout(EXIT_GRACE * 2, wait(&notify))
            .await
            .expect("should resolve once shutdown was requested");
    }

    #[tokio::test(start_paused = true)]
    async fn does_not_resolve_before_the_grace_period() {
        // Exiting the instant `shutdown` returns would cut off the response
        // still being written, and any work settling behind it.
        let notify = Notify::new();
        signal(&notify);

        tokio::select! {
            () = wait(&notify) => panic!("exited before the grace period elapsed"),
            () = tokio::time::sleep(EXIT_GRACE / 2) => {}
        }
    }

    #[tokio::test(start_paused = true)]
    async fn signal_sent_before_waiting_is_not_lost() {
        // The backend can call shutdown before main reaches the waiter; a
        // dropped signal here would reintroduce the hang this module exists to
        // fix.
        let notify = Notify::new();
        signal(&notify);
        tokio::time::sleep(EXIT_GRACE * 5).await;

        tokio::time::timeout(EXIT_GRACE * 2, wait(&notify))
            .await
            .expect("a signal sent before the waiter existed must still count");
    }
}
