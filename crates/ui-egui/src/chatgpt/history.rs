//! Private, atomic, per-document chat archives. File I/O runs on a dedicated worker.
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{Message, Reference};

const VERSION: u32 = 1;
const MAX_FILE: usize = 32 * 1024 * 1024;
pub(super) const MAX_MESSAGES: usize = 8192;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Archive {
    version: u32,
    pub messages: Vec<Message>,
    pub draft: String,
    pub references: Vec<Reference>,
    pub next_reference: u64,
    pub memory: Vec<Value>,
}
impl Archive {
    pub fn capture(state: &super::State) -> Self {
        Self {
            version: VERSION,
            messages: state.messages.clone(),
            draft: state.draft.clone(),
            references: state.references.clone(),
            next_reference: state.next_reference,
            memory: state.memory.clone(),
        }
    }
    fn validate(&self) -> Result<(), String> {
        if self.version != VERSION
            || self.messages.len() > MAX_MESSAGES
            || self.draft.len() > super::MAX_PROMPT
            || self.references.len() > super::references::MAX_REFERENCES
            || !valid_memory(&self.memory)
        {
            return Err("Saved chat has an unsupported format or exceeds its limits".into());
        }
        for message in &self.messages {
            if !matches!(message.role.as_str(), "You" | "ChatGPT" | "Tool")
                || message.text.len() > super::MAX_TEXT
                || message.item.as_ref().is_some_and(|id| id.len() > 200)
                || message.references.len() > super::references::MAX_REFERENCES
            {
                return Err("Saved chat contains an invalid message".into());
            }
        }
        for r in self.references.iter().chain(self.messages.iter().flat_map(|m| m.references.iter())) {
            r.validate_saved()?;
            if r.id > self.next_reference {
                return Err("Saved chat reference identity is invalid".into());
            }
        }
        Ok(())
    }
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.draft.is_empty() && self.references.is_empty() && self.memory.is_empty()
    }
}

pub(super) fn valid_memory(memory: &[Value]) -> bool {
    memory.len() <= 8
        && memory.iter().all(|v| {
            v["role"] == "user" && v["content"].as_str().is_some_and(|text| text.len() <= 220_000) && v.as_object().is_some_and(|o| o.len() == 2)
        })
        && serde_json::to_vec(memory).is_ok_and(|bytes| bytes.len() <= 224_000)
}

