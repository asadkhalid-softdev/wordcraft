# ChatGPT document assistant

> **Last reviewed:** 2026-10-15 · **Last updated:** 2026-10-15 · **Change:** minor (first implementation)

Click **ChatGPT** in the desktop title bar to open the resizable right sidebar. The close button
collapses it without ending the conversation. ChatGPT can read the current document and use every
registered engine command to edit it, including text, formatting, styles, tables, sections,
headers, footers, drawings, references and review features. Edits use normal **Undo**.

## Setup on native Windows

Install the native [Codex CLI](https://learn.chatgpt.com/docs/codex/cli) using its official installer
from PowerShell:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/codex/install.ps1 | iex"
```

Reopen PowerShell, verify `codex --version`, then start WordCraft:

```powershell
cd 'X:\My Data\Projects\reverse-engineered\wordcraft'
cargo run --release -p wordcraft -- --sample
```

Click **ChatGPT → Connect to ChatGPT → Sign in with ChatGPT**. Finish the sign-in in your browser,
then return to WordCraft. No API key is required. Account eligibility, available models and usage
limits are managed by OpenAI. This is a local, open-source integration using the documented
[Codex app-server](https://learn.chatgpt.com/docs/app-server) authentication and dynamic tools.

If Codex is not on PATH, set the full native executable path in **Chat options (three dots) →
Codex executable**, then reconnect. This setting is saved with your UI preferences. Use the
standalone native executable, rather than an npm `.cmd` launcher. macOS and Linux use their native
Codex CLI in the same way.

## Chat and document edits

Send a message with **Send** or **Ctrl+Enter** (**Cmd+Enter** on macOS). Enter inserts a newline.
Replies stream into the history. Tool status records commands as they complete. **Stop** interrupts
the reply and refuses any later queued document calls from that turn; edits already applied remain
available to Undo. The three-dot menu offers **Clear conversation**, **Sign out**, and **Disconnect**.
Clearing stops the active turn, discards its model context and reconnects using the saved login.

Examples: “Rewrite the selected paragraph more concisely”, “Make all headings consistent”, or
“Insert a three-column comparison table after this paragraph”. The assistant discovers commands
and exact parameters from WordCraft rather than maintaining a separate subset of editing features.
Programmatic engine calls do not open formatting or file-picker dialogs; required parameters must
be supplied in chat. Closing or replacing unsaved work is refused until it is saved, unless the
user explicitly requests discarding changes and the assistant passes a boolean `force: true`. Unsupported features remain subject to the engine’s existing limits.

## Storage and scope

- There is one conversation for the loaded document. Collapsing the pane preserves it.
- History is held in memory for this app session. Opening/replacing the document clears it.
  Restarting or reconnecting the backend starts fresh model context. After reconnecting, older
  visible messages are retained only for reference and a notice marks the new conversation.
- The app sends document content to OpenAI when the assistant inspects it during a conversation.
  Merely opening the sidebar does not send the document.
- Codex manages and refreshes login credentials using an isolated `CODEX_HOME`: the `chatgpt`
  subfolder of WordCraft’s settings directory (`%APPDATA%\WordCraft\chatgpt` on Windows).
  Signing out here does not sign out your separate Codex coding sessions. Credentials, login URLs
  and transcript text are excluded from `ui.json` and are not written to WordCraft logs.
- Shell tools, delegation, apps and web search are disabled. The backend uses a read-only filesystem
  sandbox; document edits happen through WordCraft. Tool calls check document, conversation and
  turn ownership immediately before dispatch. No server is exposed on the network by WordCraft.
- Browser/WASM builds show a desktop-availability message and never launch a native process.

## Automation and verification

The control channel exposes `ui.chatgpt.toggle {value?}`, `connect`, `login`, `logout`, `clear`,
`stop`, `send {text}` and `status`; the latter commands take an empty object unless specified.
`engine.commands {search: "chatgpt"}` lists them. Status exposes connection and message counts,
without credentials or transcript text. `login` explicitly opens the sign-in browser page.

Run focused tests with `cargo test -p wordcraft-ui-egui chatgpt --lib`. They cover engine command
discovery, edits and Undo, stale/stop/document guards, streaming, clear/collapse, text input focus,
pagination, message limits and URL validation. A protocol smoke check with Codex 0.162.1 verifies
initialization and ephemeral dynamic-tool thread creation without signing in or invoking a model.
Browser sign-in, account limits and a live model edit still require an interactive account check.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-15 | minor | First desktop ChatGPT sidebar, managed login and engine tools |
