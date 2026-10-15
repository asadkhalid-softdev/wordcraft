# ChatGPT document assistant

Last updated: 2026-10-15 · M0 extension: imported OSS OAuth authentication.

Open **ChatGPT** in the title bar, then **⋯ → ChatGPT settings**. Paste the complete protected
credential JSON from the ChatGPT Assist login helper into **Protected credential file JSON** and
click **Import and connect**. Choose an account model in the same settings popup.

WordCraft connects directly to OpenAI with the imported ChatGPT grant. The native Codex CLI and an
API key are no longer required. Access depends on your account/workspace's eligible ChatGPT plan
permissions and usage limits. This imports an OSS OAuth grant, not a ChatGPT browser session or
Codex `auth.json`.

## Acquire the JSON on native Windows

These instructions follow the reference [ChatGPT Assist installation guide](https://github.com/asadkhalid-softdev/hass-chatgpt-integration/blob/main/docs/INSTALLATION.md).
Its login helper performs OAuth, validates the returned identity and creates the protected JSON.
Use the full helper repository, including `scripts/` and `custom_components/`; the helper shares
its OAuth code with the integration. Home Assistant itself is not required.

1. Install `uv` on the computer running your browser, then clone or download
   [hass-chatgpt-integration](https://github.com/asadkhalid-softdev/hass-chatgpt-integration).
2. Open native PowerShell in that repository's directory. Run this single line:

   ```powershell
   uv run --python 3.12 --script scripts/chatgpt_login.py --output "$HOME/.config/chatgpt-assist/wordcraft-account.json"
   ```

   `uv` downloads Python and manages the helper's dependencies. Use a local NTFS user profile
   folder on Windows so the helper can protect the output file.
3. Complete OpenAI sign-in, choose your workspace if prompted, and grant ChatGPT plan access.
   Wait for **Sign-in verified** in the terminal. The browser callback must reach
   `http://127.0.0.1:<port>/auth/callback` on the same computer. The helper times out after five minutes.
4. Open the generated file:

   ```powershell
   notepad "$HOME/.config/chatgpt-assist/wordcraft-account.json"
   ```

5. Copy its entire JSON into WordCraft's masked settings field and click **Import and connect**.
   The access token must still be valid. If validation says it expired, run the helper again.
6. Clear your clipboard and close the editor. Keep any retained file private; never put it in Git
   or a support message. WordCraft clears the input after submission or closing settings.

If the browser does not open, add `--no-browser` to the helper command, then open the printed URL
in a browser on the same computer. The original helper guide also covers virtual environments
and Linux/macOS setup.

**Create a separate registration for WordCraft.** The distinct `wordcraft-account.json` output
avoids reusing Home Assistant's default file. If that output already belongs to another running
integration, choose a new output filename. Do not import one renewable session into multiple
applications: rotating refresh tokens would race. The shared helper currently labels the OAuth
registration **ChatGPT Assist** on OpenAI's consent screen. After import, WordCraft owns refreshes.
Reauthorizing an already-imported registration must use its latest rotated credentials, not a stale
helper output; use a new output filename to create a fresh registration when necessary.

## Settings and persistence

- Login persists across application restarts. Click **Connect to ChatGPT** to reuse the saved grant.
- Credentials are stored separately from preferences and documents at
  `%APPDATA%\WordCraft\chatgpt-direct\account.json` on Windows,
  `~/Library/Application Support/WordCraft/chatgpt-direct/account.json` on macOS, or
  `$XDG_CONFIG_HOME/wordcraft/chatgpt-direct/account.json` (default `~/.config/wordcraft/…`) elsewhere.
  `WORDCRAFT_PROFILE_DIR` overrides the WordCraft profile directory.
- Writes are atomic. Credential files have a protected current-user Windows ACL or Unix mode 0600.
  This is protected local plaintext storage, not an encrypted vault.
- Import verifies the access-token signature against OpenAI signing keys, RS256, issuer, audience,
  expiry, issue time, subject, issued client ID and required ChatGPT plan scopes. Imported email,
  host attribution and expiry metadata are not trusted.
- WordCraft discovers the signed-in account's model list. Your chosen model is a nonsecret preference.
- Refreshes use the issued client ID and are serialized with a cross-process file lock. Replacement
  refresh tokens are saved atomically. Do not share the session with another app.
- **Sign out** attempts remote revocation and removes local credentials. If revocation cannot be
  confirmed, WordCraft says so; disconnect the registration in ChatGPT Settings.
- Credentials and transcripts are excluded from UI preferences, control status and diagnostics.
  Provider error bodies are not shown or logged. Local credential filenames are ignored by Git.

## Conversation and document access

One conversation belongs to the loaded document. Collapsing the sidebar keeps it; **⋯ → Clear
conversation**, replacing the document, switching accounts/models or restarting the application
starts a new one. History is currently in memory only. Failed turns discard the backend context.
**Stop** cancels document actions immediately and disconnects the worker; reconnect to continue.
An in-flight HTTPS request may finish in the background, but cannot dispatch more document actions.
Earlier edits remain and can be reversed with normal Undo. Ctrl+Enter (Cmd+Enter on macOS) sends;
ordinary Enter adds a newline.

The assistant uses `wordcraft_commands` to discover every registered engine command and native
printing command, `wordcraft_inspect` to inspect paginated document state, and `wordcraft_execute`
to dispatch edits through WordCraft's normal command validation and Undo. Unsupported UI dialogs,
shell execution and arbitrary external tools are excluded. Tool calls must still belong to the
active document and turn, and cannot silently discard unsaved work.

The direct backend calls the public `https://api.openai.com/v1/models` and `/responses` routes,
with `store=false` and `stream=true`. Your questions and necessary document/tool context go to
OpenAI. This does not provide access to your ChatGPT chat history or promise zero provider retention.
Tools execute only after `response.completed`; partial, failed or interrupted streams never trigger
provisional actions. Input, stream size, output item count and action rounds are bounded. Mutating
requests are not automatically retried.

Official protocol references: [models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference),
[accounts, refresh and revocation](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions).
The browser-hosted WASM app displays an availability message; credential storage and HTTPS transport
are compiled only for native desktop.

## Validation

Offline tests cover signed access-token validation, private atomic credential replacement, malformed
imports, bounded SSE parsing, completed-response gating, document command discovery/editing/Undo,
stale and stopped tool calls, and settings/credential isolation. Headless screenshots inspect the
sidebar and settings popup. Live sign-in, account model discovery and a real inference turn still
require an interactive account check. No real credential JSON is used in tests or committed.

## Revision history

| Date | Change |
|---|---|
| 2026-10-15 | Replace Codex login with reference-compatible OAuth JSON import, protected persistence, direct streaming inference and settings/model picker |
| 2026-10-15 | Initial document sidebar and command tools |