pub(super) fn key(path: &Path) -> Result<String, String> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_err(|_| "Could not locate the document for chat history")?.join(path)
    };
    let absolute = fs::canonicalize(&absolute).unwrap_or(absolute);
    let text = absolute.to_string_lossy();
    #[cfg(windows)]
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_lowercase();
    Ok(Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(super) enum Task {
    Load { key: String, ticket: u64 },
    Save { key: String, archive: Archive, reply: Option<SyncSender<Result<(), String>>> },
    Delete { key: String },
}
pub(super) enum Event {
    Loaded { key: String, ticket: u64, archive: Result<Option<Archive>, String> },
    Written { key: String, result: Result<(), String>, cleared: bool },
}
pub(super) struct Store {
    sender: Option<SyncSender<Task>>,
    pub events: Receiver<Event>,
    worker: Option<JoinHandle<()>>,
}
impl Store {
    pub fn start(root: PathBuf, ctx: egui::Context) -> Result<Self, String> {
        let (sender, jobs) = mpsc::sync_channel(4);
        let (events, receiver) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("wordcraft-chat-history".into())
            .spawn(move || {
                let mut disk = Disk { root, expected: BTreeMap::new() };
                for task in jobs {
                    let event = match task {
                        Task::Load { key, ticket } => {
                            let archive = disk.load(&key);
                            Event::Loaded { key, ticket, archive }
                        }
                        Task::Save { key, archive, reply } => {
                            let result = disk.save(&key, &archive);
                            if let Some(reply) = reply {
                                let _ = reply.send(result.clone());
                            }
                            Event::Written { result, key, cleared: false }
                        }
                        Task::Delete { key } => Event::Written { result: disk.delete(&key), key, cleared: true },
                    };
                    if events.send(event).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .map_err(|_| "Could not start private chat storage")?;
        Ok(Self { sender: Some(sender), events: receiver, worker: Some(worker) })
    }
    pub fn send(&self, task: Task, closing: bool) -> Result<(), String> {
        let sender = self.sender.as_ref().ok_or("Chat storage is unavailable")?;
        if closing {
            sender.send(task).map_err(|_| "Could not save chat before closing".into())
        } else {
            sender.try_send(task).map_err(|_| "Chat storage is busy; waiting to save the conversation".into())
        }
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        self.sender = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(super) struct Persistence {
    pub enabled: bool,
    pub root: Option<PathBuf>,
    pub path: Option<PathBuf>,
    pub store: Option<Store>,
    pub binding: Option<(u64, Option<String>)>,
    pub ticket: u64,
    pub loading: bool,
    pub switching: bool,
    pub pending_writes: usize,
    pub blocked: bool,
    pub dirty: bool,
    pub saved_at: f64,
    pub error: Option<String>,
}
impl Default for Persistence {
    fn default() -> Self {
        Self {
            enabled: !cfg!(test),
            root: None,
            path: None,
            store: None,
            binding: None,
            ticket: 0,
            loading: false,
            switching: false,
            pending_writes: 0,
            blocked: false,
            dirty: false,
            saved_at: 0.0,
            error: None,
        }
    }
}
impl Persistence {
    pub fn start(&mut self, ctx: &egui::Context) -> Result<(), String> {
        if self.store.is_none() {
            let root = self
                .root
                .clone()
                .or_else(|| wordcraft_control_key::settings_dir().map(|p| p.join("chatgpt-history")))
                .ok_or("Could not locate private chat storage")?;
            self.store = Some(Store::start(root, ctx.clone())?);
        }
        Ok(())
    }
    pub fn send(&self, task: Task, closing: bool) -> Result<(), String> {
        self.store.as_ref().ok_or("Chat storage is unavailable")?.send(task, closing)
    }
}

struct Disk {
    root: PathBuf,
    expected: BTreeMap<String, Option<[u8; 32]>>,
}
impl Disk {
    fn paths(&self, key: &str) -> Result<(PathBuf, File), String> {
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid chat identity".into());
        }
        fs::create_dir_all(&self.root).map_err(|_| "Could not create private chat storage")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700)).map_err(|_| "Could not protect chat storage")?;
        }
        #[cfg(windows)]
        super::auth::protect(&self.root)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(format!("{key}.lock")))
            .map_err(|_| "Could not open the saved chat lock")?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| "Another WordCraft window is saving this chat; try again shortly")?;
        Ok((self.root.join(format!("{key}.json")), lock))
    }
    fn bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Could not read saved chat; check profile permissions".into()),
        };
        if !file.metadata().is_ok_and(|m| m.is_file() && m.len() <= MAX_FILE as u64) {
            return Err("Saved chat is not a regular file or exceeds 32 MiB".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE as u64 + 1).read_to_end(&mut bytes).map_err(|_| "Could not read saved chat")?;
        if bytes.len() > MAX_FILE {
            return Err("Saved chat exceeds 32 MiB".into());
        }
        Ok(Some(bytes))
    }
    fn digest(bytes: &Option<Vec<u8>>) -> Option<[u8; 32]> {
        bytes.as_ref().map(|b| Sha256::digest(b).into())
    }
    fn load(&mut self, key: &str) -> Result<Option<Archive>, String> {
        let (path, _lock) = self.paths(key)?;
        let bytes = Self::bytes(&path)?;
        let archive = bytes
            .as_ref()
            .map(|bytes| {
                let archive: Archive = serde_json::from_slice(bytes).map_err(|_| "Could not decode saved chat; it has not been overwritten")?;
                archive.validate()?;
                Ok::<_, String>(archive)
            })
            .transpose()?;
        if self.expected.len() >= 4096 && !self.expected.contains_key(key) {
            return Err("Chat storage has reached its open-document limit; restart WordCraft".into());
        }
        self.expected.insert(key.into(), Self::digest(&bytes));
        Ok(archive)
    }
    fn save(&mut self, key: &str, archive: &Archive) -> Result<(), String> {
        archive.validate()?;
        let (path, _lock) = self.paths(key)?;
        let current = Self::bytes(&path)?;
        if Self::digest(&current) != self.expected.get(key).copied().flatten() {
            return Err(
                "Another WordCraft window changed this chat. Existing saved history was kept; your current messages remain in this window".into()
            );
        }
        let mut file = tempfile::NamedTempFile::new_in(&self.root).map_err(|_| "Could not create private chat storage")?;
        super::auth::protect(file.path())?;
        let mut writer = Limited { file: file.as_file_mut(), written: 0 };
        serde_json::to_writer(&mut writer, archive).map_err(|_| "Could not save chat (maximum 32 MiB); existing history was kept")?;
        file.flush().and_then(|()| file.as_file().sync_all()).map_err(|_| "Could not synchronize saved chat")?;
        file.persist(&path).map_err(|_| "Could not replace saved chat; existing history was kept")?;
        self.expected.insert(key.into(), Self::digest(&Self::bytes(&path)?));
        Ok(())
    }
    fn delete(&mut self, key: &str) -> Result<(), String> {
        let (path, _lock) = self.paths(key)?;
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Could not delete saved chat; check profile permissions and try Clear conversation again".into()),
        }
        self.expected.insert(key.into(), None);
        Ok(())
    }
}
struct Limited<'a> {
    file: &'a mut File,
    written: usize,
}
impl Write for Limited<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_FILE.saturating_sub(self.written) {
            return Err(std::io::Error::other("chat too large"));
        }
        let n = self.file.write(bytes)?;
        self.written = self.written.saturating_add(n);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

