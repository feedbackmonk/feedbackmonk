//! Scrubbing writer chokepoint.
//!
//! The brief's Contract C9 mandates that every emitted log line passes
//! through the canonical 20-pattern scrubber. The naive shape — a custom
//! `tracing_subscriber::Layer` that re-records each event's string fields via
//! a visitor — is fragile (non-string fields lose typing, the visitor must
//! mirror every `tracing-subscriber` API revision, and field-level scrubbing
//! never sees what the formatter chose to render).
//!
//! We chokepoint at the WRITE boundary instead: `ScrubbingMakeWriter` wraps
//! the underlying stdout writer, accumulates each event's formatted bytes
//! (`fmt::Layer` calls `write_all` then `flush` per event), runs `scrub()`
//! over the accumulated UTF-8, and forwards the scrubbed bytes downstream.
//! Every log line — JSON or plain — passes through the scrubber regardless
//! of which field carried the PII. The same guarantee the brief asks for,
//! built on a more stable seam.
//!
//! From the `pii-scrub-audit` Probe A perspective, this file is INSIDE
//! `crates/feedbackmonk-tracing/`, so any `impl Layer<...> for ...` we add here
//! is allowed; the probe only forbids them OUTSIDE the crate.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;

use crate::scrubber::scrub;

/// Production scrubbing writer factory — wraps `io::stdout`. The active
/// chokepoint for `install_global_subscriber`.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct StdoutScrubbing;

/// Per-event writer for the stdout chokepoint. Buffers writes, scrubs on
/// flush (or drop), forwards to the locked stdout handle.
pub(crate) struct StdoutScrubGuard {
    pending: Vec<u8>,
}

impl Write for StdoutScrubGuard {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        flush_pending(&mut self.pending, |scrubbed| {
            let stdout = io::stdout();
            let mut h = stdout.lock();
            h.write_all(scrubbed)?;
            h.flush()
        })
    }
}

impl Drop for StdoutScrubGuard {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl<'a> MakeWriter<'a> for StdoutScrubbing {
    type Writer = StdoutScrubGuard;
    fn make_writer(&'a self) -> Self::Writer {
        StdoutScrubGuard {
            pending: Vec::new(),
        }
    }
}

/// Test-only scrubbing writer factory — accumulates bytes in a shared
/// in-memory `Vec<u8>` so integration tests can install a real
/// `tracing-subscriber` subscriber, emit events, and assert the recorded
/// bytes are PII-free.
#[derive(Debug, Default, Clone)]
pub struct SharedBufferScrubbing {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl SharedBufferScrubbing {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot the bytes that have been recorded so far. Returns a clone so
    /// the underlying buffer continues to accumulate.
    #[must_use]
    pub fn snapshot(&self) -> Vec<u8> {
        self.buf.lock().expect("snapshot lock").clone()
    }
}

/// Per-event writer for the test buffer. Scrubs on flush + drop.
pub struct SharedBufferGuard {
    buf: Arc<Mutex<Vec<u8>>>,
    pending: Vec<u8>,
}

impl Write for SharedBufferGuard {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        let buf = Arc::clone(&self.buf);
        flush_pending(&mut self.pending, |scrubbed| {
            let mut g = buf.lock().map_err(|_| io::Error::other("poisoned"))?;
            g.extend_from_slice(scrubbed);
            Ok(())
        })
    }
}

impl Drop for SharedBufferGuard {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl<'a> MakeWriter<'a> for SharedBufferScrubbing {
    type Writer = SharedBufferGuard;
    fn make_writer(&'a self) -> Self::Writer {
        SharedBufferGuard {
            buf: Arc::clone(&self.buf),
            pending: Vec::new(),
        }
    }
}

/// Apply `scrub()` to whatever has accumulated in `pending` and hand the
/// resulting bytes to `sink`. Empties `pending` so a follow-up flush is a
/// no-op. UTF-8-invalid sequences (rare; `tracing-subscriber` emits UTF-8
/// by contract) are forwarded unscrubbed.
fn flush_pending(
    pending: &mut Vec<u8>,
    sink: impl FnOnce(&[u8]) -> io::Result<()>,
) -> io::Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let bytes_owned = std::mem::take(pending);
    let scrubbed = match std::str::from_utf8(&bytes_owned) {
        Ok(s) => scrub(s).into_bytes(),
        Err(_) => bytes_owned,
    };
    sink(&scrubbed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_buffer_scrubs_email() {
        let mw = SharedBufferScrubbing::new();
        {
            let mut w = mw.make_writer();
            w.write_all(b"contact alice@example.com today\n").unwrap();
            w.flush().unwrap();
        }
        let out = String::from_utf8(mw.snapshot()).unwrap();
        assert_eq!(out, "contact [email] today\n");
    }

    #[test]
    fn shared_buffer_scrubs_on_drop_without_explicit_flush() {
        let mw = SharedBufferScrubbing::new();
        {
            let mut w = mw.make_writer();
            w.write_all(b"ip 192.168.1.1 here").unwrap();
        }
        let out = String::from_utf8(mw.snapshot()).unwrap();
        assert_eq!(out, "ip [ip] here");
    }

    #[test]
    fn shared_buffer_handles_multiple_writes_per_event() {
        let mw = SharedBufferScrubbing::new();
        {
            let mut w = mw.make_writer();
            w.write_all(b"user ").unwrap();
            w.write_all(b"alice@example.com ").unwrap();
            w.write_all(b"connected\n").unwrap();
            w.flush().unwrap();
        }
        let out = String::from_utf8(mw.snapshot()).unwrap();
        assert_eq!(out, "user [email] connected\n");
    }

    #[test]
    fn shared_buffer_pii_free_text_unchanged() {
        let mw = SharedBufferScrubbing::new();
        {
            let mut w = mw.make_writer();
            w.write_all(b"feedbackmonk-api listening\n").unwrap();
            w.flush().unwrap();
        }
        assert_eq!(
            String::from_utf8(mw.snapshot()).unwrap(),
            "feedbackmonk-api listening\n"
        );
    }
}
