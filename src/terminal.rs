//! PTY-backed terminal session.
//!
//! Spawns the user's login shell inside a real pseudo-terminal so that
//! programs like `ls --color`, `git log`, `top`, etc. behave exactly as they
//! would in macOS Terminal.app.  Raw bytes (including ANSI escape sequences)
//! are collected in a shared ring-buffer; the UI layer strips / interprets
//! the escape codes for rendering.

use anyhow::Context;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;

// ── Shared state ──────────────────────────────────────────────────────────────

/// Raw bytes received from the PTY (stdout + stderr merged, as a real
/// terminal would see them).  The UI drains this each frame.
pub type RawBuf = Arc<Mutex<Vec<u8>>>;

// ── TerminalSession ───────────────────────────────────────────────────────────

pub struct TerminalSession {
    /// Write end of the PTY – send bytes to the shell.
    writer: Box<dyn Write + Send>,
    /// Shared raw-byte buffer filled by the reader thread.
    pub raw: RawBuf,
    /// Current working directory (best-effort).
    pub cwd: String,
}

impl TerminalSession {
    /// Spawn a new interactive login shell inside a PTY.
    pub fn new() -> anyhow::Result<Self> {
        let pty_system = native_pty_system();

        // Start with a reasonable size; the UI can resize later.
        let pair = pty_system
            .openpty(PtySize {
                rows: 40,
                cols: 220,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty failed")?;

        // Build the shell command (interactive login shell).
        let cmd = build_shell_command();

        // Spawn inside the PTY slave.
        let _child = pair
            .slave
            .spawn_command(cmd)
            .context("spawn shell failed")?;

        // Take the master read/write ends.
        let writer = pair
            .master
            .take_writer()
            .context("take_writer failed")?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .context("try_clone_reader failed")?;

        // Shared ring-buffer (capped at ~1 MB).
        let raw: RawBuf = Arc::new(Mutex::new(Vec::with_capacity(4096)));

        // Reader thread: copy PTY output into the shared buffer.
        {
            let buf = Arc::clone(&raw);
            thread::spawn(move || {
                let mut chunk = [0u8; 4096];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut b) = buf.lock() {
                                b.extend_from_slice(&chunk[..n]);
                                // Keep buffer bounded to ~1 MB
                                const CAP: usize = 1024 * 1024;
                                if b.len() > CAP {
                                    let trim = b.len() - CAP / 2;
                                    *b = b[trim..].to_vec();
                                }
                            }
                        }
                    }
                }
            });
        }

        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        Ok(Self { writer, raw, cwd })
    }

    /// Send raw bytes to the shell (e.g. a command + '\n', or Ctrl+C = 0x03).
    pub fn write_raw(&mut self, data: &[u8]) {
        let _ = self.writer.write_all(data);
        let _ = self.writer.flush();
    }

    /// Send a text line followed by a newline.
    pub fn send_line(&mut self, line: &str) {
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(b'\n');
        self.write_raw(&bytes);
    }

    /// Drain the raw buffer and return its contents.
    pub fn drain_raw(&self) -> Vec<u8> {
        if let Ok(mut b) = self.raw.lock() {
            std::mem::take(&mut *b)
        } else {
            Vec::new()
        }
    }

    /// Resize the PTY (call when the terminal pane is resized).
    #[allow(dead_code)]
    pub fn resize(&self, rows: u16, cols: u16) {
        // portable-pty doesn't expose resize on the writer directly;
        // we'd need to keep a handle to the master.  Left as a no-op for now.
        let _ = (rows, cols);
    }
}

fn build_shell_command() -> CommandBuilder {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let mut cmd = CommandBuilder::new(&shell);
    cmd.arg("-l");
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_shell_command_uses_login_shell_without_prompt_overrides() {
        let cmd = build_shell_command();

        assert!(cmd.get_argv().iter().any(|arg| arg == "-l"));
        assert!(cmd.iter_extra_env_as_str().all(|(key, _)| {
            !matches!(key, "PROMPT" | "PS1" | "RPROMPT" | "RPS1" | "KUBE_PS1_ENABLED")
        }));
    }
}
