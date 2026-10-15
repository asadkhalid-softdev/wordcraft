# WordCraft roadmap: milestones and what's next

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-15 · **Change:** minor (private per-document ChatGPT history); previously trivial (streaming ChatGPT sidebar visibility); previously minor (bounded ChatGPT document reads and working context); previously minor (Program Files install and DOCX default-app registration); previously minor (native Windows EXE installer); previously minor (ChatGPT web search, referenced files and context menu); previously minor (ChatGPT selection references and formatted replies); previously minor (simplified ChatGPT conversation controls); previously minor (standalone ChatGPT sign-in and first-party documentation); previously minor (ChatGPT OAuth JSON settings and direct transport); previously minor (ChatGPT sidebar: browser sign-in, one clearable conversation and document editing through engine commands); previously minor (Czech proofing review fixes; previously minor: Portuguese proofing; previously minor: Polish interface and Polish proofing; previously trivial: M7: formatting revisions landed, #41; previously trivial: M5: Draw Table and Eraser landed, #303; previously minor: beta distance restated: ~15 points, ~150–230 h; previously: alpha gate checked: six core workflows pass; previously major: milestones moved here from ROADMAP.md; Current focus set from the re-measured gaps) · **Target:** Microsoft Word (Microsoft 365) for Mac 16.113.4

Forward-looking plan. The summary is [`ROADMAP.md`](../ROADMAP.md); the ranked work list is
[`gaps.md`](gaps.md); the numbers are in [`target-app-parity.md`](target-app-parity.md). Hours are
Opus 5.5 agent wall-clock.

## Current focus (toward beta, ~150–230 h)

1. **Merge or close the ~45 open PRs** before starting overlapping work: many close catalog gaps
   (Clipboard pane #264, Manage Styles #270, Group #267), fix field bugs (Hebrew RTL #20, IME
   fixes #155–#164) or add languages (nb, nn, he). Owner review needed.
2. **DOCX real-world corpus** (gap #1, 30–50 h): collect openly licensed real-world .docx files,
   open/render/round-trip them, compare with Word locally, fix what breaks. Omitted/empty
   paragraph defaults now have synthetic save/reopen coverage; the corpus gate remains open.
3. **Preserve what we can't render** (gap #2, 10–15 h): remaining native ink forms and unsupported
   graphics survive a round trip. Drawing-level native ink now keeps its InkML and optional picture
   fallback alongside chart, SmartArt, OLE and drawing-canvas support.
4. **Stability sweep** (gap #3, 20–30 h): the open crash, hang and write-failure issues, plus an
   autosave/recovery soak test. Local desktop recovery is implemented; cross-platform field
   validation and longer soak testing remain. See [recovery.md](recovery.md).
5. **Pagination fidelity** (gap #4, 30–50 h): column balancing, footnote continuation, compat
   options, a local page-by-page comparison harness; an owner decision on an Aptos-metric font.
6. **Formatting revisions and content controls** (gap #5, 12–18 h), **native printing** (gap #6,
   6–10 h), **dialog depth** (gap #7, 20–30 h).

## Alpha gate (core workflows)

The core-workflow gate from the craftrules progress-docs standard: what a typical Word user does
every day, checked end to end on the main platform (macOS desktop), including save and reopen.
"Partial" here means rough but not blocking: the workflow completes.

| Core workflow | Works end to end? | Evidence | Hours to pass |
|---|---|---|---|
| Write and format a document: type, fonts, paragraph formatting, styles, lists, undo, find/replace; save as .docx and reopen | yes | 422 commands, typing parity pinned by `tests_typing.rs`, DOCX round-trip tests (`crates/docx/tests/roundtrip.rs`) | 0 |
| Open a .docx someone sent, edit it, send back a .docx Word opens | partial (not blocking) | Word opens our files; typical text/table/picture documents work; never tested on a real-world corpus; charts, SmartArt and OLE dropped silently, move tracking lost (gaps #1, #2, #5) | 0 (beta work: 50–80) |
| Build a structured document: tables, pictures, headers/footers, page numbers, sections, columns | yes | Tables with row splitting and floating tables (#137), floating pictures (#136), first/odd/even headers; columns don't balance (gap #4) | 0 |
| Review with others: comments, track changes, accept/reject, compare | yes | Margin balloons, tracked insert/delete incl. paragraph marks (#125, #244), formatting revisions (#41), compare | 0 |
| References for reports and papers: TOC, footnotes, citations and bibliography, captions | yes | Updatable TOC field (#52), notes, APA/MLA/Chicago/IEEE, Zotero (#189); long footnotes don't continue (gap #4) | 0 |
| Share or print: export PDF, print | partial (not blocking) | PDF export solid (krilla); desktop printing goes through a PDF, no native print dialog (gap #6) | 0 (beta work: 6–10) |

**Result: passes.** Every core workflow completes on macOS and the work saves and reopens; the
partial rows are fidelity gaps (beta work), not broken workflows. WordCraft stays **alpha**.

## Milestones

| # | Milestone | State | Remaining |
|---|---|---|---|
| M0 | Skeleton and vertical slice (model, layout, render, engine, Word-style UI, CLI, MCP, web) | done | — |
| M1 | DOCX I/O | done (first version); real-world corpus not started; charts/SmartArt/OLE dropped | 45–75 h |
| M2 | Home tab | done except Paste Special, Clipboard pane, Style Inspector, Manage Styles, Asian typography (PRs open for four) | 5–10 h |
| M3 | Insert tab | mostly done; equations done (#191), icons done (#309); charts, SmartArt, 3D, screenshot, online media missing | 50–80 h |
| M4 | Layout and Design tabs | done except column balancing, group | 6–10 h |
| M5 | Tables | done (row splitting, floating tables, style formatting); custom table styles (#256); Draw Table and Eraser (#303) | 6–10 h |
| M6 | References | first version done (TOC fields, cross-references, Zotero); footnote continuation missing; all 12 bibliography styles (#383) | 15–25 h |
| M7 | Review | done (balloons, paragraph-mark revisions, formatting revisions #41); move tracking, multilingual proofing missing | 35–60 h |
| M8 | View | mostly done; window commands missing | 3–5 h |
| M9 | Mailings | done (first version); Excel/Outlook data sources and email merge missing | 6–10 h |
| M10 | File/Backstage | mostly done; native printing, Options depth missing | 12–20 h |
| M11 | Draw and objects | text wrap, text boxes, canvas handles, rotation (#332), ink pens and eraser (#307), contour wrap (#354), lasso, Add Pen, Ink to Shape and Ink Replay (#395) done; Ink to Math, grouping missing | 35–55 h |
| M12 | Formats breadth (PDF, ODT, RTF, HTML, MD, TXT, LaTeX, `.doc` import) | done (first versions); `.doc` write, encryption, minor formats missing | 60–100 h |
| M13 | Performance budgets | on track (1.4 ms relayout); large real-world documents unmeasured | 10–20 h |
| M14 | 1.0 polish, packaging, signing | releases v0.1.0–v0.4.0 published (signed macOS, Windows MSI x64/x86/arm64, Linux, FreeBSD, web) | — |
| M15 | Localization (12 key languages, RTL interface, proofing languages) | 15 UI languages; English, Polish and Portuguese proofing plus Czech spelling and generic checks (system Hunspell; English hyphenation fallback), a proofing language per run | 70–115 h |
| M16 | Right-to-left and East Asian typography | RTL paragraphs done (#207) | 30–45 h |

**M0 extension:** desktop ChatGPT document assistant: collapsible right pane, standalone browser sign-in and advanced OAuth JSON settings, account-model selection, streamed single chat, clear/stop controls, and full engine command discovery/execution. See [chatgpt.md](chatgpt.md) for setup and validation limits. Beta priorities and readiness estimates are unchanged.

## Next after beta

Charts and SmartArt editing, the Draw tab, `.doc` writing, PDF Reflow, the remaining interface
languages, screen-reader access, ecosystem decisions (VBA, add-ins, cloud), an in-app assistant.

## Fork Windows installer

[The EXE setup wizard](windows-installer.md) installs for all users into Program Files with license acceptance,
an installation-folder chooser, optional CLI tools, shortcuts and Open with/Default Apps registration. Setup requests
administrator permission; the application keeps normal per-user permissions and credentials.
It stages an explicit public payload and keeps the existing native ChatGPT profile and Windows
permissions. Uninstall preserves documents and saved login. Signing remains a release-time
configuration; local builds without a configured certificate are unsigned. Setup refuses to overwrite a
different edition in the same directory.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-15 | minor | Private per-document ChatGPT archives with restored transcript, draft, references and bounded continuation context |
| 2026-10-15 | trivial | Keep streaming ChatGPT visible through Focus-mode changes and wide Markdown replies |
| 2026-10-15 | minor | Bounded ChatGPT overview/search/read tools, continuation coverage and automatic working-memory checkpoints |
| 2026-10-15 | minor | Program Files default, all-users setup, DOCX Default Apps registration and foreign-install protection |
| 2026-10-15 | minor | Native per-user Windows EXE installer, license/folder/components/shortcuts pages and uninstall preserving the user profile |
| 2026-10-15 | minor | WordCraft-owned browser login, automatic credential storage and standalone documentation |
| 2026-10-15 | minor | Default-on ChatGPT web search, referenced file reads and right-click Add to Chat |
| 2026-10-15 | minor | ChatGPT selection references, source navigation and native Markdown/equation previews |
| 2026-10-15 | minor | Removed Stop and Disconnect controls; Clear conversation and Sign out cancel active replies |
| 2026-10-15 | minor | Standalone ChatGPT browser sign-in, automatic credential import and bundled Rust helper |
| 2026-10-15 | minor | ChatGPT sidebar: browser sign-in, one clearable conversation and document editing through engine commands |
| 2026-10-12 | minor | Czech proofing review: short-word splits rank after deletion, NOSUGGEST excludes suggestions while accepting typed forms, affix comments apply only to headers, ratios keep spaces and repeated-word messages use Czech quotes; synthetic engine end-to-end and latency checks |
| 2026-10-12 | minor | Czech spelling from the installed Hunspell dictionary, runtime-table suggestions and generic checks; Czech interface writes cs-CZ documents; no Czech hyphenation patterns bundled; estimates unchanged |
| 2026-10-12 | minor | M15: Portuguese proofing (pt-BR spelling from the installed VERO dictionary, suggestions, grammar, hyphenation; Review › Language gains Português (Brasil)) |
| 2026-10-11 | trivial | M15: Polish interface and Polish proofing landed |
| 2026-10-11 | trivial | M3: icons landed (#309) |
| 2026-10-11 | trivial | M11: contour (Tight/Through) wrap done (#354) |
| 2026-10-11 | minor | Native drawing-level ink and its picture fallback survive DOCX edits and repeated saves; rendering and editing InkML remain missing |
| 2026-10-12 | minor | Stability sweep: local desktop recovery implemented with original-file protection and focused lifecycle tests. Cross-platform field validation and longer soak testing remain. |
| 2026-10-11 | trivial | M6: the eight missing bibliography styles landed (#383) |
| 2026-10-11 | trivial | M7: formatting revisions landed (#41) |
| 2026-10-11 | trivial | Omitted `w:pPrDefault` stays omitted across saves; explicit empty/default properties and later spacing edits remain writable. Synthetic preservation checks only; readiness estimates unchanged |
| 2026-10-10 | trivial | M5: Draw Table and Eraser landed (#303) |
| 2026-10-10 | trivial | Merged main: PRs that landed removed from the Current focus list |
| 2026-10-10 | minor | Current focus heading back to ~150–230 h to beta: full number restored to the additive weighted sum (~60%), method aligned with the standard, no new evidence |
| 2026-10-10 | minor | Alpha gate table added (core-workflow gate from the progress-docs standard): all six workflows pass, stage stays alpha |
| 2026-10-10 | major | Created: milestones moved from ROADMAP.md with remaining hours; Current focus from gaps.md; M15–M16 added |
| 2026-10-10 | trivial | M11: ink pens and eraser landed (#307) |
| 2026-10-11 | trivial | M11: lasso, Add Pen, Ink to Shape and Ink Replay landed (#395) |
