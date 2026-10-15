# WordCraft architecture

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-16 · **Change:** minor (clickable ChatGPT document citations); previously minor (private per-document ChatGPT archives); previously minor (Program Files install and DOCX default-app registration); previously minor (native Windows EXE installer); previously minor (ChatGPT web search, referenced files and context menu); previously minor (ChatGPT selection references and formatted replies); previously minor (simplified ChatGPT conversation controls); previously minor (standalone ChatGPT sign-in and first-party documentation); previously minor (ChatGPT OAuth JSON settings and direct transport); previously minor (Desktop ChatGPT transport and UI-thread document tools); previously minor (`proof` is per language: English and Polish; previously major: first committed version; `plan/architecture.md` is local-only) · **Target:** Microsoft Word (Microsoft 365) for Mac 16.113.4

How WordCraft is built today (origin/main `40c5d51`). The longer design notes in
`plan/architecture.md` are gitignored and local to the original author's machine; this file is the
committed, current summary. Rules for working in the code are in [`AGENTS.md`](../AGENTS.md).

## Layers

~85,600 lines of Rust, 15 crates, 3 apps and `xtask`. Layering is enforced by `cargo xtask layers`:
a crate may only depend on crates in lower layers, and nothing below L6 depends on egui, eframe,
winit or rfd.