fn enqueue_snapshot(state: &super::State, closing: bool) -> Result<bool, String> {
    let p = &state.persistence;
    if !p.enabled || p.loading || p.blocked || (!p.dirty && !closing) {
        return Ok(false);
    }
    let Some((_, Some(key))) = &p.binding else { return Ok(false) };
    let archive = Archive::capture(state);
    if archive.is_empty() && !p.dirty {
        return Ok(false);
    }
    if closing {
        let (reply, result) = mpsc::sync_channel(1);
        p.send(Task::Save { key: key.clone(), archive, reply: Some(reply) }, true)?;
        result.recv_timeout(std::time::Duration::from_secs(10)).map_err(|_| "Timed out saving chat before closing")?.map(|()| true)
    } else {
        p.send(Task::Save { key: key.clone(), archive, reply: None }, false).map(|()| true)
    }
}

/// Also used by native shutdown paths, including macOS's process-exit fallback.
pub(super) fn flush(state: &super::State) -> Result<(), String> {
    enqueue_snapshot(state, true).map(|_| ())
}

pub(super) fn erase(state: &mut super::State) -> Result<(), String> {
    if state.persistence.enabled
        && let Some((_, Some(key))) = &state.persistence.binding
    {
        state.persistence.send(Task::Delete { key: key.clone() }, false)?;
        state.persistence.pending_writes = state.persistence.pending_writes.saturating_add(1);
    }
    state.persistence.ticket = state.persistence.ticket.saturating_add(1);
    state.persistence.loading = false;
    state.persistence.switching = false;
    state.persistence.blocked = false;
    state.persistence.error = None;
    state.clear();
    state.persistence.dirty = false;
    Ok(())
}

