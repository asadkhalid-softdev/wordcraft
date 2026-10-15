Original project documentation: [README_original.md](README_original.md) · [Upstream WordCraft](https://github.com/storytold/wordcraft).

# WordCraft with ChatGPT document editing

This fork adds a collapsible ChatGPT sidebar to WordCraft. ChatGPT can read and edit the loaded
document through WordCraft’s command system, including formatting, tables, styles, layout,
references and review. Edits use normal Undo and protect unsaved work.

The sidebar provides one chat, streamed replies, **⋯ → Clear conversation**, account settings
and an account-specific model picker. Login persists across restarts. Chat history currently lasts
for the app session and resets when another document opens.

Select document text and click the **ChatGPT icon** in the floating mini toolbar to attach it to
your next message. A compact reference link appears above the chat input; click it to highlight
the source text, or hover and click **×** to remove it. Replies render Markdown tables, headings,
lists, code blocks and supported LaTeX equations.

## Run on native Windows

With Rust and the Windows build prerequisites installed, open PowerShell in this repository:

```powershell
cargo run --release -p wordcraft -- --sample
```

See the [original README](README_original.md) for the project’s general build prerequisites,
platform support and document-editor documentation.

## Connect your ChatGPT account

1. Open **ChatGPT** in WordCraft’s title bar.
2. Click **Continue with ChatGPT**.
3. Complete sign-in, choose your workspace if prompted, and approve WordCraft in your browser.
4. Return to WordCraft. It verifies your sign-in, saves the credentials privately, loads your
   available models and connects automatically. No JSON copying or terminal command is needed.
5. Ask for an edit. Use **⋯ → ChatGPT settings** to select a model, or normal **Undo** to reverse edits.

The browser runs on the same computer as WordCraft and returns to a temporary `127.0.0.1` callback.
You can cancel sign-in in the sidebar. On later launches, **Continue with ChatGPT** reuses the saved
login when it is valid. An eligible ChatGPT account/workspace must allow OpenAI’s OSS plan access;
account policy and usage limits still apply. This integration does not require an API key,
Codex CLI, Python or a separate integration repository.

## Optional: acquire and import authentication JSON

This is an advanced alternative to the browser button. The Rust login helper is included in this
WordCraft repository and identifies itself as **WordCraft** during authorization.

From this repository in native Windows PowerShell:

```powershell
cargo run -p wordcraft-ui-egui --bin wordcraft-chatgpt-login
```

Complete browser sign-in and wait for **Sign-in verified**. The helper prints the private output
file’s location without printing its contents. On Windows, open the default output with:

```powershell
notepad "$env:APPDATA\WordCraft\chatgpt-login\wordcraft-account.json"
```

In WordCraft, open **ChatGPT → ⋯ → ChatGPT settings → Advanced: import authentication JSON**.
Paste the file’s complete contents into **Protected credential file JSON**, then click
**Import and connect**. Close the editor and clear your clipboard afterwards.

If the helper cannot open your browser, add `-- --no-browser` to the Cargo command and open its
printed sign-in URL on the same computer. Use `-- --new-account` to create a separate account or
workspace registration. Only one application should refresh a given credential session.

## Credential storage and privacy

The normal browser flow creates and imports the credential JSON automatically, storing it at
`%APPDATA%\WordCraft\chatgpt-direct\account.json`. The optional helper’s export is separate.
Credentials are written atomically with current-user Windows permissions or Unix mode 0600,
and are excluded from document files and normal UI preferences. Keep profile folders and backups
private: credential storage is protected local plaintext, not an encrypted vault.

WordCraft refreshes login tokens as needed. **Sign out** clears local credentials and attempts
remote revocation. Questions and necessary document context go to OpenAI through the public
Responses API with `store=false`; this does not promise zero provider retention.
Never share credential JSON, attach it to issues or commit it to Git. Credential files, local
profiles, logs and build artifacts are excluded from the source changes.

[Detailed ChatGPT setup, behavior and troubleshooting](docs/chatgpt.md).