| Layer | Crate | Lines | Job |
|---|---|---|---|
| L0 | `geom` | 0.2k | Units (points, twips, EMU, inches, cm), rectangles, measurement parsing |
| L1 | `doc` | 10.0k | Document model: stories, paragraphs, runs, tables, sections, styles, numbering, fields, notes, comments, revisions, equations (OMML-shaped math tree, linear/LaTeX input); edit primitives; bidi |
| L1 | `fonts` | 2.3k | Font database (bundled OFL + system fonts), metrics, outlines, shaping with HarfRust, Word font substitution (`word.rs`: Carlito ↔ Calibri, Caladea ↔ Cambria, Liberation ↔ Arial/Times/Courier), OpenType MATH |
| L1 | `proof` | 3.8k | Proofing per language (`ProofLang` from a run's `w:lang`): English (public-domain word list) and Polish (installed Hunspell dictionary through a Hunspell-style affix engine) spelling and suggestions, rule-based grammar for both, hyphenation dictionary and Liang patterns (English, Polish) |
| L2 | `layout` | 9.6k | Shaping runs, line breaking, tabs, lists, tables (row splitting, floating tables), pagination, headers/footers, notes, floating objects and wrap, math layout, fields, hit testing; produces a display list |
| L2 | `docx` | 8.4k | OOXML WordprocessingML reader and writer (Transitional and Strict namespaces, `.docx/.docm/.dotx/.dotm`), OMML, custom properties, VBA parts preserved |
| L2 | `docbin` | 4.4k | Word 97-2003 `.doc` reader ([MS-DOC]: FIB, piece table, FKPs, SPRMs, stylesheet, lists, tables, notes, pictures) over a CFB container |
| L2 | `formats` | 10.6k | TXT, Markdown, HTML, RTF, ODT and LaTeX import/export through a shared intermediate model |
| L3 | `render` | 0.6k | vello_cpu rasteriser: pages and thumbnails from the display list; PNG/JPEG |
| L3 | `pdf` | 1.6k | krilla PDF export: font subsets, selectable text, links, outline, tagging |
| L4 | `engine` | 15.2k | `Session` (document, selection, undo, layout cache, view state), **command registry** (422 `CommandSpec`s in `cmd/*.rs`), Word feature catalog (`catalog.rs`), samples, file I/O dispatch (`io.rs`), Read Aloud sentence model |
| L4 | `zotero` | 1.7k | Zotero's word-processor integration protocol answered against a `Session` |
| L4 | `control-key` | 0.1k | Per-window control-port key files shared by the app and the MCP bridge |
| L5 | `mcp` | 1.0k | MCP server: every command as a tool; headless or bridged to the running app |
| L6 | `ui-egui` | 13.1k | Word-style front end: ribbon, keytips, mini-toolbar, Backstage, canvas, rulers, panes, dialogs, Equation tab, i18n catalogs, themes, control channel, headless `ui_shot` example |
| apps | `wordcraft` | 1.6k | Desktop app (eframe + wgpu; logging, window geometry, file associations) |
| apps | `wordcraft-cli` | 0.4k | `convert`, `text`, `inspect`, `run`, `commands`, `parity`, `mcp`, `zotero` |
| apps | `wordcraft-web` | 0.3k | Browser build (eframe web runner, WebGPU with WebGL2 fallback) |

## Data flow

1. **Open:** `engine::io` picks a reader by extension (`docx`, `docbin`, `formats`, JSON) and
   produces a `doc::Document`.
2. **Edit:** every user-visible action is a command id run through `Session::run` (ribbon,
   shortcuts, command search, CLI, control channel and MCP all dispatch by id). `Session::run`
   snapshots for undo (blocks are `Arc`-shared, so snapshots are cheap) and catches an escaped
   panic, restoring the document (never-crash standard).
3. **Layout:** `layout` turns the document into pages of positioned glyphs, boxes and images
   (display list), incrementally: an edit relayouts only what changed (1.4 ms on a 188-page
   document, measured 2026-10-06).
4. **Draw:** the UI paints pages through `render` (CPU raster, cached per page, pixel-aligned) on
   an egui/wgpu canvas; PDF export walks the same display list through `pdf`.
5. **Save:** `engine::io` picks a writer by extension.

## ChatGPT document assistant

Native chat transcripts, drafts, references and bounded continuation notes are stored separately
from credentials and documents in private `chatgpt-history` archives keyed by canonical path hashes.
A bounded background queue serializes load/save/delete operations. Writes use protected temporary
files, atomic replacement, per-document locks and expected-content hashes to detect conflicting
windows. Load tickets reject stale results after clearing or changing documents. Shutdown flushes
the latest snapshot. Resumption restores text checkpoints rather than replaying tool calls; live
positions and references are revalidated against the newly loaded document.


`ui-egui::chatgpt` owns the sidebar and its document-scoped conversation. On native hosts, `chatgpt::desktop` runs a background direct Responses worker. `chatgpt::auth` validates imported OSS OAuth JSON, stores credentials privately under `chatgpt-direct` in WordCraft’s profile, and serializes atomic token refreshes across processes. `chatgpt::inference` bounds SSE and waits for completed responses before dispatching tools. `chatgpt::auth::login` implements WordCraft-owned PKCE/OIDC browser sign-in and the bundled Rust JSON helper. The settings popup supports automatic sign-in, optional JSON import and account-model discovery; credentials and chat text are excluded from UI preferences.

The model receives six function tools in a WordCraft namespace: discover commands, inspect a bounded document overview, search/read document ranges, create a verified citation destination, read a user-referenced file, and execute an engine command. Hosted web search is offered separately and controlled by a persisted setting, enabled by default. Calls return to the UI thread and use `WordApp::execute`, preserving engine validation and Undo. Each call checks the current document, conversation, turn and active state. Closing a conversation cancels its worker and discards its in-memory context; an in-flight HTTPS request cannot dispatch later document actions. This module is disabled for web transport; the WASM UI still compiles. See [chatgpt.md](chatgpt.md).

`chatgpt::references` bounds excerpt attachments saved with the local conversation and validates document ownership,
relocating unchanged text with boundary context after edits. UI commands attach, remove and reveal
references through the ordinary selection command. The mini toolbar adds a code-drawn assistant
icon; the composer and sent messages show local reference links. `chatgpt::markdown` renders
assistant replies with `egui_commonmark`, reuses WordCraft's math previews, preserves code and
filters model-provided images and unsupported links before rendering. `chatgpt::links` registers opaque document-scoped destinations after exact-text/revision verification. Markdown hooks intercept these URLs and defer navigation until the transcript has been restored to state; links use the normal selection command and persist in chat archives. Unknown local IDs never reach the OS URL handler.

`chatgpt::files` validates file paths against user-authored chat messages on the UI thread;
the background transport reads authorized paths with bounded pages and cancellation checks.
Rich-document extraction creates a separate document model, preserving the live session.
`chatgpt::citations` formats URL annotations as clickable Markdown links and hides internal citation
tokens. Search output items remain in Responses history; search progress reaches the sidebar.
The selection context menu reuses `ui.chatgpt.attachSelection` through **Add to Chat**.

## Fork Windows installer

[The EXE setup wizard](windows-installer.md) installs for all users into Program Files with license acceptance,
an installation-folder chooser, optional CLI tools, shortcuts and Open with/Default Apps registration. Setup requests
administrator permission; the application keeps normal per-user permissions and credentials.
It stages an explicit public payload and keeps the existing native ChatGPT profile and Windows
permissions. Uninstall preserves documents and saved login. Signing remains a release-time
configuration; local builds without a configured certificate are unsigned. Setup refuses to overwrite a
different edition in the same directory.

## Testing and gates

- 821 `#[test]` functions and 10 property-test blocks: model, layout, DOCX round-trip and hostile
  input, `.doc`, formats, typing parity (`engine/src/tests_typing.rs`), bidi, i18n catalogs, UI
  interaction (egui_kittest), MCP acceptance; `hostile_params_never_panic` runs every command with
  junk params.
- `cargo xtask ci`: fmt, clippy `-D warnings`, tests, asset attribution, layering, wasm build.
- `cargo xtask parity` regenerates [`parity-checklist.md`](parity-checklist.md); a test keeps the
  catalog percentage above a floor that only rises.
- GitHub Actions on PRs: packaging lint, and FreeBSD / Windows ARM64 when build files change;
  release builds on pushes to `release`.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-16 | minor | Verified ChatGPT document citations, local Markdown navigation and persisted link destinations |
| 2026-10-15 | minor | Private per-document chat archives, background atomic writes and bounded continuation snapshots |
| 2026-10-15 | minor | Program Files default, all-users setup, DOCX Default Apps registration and foreign-install protection |
| 2026-10-15 | minor | Native per-user Windows EXE installer, license/folder/components/shortcuts pages and uninstall preserving the user profile |
| 2026-10-15 | minor | WordCraft-owned browser login, automatic credential storage and standalone documentation |
| 2026-10-15 | minor | Default-on ChatGPT web search, referenced file reads and right-click Add to Chat |
| 2026-10-15 | minor | ChatGPT selection references, source navigation and native Markdown/equation previews |
| 2026-10-15 | minor | Removed Stop and Disconnect controls; Clear conversation and Sign out cancel active replies |
| 2026-10-15 | minor | Standalone ChatGPT browser sign-in, automatic credential import and bundled Rust helper |
| 2026-10-15 | minor | Desktop ChatGPT transport and UI-thread document tools |
| 2026-10-10 | minor | `proof`: proofing languages (English, Polish), affix engine, Polish patterns; layout and engine route each run by its language |
| 2026-10-10 | major | First committed architecture doc, from the crates' sources and `AGENTS.md` |
