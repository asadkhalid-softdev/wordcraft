# ChatGPT document assistant

Last updated: 2026-10-15 · M0 extension: bounded document retrieval and automatic working memory.

## Sign in from WordCraft

Open **ChatGPT** in the title bar and click **Continue with ChatGPT**. Your browser opens OpenAI’s
sign-in page. Sign in, select your workspace if prompted, and approve **WordCraft** to use your
ChatGPT plan. The browser returns to a temporary `http://127.0.0.1:<port>/auth/callback` listener
on this computer. Return to WordCraft while it finishes verification and model discovery.

WordCraft creates the authentication JSON, saves it privately and connects automatically.
There is no file to copy for normal setup. The sidebar offers **Cancel sign-in** and a link to
reopen the browser page if needed. The browser callback has a five-minute timeout. On later
launches, **Continue with ChatGPT** uses the existing saved grant when it is valid.

Choose a model under **⋯ → ChatGPT settings**. To register another account/workspace, use
**Use a different account** there. New registrations use WordCraft’s own app name and stable
host ID. Previously imported registrations are not silently repurposed for native sign-in.

Only WordCraft and your browser are needed. No API key, Codex CLI, Python environment or other
integration repository is required. Access remains subject to your account/workspace’s eligible
ChatGPT plan permissions and usage limits. OAuth sign-in cannot bypass an account restriction.

## Optional standalone JSON helper

The repository includes its own Rust executable, `wordcraft-chatgpt-login`. It uses the same
OAuth validation and protected storage implementation as the sidebar. Use it only if you prefer
an explicit file transfer or are troubleshooting browser sign-in in the app.

From the WordCraft repository in native Windows PowerShell:

```powershell
cargo run -p wordcraft-ui-egui --bin wordcraft-chatgpt-login
```

Complete browser sign-in and wait for **Sign-in verified**. On Windows, open its generated JSON:

```powershell
notepad "$env:APPDATA\WordCraft\chatgpt-login\wordcraft-account.json"
```

Open **ChatGPT → ⋯ → ChatGPT settings → Advanced: import authentication JSON**. Paste the complete
file into **Protected credential file JSON**, then click **Import and connect**. WordCraft verifies
the signed access token and discovers account models before replacing the managed credentials.
Clear your clipboard and close the editor. The JSON input is cleared after submission or closing
settings. Do not share a renewable session with another application; refresh tokens rotate.

Helper options follow the Cargo argument separator:

```powershell
cargo run -p wordcraft-ui-egui --bin wordcraft-chatgpt-login -- --help
cargo run -p wordcraft-ui-egui --bin wordcraft-chatgpt-login -- --no-browser
cargo run -p wordcraft-ui-egui --bin wordcraft-chatgpt-login -- --new-account
```

`--no-browser` prints a sign-in URL for opening manually on the same computer. It contains no ID,
access or refresh token. Returning login preserves the issued client ID and checks the returned
identity. `--new-account` creates a fresh registration. The helper writes a separate export and
never prints its credential contents. Recreate an expired export with the helper instead of
editing token fields by hand.

On macOS and Linux/BSD, use the same Cargo command. The helper prints the output location under
WordCraft’s settings directory. A separately built helper executable accepts the same flags directly.

## Storage and validation

WordCraft’s settings folder is `%APPDATA%\WordCraft` on Windows,
`~/Library/Application Support/WordCraft` on macOS, and `$XDG_CONFIG_HOME/wordcraft` or
`~/.config/wordcraft` on Linux/BSD. `WORDCRAFT_PROFILE_DIR` overrides this root.

- Automatic sign-in and imported credentials use `chatgpt-direct/account.json`.
- The advanced helper’s export is `chatgpt-login/wordcraft-account.json`.
- `chatgpt-login/host.json` persists an opaque host ID; `registration.json` remembers the
  WordCraft registration’s issued client ID and subject. Lock files serialize sign-in and refresh.
- Credential writes are atomic and protected with current-user Windows ACLs or Unix mode 0600.
  Files are protected local plaintext, not an encrypted vault. Keep the profile and backups private.
- Each browser attempt has fresh state, OIDC nonce and PKCE verifier. Callback host, path, method,
  duplicate parameters, state, issued client ID and account identity are checked.
- ID-token and access-token signatures use OpenAI’s signing keys. Verification checks RS256,
  issuer, audience, expiry, issue time and subject; ID tokens must match the attempt’s nonce,
  and access tokens must authorize the issued client and required ChatGPT plan scopes.
- Imported account/expiry/host metadata is not trusted to establish identity or permissions.
- Rotating refreshes use the saved issued client ID and a cross-process lock. The latest replacement
  tokens are persisted atomically. The selected model is a nonsecret UI preference.
- **Sign out** attempts remote revocation and removes local credentials. If remote revocation
  cannot be confirmed, WordCraft says so; disconnect the registration in ChatGPT Settings.
- Credentials and transcripts are excluded from normal UI preferences and control status.
  Authentication error messages never echo submitted JSON or provider error bodies.