pub(super) fn poll(app: &mut crate::WordApp, ctx: &egui::Context, force: bool) -> bool {
    if !app.chatgpt.persistence.enabled {
        return true;
    }
    if !force && !app.chatgpt.open && app.chatgpt.persistence.binding.is_none() && app.chatgpt.messages.is_empty() {
        return true;
    }
    match sync(app, ctx) {
        Ok(()) => !app.chatgpt.persistence.switching,
        Err(error) => {
            app.chatgpt.persistence.error = Some(error);
            false
        }
    }
}
fn sync(app: &mut crate::WordApp, ctx: &egui::Context) -> Result<(), String> {
    let document = app.session.document_id();
    let path = app.session.path.clone();
    while let Some(event) = app.chatgpt.persistence.store.as_ref().and_then(|store| store.events.try_recv().ok()) {
        let current_key = app.chatgpt.persistence.binding.as_ref().and_then(|(_, key)| key.as_deref());
        match event {
            Event::Loaded { key, ticket, archive } if current_key == Some(key.as_str()) && ticket == app.chatgpt.persistence.ticket => {
                app.chatgpt.persistence.loading = false;
                match archive {
                    Ok(Some(archive)) => {
                        app.chatgpt.messages = archive.messages;
                        app.chatgpt.draft = archive.draft;
                        app.chatgpt.references = archive.references;
                        app.chatgpt.next_reference = archive.next_reference;
                        app.chatgpt.memory = archive.memory;
                        app.chatgpt.visible_messages = 256;
                        for message in &mut app.chatgpt.messages {
                            message.item = None;
                            for r in &mut message.references {
                                r.restore_document(document);
                            }
                        }
                        for r in &mut app.chatgpt.references {
                            r.restore_document(document);
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        app.chatgpt.persistence.blocked = true;
                        app.chatgpt.persistence.error = Some(error);
                    }
                }
            }
            Event::Written { key, result, cleared } => {
                app.chatgpt.persistence.pending_writes = app.chatgpt.persistence.pending_writes.saturating_sub(1);
                // Errors for a document just closed must still be surfaced rather than losing history silently.
                if cleared && result.is_ok() && current_key == Some(key.as_str()) {
                    app.chatgpt.persistence.blocked = false;
                    app.chatgpt.persistence.error = None;
                } else if let Err(error) = result {
                    app.chatgpt.persistence.error = Some(error);
                    if current_key == Some(key.as_str()) {
                        app.chatgpt.persistence.blocked = true;
                    }
                } else if current_key == Some(key.as_str()) && !app.chatgpt.persistence.blocked {
                    app.chatgpt.persistence.error = None;
                }
            }
            _ => {}
        }
    }
    let changed_document = app.chatgpt.persistence.binding.as_ref().is_none_or(|(id, _)| *id != document);
    let changed_path = app.chatgpt.persistence.path != path;
    if changed_document || changed_path {
        if app.chatgpt.persistence.blocked {
            return Err("The previous chat could not be saved. Its messages are still in this window; resolve the storage error before switching conversations.".into());
        }
        if app.chatgpt.persistence.pending_writes > 0 {
            app.chatgpt.persistence.switching = true;
            return Ok(());
        }
        if app.chatgpt.persistence.dirty && app.chatgpt.persistence.binding.as_ref().is_some_and(|(_, key)| key.is_some()) {
            let queued = enqueue_snapshot(&app.chatgpt, false)?;
            app.chatgpt.persistence.dirty = false;
            if queued {
                app.chatgpt.persistence.pending_writes = 1;
                app.chatgpt.persistence.switching = true;
                return Ok(());
            }
        }
        app.chatgpt.persistence.switching = false;
        let key = path.as_deref().map(key).transpose()?;
        let ticket = app.chatgpt.persistence.ticket.checked_add(1).ok_or("Restart WordCraft to load saved chat")?;
        if key.is_some() {
            app.chatgpt.persistence.start(ctx)?;
        }
        let copying = !changed_document && !Archive::capture(&app.chatgpt).is_empty();
        if let Some(key) = &key {
            if copying {
                app.chatgpt.persistence.send(Task::Save { key: key.clone(), archive: Archive::capture(&app.chatgpt), reply: None }, false)?;
                app.chatgpt.persistence.pending_writes = app.chatgpt.persistence.pending_writes.saturating_add(1);
            } else {
                app.chatgpt.persistence.send(Task::Load { key: key.clone(), ticket }, false)?;
            }
        }
        if changed_document {
            app.chatgpt.clear();
        }
        let p = &mut app.chatgpt.persistence;
        p.binding = Some((document, key.clone()));
        p.path = path;
        p.ticket = ticket;
        p.loading = key.is_some() && !copying;
        p.blocked = false;
        p.error = None;
        p.dirty = false;
        app.chatgpt.document = Some(document);
    }
    if app.chatgpt.persistence.dirty {
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
    let now = ctx.input(|i| i.time);
    if app.chatgpt.persistence.dirty
        && !app.chatgpt.persistence.loading
        && !app.chatgpt.persistence.blocked
        && now - app.chatgpt.persistence.saved_at >= 1.0
    {
        let queued = enqueue_snapshot(&app.chatgpt, false)?;
        if queued {
            app.chatgpt.persistence.pending_writes = app.chatgpt.persistence.pending_writes.saturating_add(1);
        }
        app.chatgpt.persistence.dirty = false;
        app.chatgpt.persistence.saved_at = now;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn storage_errors_do_not_prevent_reopening_the_sidebar() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("current.docx");
        let mut app = app(temp.path(), &path);
        app.chatgpt.open = false;
        app.chatgpt.persistence.binding = Some((0, Some(key(&path).unwrap())));
        app.chatgpt.persistence.path = Some(path);
        app.chatgpt.persistence.blocked = true;
        super::super::command(&mut app, "ui.chatgpt.toggle", &json!({"value":true})).unwrap().unwrap();
        assert!(app.chatgpt.open);
        assert!(app.chatgpt.persistence.error.is_some());
        super::super::command(&mut app, "ui.chatgpt.settings", &json!({})).unwrap().unwrap();
        assert!(app.chatgpt.settings_open);
    }
    #[test]
    fn successful_clear_resolves_an_older_queued_save_error() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cleared.docx");
        let mut app = app(temp.path(), &path);
        let id = key(&path).unwrap();
        let (sender, events) = mpsc::channel();
        sender.send(Event::Written { key: id.clone(), result: Err("Older save failed".into()), cleared: false }).unwrap();
        sender.send(Event::Written { key: id.clone(), result: Ok(()), cleared: true }).unwrap();
        let p = &mut app.chatgpt.persistence;
        p.binding = Some((app.session.document_id(), Some(id)));
        p.path = Some(path);
        p.pending_writes = 2;
        p.store = Some(Store { sender: None, events, worker: None });
        assert!(poll(&mut app, &egui::Context::default(), true));
        assert!(app.chatgpt.history_ready());
        assert_eq!(app.chatgpt.persistence.pending_writes, 0);
    }
    #[test]
    fn queued_save_then_clear_cannot_restore_old_history() {
        let temp = tempfile::tempdir().unwrap();
        let id = key(&temp.path().join("queued.docx")).unwrap();
        let store = Store::start(temp.path().into(), egui::Context::default()).unwrap();
        store.send(Task::Save { key: id.clone(), archive: archive(), reply: None }, true).unwrap();
        store.send(Task::Delete { key: id.clone() }, true).unwrap();
        store.send(Task::Load { key: id, ticket: 1 }, true).unwrap();
        let timeout = std::time::Duration::from_secs(15);
        assert!(matches!(store.events.recv_timeout(timeout).unwrap(), Event::Written { result: Ok(()), .. }));
        assert!(matches!(store.events.recv_timeout(timeout).unwrap(), Event::Written { result: Ok(()), .. }));
        assert!(matches!(store.events.recv_timeout(timeout).unwrap(), Event::Loaded { archive: Ok(None), .. }));
    }
    #[test]
    fn deleting_an_unsent_draft_is_persisted() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("draft.docx");
        let ctx = egui::Context::default();
        let mut first = app(temp.path(), &path);
        settle(&mut first, &ctx);
        first.chatgpt.draft = "Temporary draft".into();
        first.chatgpt.mark_history();
        flush(&first.chatgpt).unwrap();
        first.chatgpt.draft.clear();
        first.chatgpt.mark_history();
        flush(&first.chatgpt).unwrap();
        drop(first);
        let mut reopened = app(temp.path(), &path);
        settle(&mut reopened, &ctx);
        assert!(reopened.chatgpt.draft.is_empty());
    }
    use super::*;
    use serde_json::json;
    use wordcraft_doc::{Document, Pos};
    use wordcraft_engine::{Selection, Session};

    fn archive() -> Archive {
        let mut state = super::super::State::default();
        state.push("You", "Remember this discussion".into(), None);
        state.push("ChatGPT", "We agreed to revise the introduction.".into(), None);
        state.draft = "Continue reviewing".into();
        state.memory = vec![json!({"role":"user","content":"Completed: revised heading. Next: introduction."})];
        Archive::capture(&state)
    }
    fn disk(root: &Path) -> Disk {
        Disk { root: root.into(), expected: BTreeMap::new() }
    }
    fn app(root: &Path, path: &Path) -> crate::WordApp {
        let mut session = Session::new(Document::from_text("Hello world"));
        session.path = Some(path.into());
        let mut app = crate::WordApp::new(session, crate::Services::default());
        app.chatgpt.open = true;
        app.chatgpt.persistence = Persistence { enabled: true, root: Some(root.into()), ..Default::default() };
        app
    }
    fn settle(app: &mut crate::WordApp, ctx: &egui::Context) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            poll(app, ctx, true);
            let p = &app.chatgpt.persistence;
            assert!(p.error.is_none(), "{:?}", p.error);
            if !p.loading && !p.switching && p.pending_writes == 0 {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "history worker timed out");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    #[test]
    fn atomic_round_trip_validation_and_private_schema() {
        let temp = tempfile::tempdir().unwrap();
        let id = key(&temp.path().join("document.docx")).unwrap();
        let mut disk = disk(temp.path());
        assert!(disk.load(&id).unwrap().is_none());
        let saved = archive();
        disk.save(&id, &saved).unwrap();
        let path = temp.path().join(format!("{id}.json"));
        let bytes = fs::read(&path).unwrap();
        let restored = disk.load(&id).unwrap().unwrap();
        assert_eq!(restored.messages.len(), 2);
        assert_eq!(restored.draft, saved.draft);
        assert_eq!(restored.memory, saved.memory);
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 6);
        assert!(value.get("account").is_none());
        assert!(value.get("auth_json").is_none());
        let mut invalid = saved;
        invalid.version = 999;
        assert!(disk.save(&id, &invalid).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::write(&path, b"invalid archive").unwrap();
        assert!(disk.load(&id).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"invalid archive");
        disk.delete(&id).unwrap();
        assert!(disk.load(&id).unwrap().is_none());
    }
    #[test]
    fn stale_windows_cannot_overwrite_or_resurrect_cleared_chats() {
        let temp = tempfile::tempdir().unwrap();
        let id = key(&temp.path().join("a.docx")).unwrap();
        assert_ne!(id, key(&temp.path().join("other/a.docx")).unwrap());
        let mut first = disk(temp.path());
        let mut second = disk(temp.path());
        first.load(&id).unwrap();
        second.load(&id).unwrap();
        first.save(&id, &archive()).unwrap();
        assert!(second.save(&id, &archive()).is_err());
        second.load(&id).unwrap();
        first.delete(&id).unwrap();
        assert!(second.save(&id, &archive()).is_err());
        assert!(first.load(&id).unwrap().is_none());
    }
    #[test]
    fn reopening_restores_full_transcript_draft_references_and_resume_context() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("a.docx");
        let ctx = egui::Context::default();
        let mut original = app(temp.path(), &path);
        settle(&mut original, &ctx);
        original.session.sel = Selection { anchor: Pos::body(0, 0), focus: Pos::body(0, 5) };
        let reference = Reference::capture(&original.session, 1).unwrap();
        original.chatgpt.references.push(reference);
        original.chatgpt.next_reference = 1;
        for i in 0..320 {
            original.chatgpt.push("You", format!("Message {i}"), None);
        }
        original.chatgpt.messages.first_mut().unwrap().item = Some("turn-1-0".into());
        original.chatgpt.draft = "Unsent draft".into();
        original.chatgpt.memory = archive().memory;
        original.chatgpt.auth_json = "PRIVATE_CREDENTIAL_SENTINEL".into();
        flush(&original.chatgpt).unwrap();
        drop(original);
        let mut restored = app(temp.path(), &path);
        settle(&mut restored, &ctx);
        assert_eq!(restored.chatgpt.messages.len(), 320);
        assert_eq!(restored.chatgpt.messages.first().unwrap().text, "Message 0");
        assert!(restored.chatgpt.messages.first().unwrap().item.is_none());
        assert_eq!(restored.chatgpt.draft, "Unsent draft");
        assert_eq!(restored.chatgpt.references[0].resolve(&restored.session).unwrap().focus.off, 5);
        let (client, outgoing) = super::super::desktop::Client::test_pair();
        restored.chatgpt.client = Some(client);
        restored.chatgpt.ready = true;
        restored.chatgpt.account = Some("test@example.com".into());
        restored.chatgpt.submit(&ctx, "Continue our review", restored.session.document_id()).unwrap();
        let start = outgoing.try_recv().unwrap();
        assert_eq!(start["method"], "thread/start");
        let resume = start["params"]["resume"].to_string();
        assert!(resume.contains("Message 319"));
        assert!(resume.contains("revised heading"));
        assert!(!resume.contains("PRIVATE_CREDENTIAL_SENTINEL"));
        assert!(valid_memory(start["params"]["resume"].as_array().unwrap()));
        let disk_text = fs::read_to_string(temp.path().join(format!("{}.json", key(&path).unwrap()))).unwrap();
        assert!(!disk_text.contains("PRIVATE_CREDENTIAL_SENTINEL"));
    }
    #[test]
    fn clear_is_durable_and_documents_keep_separate_chats() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a.docx");
        let b = temp.path().join("b.docx");
        let ctx = egui::Context::default();
        let mut app = app(temp.path(), &a);
        settle(&mut app, &ctx);
        app.chatgpt.push("You", "Only in A".into(), None);
        app.session.set_document(Document::from_text("Document B"));
        app.session.path = Some(b);
        settle(&mut app, &ctx);
        assert!(app.chatgpt.messages.is_empty());
        app.chatgpt.push("You", "Only in B".into(), None);
        app.session.set_document(Document::from_text("Document A"));
        app.session.path = Some(a.clone());
        settle(&mut app, &ctx);
        assert_eq!(app.chatgpt.messages[0].text, "Only in A");
        erase(&mut app.chatgpt).unwrap();
        settle(&mut app, &ctx);
        drop(app);
        let mut restored = self::app(temp.path(), &a);
        settle(&mut restored, &ctx);
        assert!(restored.chatgpt.messages.is_empty());
        assert!(restored.chatgpt.memory.is_empty());
    }
    #[test]
    fn saving_untitled_document_keeps_its_conversation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("saved.docx");
        let ctx = egui::Context::default();
        let mut app = app(temp.path(), &path);
        app.session.path = None;
        settle(&mut app, &ctx);
        app.chatgpt.push("You", "Started before saving".into(), None);
        app.session.path = Some(path.clone());
        settle(&mut app, &ctx);
        drop(app);
        let mut restored = self::app(temp.path(), &path);
        settle(&mut restored, &ctx);
        assert_eq!(restored.chatgpt.messages[0].text, "Started before saving");
    }
}
