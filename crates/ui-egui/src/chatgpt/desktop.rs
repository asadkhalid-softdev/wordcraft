//! Stdio transport for the official Codex app-server. No network or credentials in the UI.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use serde_json::Value;

const MAX_LINE: usize = 2 * 1024 * 1024;

pub(super) struct Client {
    outgoing: SyncSender<Value>,
    pub incoming: Receiver<Result<Value, String>>,
    stop: mpsc::Sender<()>,
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

impl Client {
    #[cfg(test)]
    pub(super) fn test_pair() -> (Self, Receiver<Value>) {
        let (outgoing, writes) = mpsc::sync_channel(32);
        let (_events, incoming) = mpsc::sync_channel(64);
        let (stop, _stopped) = mpsc::channel();
        (Self { outgoing, incoming, stop }, writes)
    }

    pub fn start(executable: &str, ctx: egui::Context) -> Result<Self, String> {
        let executable = executable.to_owned();
        let (outgoing, writes) = mpsc::sync_channel::<Value>(32);
        let (events, incoming) = mpsc::sync_channel(64);
        let (stop, stopped) = mpsc::channel();
        std::thread::Builder::new()
            .name("wordcraft-chatgpt".into())
            .spawn(move || {
                let report = |error: String| {
                    let _ = events.send(Err(error));
                    ctx.request_repaint();
                };
                let result = supervise(&executable, &ctx, &events, writes, stopped);
                if let Err(error) = result {
                    report(error);
                }
            })
            .map_err(|e| format!("Couldn't start ChatGPT worker: {e}"))?;
        Ok(Self { outgoing, incoming, stop })
    }

    pub fn send(&self, value: Value) -> Result<(), String> {
        self.outgoing.try_send(value).map_err(|_| "ChatGPT connection is unavailable or busy. Reconnect and try again.".into())
    }
}

fn supervise(
    executable: &str,
    ctx: &egui::Context,
    events: &SyncSender<Result<Value, String>>,
    writes: Receiver<Value>,
    stopped: Receiver<()>,
) -> Result<(), String> {
    let home = wordcraft_control_key::settings_dir().ok_or("Couldn't locate WordCraft's settings folder")?.join("chatgpt");
    std::fs::create_dir_all(&home).map_err(|e| format!("Couldn't create ChatGPT settings: {e}"))?;
    let mut command = Command::new(executable);
    command
        .args([
            "-c",
            "forced_login_method=\"chatgpt\"",
            "-c",
            "features.shell_tool=false",
            "-c",
            "features.multi_agent=false",
            "-c",
            "features.apps=false",
            "-c",
            "web_search=\"disabled\"",
            "app-server",
        ])
        .env("CODEX_HOME", &home)
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN")
        .current_dir(&home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Launch the native executable directly. No console flash and no shell interpolation.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Couldn't start Codex ({e}). Install the native Codex CLI, or choose its executable in Chat settings."))?;
    let result = (|| {
        let mut stdin = child.stdin.take().ok_or("Codex did not provide stdin")?;
        let stdout = child.stdout.take().ok_or("Codex did not provide stdout")?;
        let reader_events = events.clone();
        let wake = ctx.clone();
        std::thread::Builder::new()
            .name("chatgpt-read".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let value = read_message(&mut reader);
                    let end = value.is_err();
                    if reader_events.send(value).is_err() {
                        break;
                    }
                    wake.request_repaint();
                    if end {
                        break;
                    }
                }
            })
            .map_err(|e| format!("Couldn't start ChatGPT reader: {e}"))?;
        let writer_events = events.clone();
        let wake = ctx.clone();
        std::thread::Builder::new()
            .name("chatgpt-write".into())
            .spawn(move || {
                while let Ok(value) = writes.recv() {
                    let result = serde_json::to_writer(&mut stdin, &value)
                        .map_err(std::io::Error::other)
                        .and_then(|_| stdin.write_all(b"\n"))
                        .and_then(|_| stdin.flush());
                    if result.is_err() {
                        let _ = writer_events.send(Err("ChatGPT connection closed while sending a request".into()));
                        wake.request_repaint();
                        break;
                    }
                }
            })
            .map_err(|e| format!("Couldn't start ChatGPT writer: {e}"))?;
        loop {
            match stopped.recv_timeout(Duration::from_millis(50)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                return Err(format!("Codex exited ({status}). Check the executable and reconnect."));
            }
        }
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}

/// Bound allocation before parsing, including a peer that never sends a newline.
fn read_message(reader: &mut impl BufRead) -> Result<Value, String> {
    let mut bytes = Vec::new();
    loop {
        let chunk = reader.fill_buf().map_err(|e| format!("Couldn't read from Codex: {e}"))?;
        if chunk.is_empty() {
            return Err("ChatGPT connection closed".into());
        }
        let end = chunk.iter().position(|b| *b == b'\n').map(|n| n + 1);
        let count = end.unwrap_or(chunk.len());
        if count > MAX_LINE.saturating_sub(bytes.len()) {
            return Err("Codex response exceeded the message size limit".into());
        }
        if let Some(part) = chunk.get(..count) {
            bytes.extend_from_slice(part);
        }
        reader.consume(count);
        if end.is_some() {
            break;
        }
    }
    serde_json::from_slice(&bytes).map_err(|_| "Codex returned an invalid protocol message; update the Codex CLI".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_reader_bounds_unterminated_messages_and_rejects_bad_json() {
        assert!(read_message(&mut &b"{\"id\":1}\n"[..]).is_ok());
        assert!(read_message(&mut &b"bad json\n"[..]).is_err());
        assert!(read_message(&mut &b"\xff\n"[..]).is_err());
        assert!(read_message(&mut &b""[..]).is_err());
        assert!(read_message(&mut vec![b'x'; MAX_LINE + 1].as_slice()).is_err());
    }
}