## Troubleshooting

If no browser opens, click **Open sign-in page** in the sidebar. The browser and WordCraft must
run on the same computer with functioning loopback networking; use native Windows PowerShell
for the optional helper when WordCraft runs natively on Windows. Check local firewall rules if
the callback cannot reach the app. Cancel and retry after a timeout or denied consent.

If token verification fails, check your system clock and sign in again. If OpenAI denies model
access or reports a usage limit, review account/workspace policy and
[ChatGPT Settings → Usage](https://chatgpt.com/settings/usage). A successful browser login does
not itself guarantee that the account has eligible inference access.

## Add selected text to a message

1. Select a continuous range in the document. The floating **mini toolbar** appears.
2. Click its **ChatGPT** speech-bubble/sparkle icon (**Add selection to ChatGPT**), or right-click
   inside the selection and choose **Add to Chat**. The sidebar
   opens and attaches the selection to your next message.
3. Type a request, such as “Rewrite this more clearly”, and send it. The assistant receives the
   full excerpt and its document range, even if you select other text before sending.

The composer displays a compact **Selected text …** reference link instead of the excerpt.
Click the link to select and reveal the source range. Hover the reference to reveal **×**, or
focus its remove control with the keyboard, to remove it before sending. Removing a reference
changes only the draft. Sent messages retain clickable references in chat history.

You can attach up to eight selections, totaling at most 32,000 bytes. Attachments are in memory
only, belong to the current document, and clear with the conversation. Sign-in preserves a draft's
attachments. After edits, navigation relocates an unchanged excerpt using nearby text; changed,
ambiguous or excessively large searches ask you to select it again instead of guessing a range.
A missing or changed reference also prevents sending that draft until it is removed or replaced.
Rectangular column selections and whole-cell selections require a continuous text selection.

## Formatted replies

Assistant messages use a native CommonMark/GitHub Markdown preview: headings, emphasis, lists,
quotes, links, tables, strikethrough, task lists and fenced code blocks with copy controls.
Supported LaTeX equations render through WordCraft's equation engine, including `$…$`, `$$…$$`,
`\(…\)` and `\[…\]`. Code examples retain their literal source. This is a Rust-native renderer,
not ChatGPT's website or its proprietary UI. Unsupported LaTeX commands may remain visible as
text within the equation; equation size and count are bounded for responsiveness.

Images are shown as their alternative text and never fetched from a model-provided URL or local
file path. Clickable external links allow HTTP, HTTPS and mail links. User messages remain literal
text so document excerpts and user input are not interpreted as formatting instructions.

## Web search

**Enable web search** in **ChatGPT → ⋯ → ChatGPT settings** is on by default and saved across
restarts. It offers OpenAI's hosted `web_search` tool; the model decides when a search is useful.
Turning it off removes that tool from the next message's inference requests without clearing the
conversation. It does not retract searches already performed or erase existing source context.
Search availability depends on the selected model and account/workspace policy. If the service
rejects search, turn the switch off or choose another eligible account model.

Search progress appears in the sidebar. URL citations become clickable numbered links next to
claims; source links remain available if the provider supplies an invalid citation position.
Internal provider citation tokens are hidden while a reply streams. Completed search items remain
in the in-memory model context, alongside completed messages and document tools.

## Read a referenced file

Put a file path in your own chat message, preferably inside backticks or quotes, for example:
“Compare this document with `C:\Users\YourName\Documents\notes.txt`.” ChatGPT can call
`wordcraft_read_file` to read that file in the background without opening it in the editor or
replacing your current document. Absolute paths can point anywhere your OS account may read.
Relative paths use the open document's directory, or WordCraft's working directory for an unsaved
document. No elevated permissions, directory browsing or shell commands are added.

A path must occur in your own conversation messages. Paths found only in document contents,
assistant replies, search results or other tool output do not grant the file reader access.
Clearing the conversation clears these path references. Existing engine file commands retain
their usual behavior; the new reader's path check is not a filesystem sandbox for the application.

Reads return up to 32 KiB per call with byte offsets and `nextOffset`/`hasMore` for continuation.
Auto mode extracts body text from Word, ODT and RTF documents up to 32 MiB without modifying the
live session. Other files return UTF-8/UTF-16 text when detected, or base64 raw bytes for binary
content. The model can request explicit text or base64 mode. Raw bytes do not provide OCR, image
understanding or extracted PDF text. Large rich documents can still be read as paged raw bytes;
parsing and readable content are separate from filesystem read permission. Referenced file
contents are sent to OpenAI as needed for the requested response, and remain outside saved UI
preferences and document files.

## Conversation and document access

One conversation belongs to the loaded document. Collapsing the sidebar keeps it; **⋯ → Clear
conversation**, replacing the document, switching accounts/models or restarting the application
starts a new one. History is currently in memory only. Failed turns preserve working context and mark interrupted tool outcomes as uncertain, so the assistant must inspect before retrying an edit.
**Clear conversation** and **Sign out** cancel pending document actions immediately. Clearing keeps
your saved login and reconnects automatically; signing out removes the saved credentials.
An in-flight HTTPS request may finish in the background, but cannot dispatch more document actions.
Earlier edits remain and can be reversed with normal Undo. Ctrl+Enter (Cmd+Enter on macOS) sends;
ordinary Enter adds a newline.

The assistant uses `wordcraft_read_file` for explicitly referenced files, `wordcraft_commands` to discover every registered engine command and native
printing command, `wordcraft_document` for bounded overview/search/read operations, `wordcraft_inspect` for a bounded overview, and `wordcraft_execute`
to dispatch edits through WordCraft's normal command validation and Undo. Unsupported UI dialogs,
shell execution and arbitrary external tools are excluded. Tool calls must still belong to the
active document and turn, and cannot silently discard unsaved work.

### Large documents and long conversations

WordCraft does not send the whole loaded document with your question. The assistant starts with
a small overview: title, block count, headings, selection positions, revision and available document
parts. It can search literal, case-sensitive phrases and read relevant paragraphs in small pages,
including paragraphs inside nested table cells. It can search related terms separately. Headers,
footers, notes, comments and text boxes are available as separate stories. Selecting text and using
**Add to Chat** remains the most direct way to identify a passage.

For a whole-document review, the assistant follows continuation cursors through the body and each
part. A result page is explicitly partial; it must not claim to have reviewed unread text. Long
individual paragraphs are split at UTF-8 boundaries, and search windows overlap so phrases crossing
a boundary are found. Cursors include paragraph ordinals and byte offsets; pass the returned
revision to detect changes before continuing a scan. Known selection/search paths can be read
directly. Formatting details remain available through `document.paragraph` and other commands.
The legacy `document.inspect` route is replaced with a bounded overview inside ChatGPT; other
clients retain its original behavior.

Before every model request, WordCraft checks the complete serialized payload, including tool
schemas and JSON escaping. At 144 KB, it reduces older working context to a bounded summary and
keeps the current question plus current-turn tool receipts. The sidebar transcript is independent
and is not cleared by this process (it retains the latest 256 messages). A brief **Updating
conversation memory…** status appears. Summarization uses your selected ChatGPT model and may
consume plan usage; if it fails, a bounded local checkpoint allows continuation. Summaries can omit
details, so the assistant is instructed to reread the live document and sources when needed.

Every tool result has a 24 KB serialization limit, including nested objects and results from generic
commands. Oversized results preserve whether the operation succeeded and explicitly warn against
repeating a successful edit. File reads are clamped to 4 KB per page with valid continuation offsets.
A recognized provider context-limit rejection triggers one smaller-context inference retry; completed
commands are never replayed by the transport. If a single user message/excerpt is itself too large,
WordCraft asks for a smaller message while preserving the conversation. Existing per-reply action
limits still apply; full-document tasks may need follow-up messages to continue their coverage.

The direct backend calls the public `https://api.openai.com/v1/models` and `/responses` routes,
with `store=false` and `stream=true`. Your questions and necessary document/tool context go to
OpenAI. This does not provide access to your ChatGPT chat history or promise zero provider retention.
Tools execute only after `response.completed`; partial, failed or interrupted streams never trigger
provisional actions. Input, stream size, output item count and action rounds are bounded. Mutating
requests are not automatically retried.

Official web-search references: [web search and citations](https://developers.openai.com/api/docs/guides/tools-web-search),
[ChatGPT plan tool support](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations).

Official protocol references: [registration and sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in),
[models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference),
[accounts, refresh and revocation](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions).
The browser-hosted WASM app displays an availability message; credential storage and HTTPS transport
are compiled only for native desktop.

## Validation

Offline tests cover PKCE, loopback callback validation, nonce and signed ID-token checks, cancellation, signed access-token validation, private atomic credential replacement, malformed
imports, bounded SSE parsing, completed-response gating, document command discovery/editing/Undo,
stale and cancelled tool calls, settings/credential isolation, large Unicode paragraph pagination, search across chunk boundaries, table/part paths, oversized nested results, escaped request budgeting and completed-edit receipts. Headless screenshots inspect the
sidebar and settings popup. Live sign-in, account model discovery and a real inference turn still
require an interactive account check. No real credential JSON is used in tests or committed.

## Revision history

| Date | Change |
|---|---|
| 2026-10-15 | Bounded overview, literal search and paged document reads; automatic working-memory checkpoints and context-limit recovery |
| 2026-10-15 | Default-on web search with persisted switch and source citations, user-referenced file reader, and right-click Add to Chat |
| 2026-10-15 | Selection references in the mini toolbar/composer, document navigation and native Markdown/equation previews |
| 2026-10-15 | Simplified chat controls to Clear conversation and Sign out; both cancel active replies |
| 2026-10-15 | Standalone WordCraft browser sign-in, automatic credential import, bundled Rust JSON helper and first-party setup guide |
| 2026-10-15 | OAuth JSON import, protected persistence, direct streaming inference and settings/model picker |
| 2026-10-15 | Initial document sidebar and command tools |
