# Windows EXE installer

This fork adds a native Windows setup wizard alongside the upstream MSI packager.
Linux users can continue building and running WordCraft with Cargo.

## Install

Run `wordcraft-<version>-windows-x64-setup.exe` (or the ARM64 build on Windows ARM).
The wizard includes:

1. Welcome and a short explanation of the optional ChatGPT connection.
2. The original MIT and Apache-2.0 licenses; choose either license as permitted by the project.
3. An installation folder chooser. The default is `C:\Program Files\WordCraft` (the system Program Files folder);
   a writable folder on another drive is supported.
4. Standard, application-only or custom components. Command-line tools and the standalone
   ChatGPT sign-in helper are optional; browser login is built into the main application.
5. Start Menu folder selection, an optional desktop shortcut, and optional document **Open with**
   and Windows Default Apps registration, enabled initially. Existing default applications are preserved.
6. A review page, installation progress, and a finish page offering to launch WordCraft or
   open Windows Default Apps settings.

Windows 10 or newer is required. Setup installs for all users and requests administrator
permission to write to Program Files and register the application. The application itself runs
with the signed-in user's ordinary permissions; there is no AppContainer sandbox, background
service, or Rust prerequisite. Launch setup normally and approve the Windows UAC prompt, then
open WordCraft from its shortcut. ChatGPT sign-in and credentials remain separate for each user.

An upgrade of an existing all-users fork installation reuses its chosen directory. If you
installed the earlier per-user version under AppData, uninstall that version before installing
this one; uninstall preserves your user profile and documents. The fork retains a distinct
installer identity from upstream's MSI, but the default Program Files directory can coincide
with upstream's directory. Setup refuses to overwrite another edition's `wordcraft.exe`.
Uninstall the upstream edition first or choose a different folder. Both editions use WordCraft's
existing profile; avoid running both against the same settings simultaneously.

## Make WordCraft the default for DOCX

Leave **Register WordCraft for Open with and Default apps** selected during setup. Then either
select **Choose default apps for documents** on the finish page or open **Windows Settings →
Apps → Default apps**, search for `.docx`, and select **WordCraft with ChatGPT**.
You can also right-click a DOCX file, choose **Open with → Choose another app**, select WordCraft,
and choose the option to always use it. Windows labels vary between versions.

Setup registers supported document types without changing your existing defaults. Windows
requires you to choose the default through its own interface. The fork's registration is
separate from the upstream MSI and is removed on uninstall.

## ChatGPT and private data

Open **ChatGPT → Continue with ChatGPT** after installing and sign in with your own account.
See [ChatGPT setup](chatgpt.md). Installation never logs in or imports a developer's credentials.

Credentials remain in `%APPDATA%\WordCraft\chatgpt-direct`, outside the installation folder,
using the same storage and protection as a Cargo-launched desktop build. Moving the installed
application to another drive does not move the user profile. Browser authorization, the local
callback and outbound HTTPS use the existing native integration; the installer adds no firewall
rules, network proxy or command-execution permissions. Reading referenced files still requires
normal Windows file access. Web search defaults on and can be disabled in ChatGPT settings.

Uninstall from Windows **Settings → Apps → Installed apps → WordCraft (ChatGPT fork)** or its
Start Menu entry. Uninstall preserves documents, settings and saved login. Choose **Sign out**
in WordCraft first if you want to remove its saved ChatGPT credentials. An upgrade retains those
same settings; chat history remains in memory and is not persisted by installation.

The installer contains only explicitly staged release executables, public documentation and
license notices. It never packages a user profile, authentication JSON, `.env`, local configuration,
chat history or personal documents. The optional helper creates JSON only when its user runs it
and completes their own sign-in; no JSON is shipped with it. Advanced users who installed the
command-line tools can run `wordcraft-chatgpt-login.exe` directly from their installation folder
without Cargo. The normal in-app browser login does not need this helper.

## Build on native Windows

Requirements: the MSVC Rust toolchain, Visual Studio C++ build tools and Windows SDK (as for a
normal native Cargo build), and [Inno Setup 6.7 or newer](https://jrsoftware.org/isdl.php).
Only the person building the installer needs these tools.

From this repository in PowerShell:

```powershell
pwsh -File packaging/windows/package-exe.ps1
```

If the compiler is not on PATH or in its usual installation folder:

```powershell
pwsh -File packaging/windows/package-exe.ps1 -IsccPath 'D:\Tools\Inno Setup 6\ISCC.exe'
```

Outputs are in `dist\release` (or `$env:DIST`):

- `wordcraft-<version>-windows-x64-setup.exe`
- A SHA-256 checksum (`.exe.sha256`).
- A build inventory with relative payload names and SHA-256 hashes (`.exe.payload.json`).
  This is build metadata, separate from the installed application; it contains no credentials.

The build uses `--locked --release` and a static C runtime, embeds the original WordCraft icon,
checks executable architecture and GUI/console subsystem, and stages an explicit file allowlist.
License texts are taken directly from the repository and locked dependency sources. The setup
artwork is drawn in code using the existing original WordCraft icon. No Office artwork is used.

Use `-Arch arm64` with the Rust ARM64 MSVC target and corresponding Microsoft ARM64 build tools.
Set `$env:WORDCRAFT_VERSION` to give an installer a fork/revision version (for example
`0.7.0-fork.2`); this changes the setup version and filename, while the packaged application
retains its Cargo version.

`-SkipBuild` packages already-built release binaries from the normal Cargo target directory;
those binaries must have been built with `RUSTFLAGS='-C target-feature=+crt-static'`.
Do not enable `CRAFT_FONTS_DIR` only for packaging: use the same value at build and packaging time
so embedded optional fonts receive their license notices.

The existing `packaging/windows/sign.ps1` signs staged executables and the final setup when
signing credentials are configured. Otherwise it produces an unsigned local build. An unsigned
installer may show **Unknown publisher** or Windows SmartScreen warnings; a trusted public
release requires an appropriate code-signing certificate or trusted signing service. Signing
keys are build-time inputs and never installed with WordCraft.

## Verify a build

```powershell
pwsh -File packaging/windows/test-exe.ps1 -Installer .\dist\release\wordcraft-0.7.0-windows-x64-setup.exe
```

Add `-TestDirectory D:\InstallerTests` to verify installation on another writable drive.
The smoke check uses a unique temporary folder, an isolated synthetic profile and no shortcuts.
It verifies a custom installation path, payload hashes, command-line startup, same-version
reinstallation and uninstall, including preservation of a synthetic profile and an untracked
user document. It refuses to run when this fork is already installed for any tested scope, to
avoid modifying a real installation. It also checks DOCX/Default Apps registration, preserves
the current default choice, and verifies that an unowned WordCraft directory cannot be overwritten. It does not sign in or use real account credentials.

By default the test passes `/CURRENTUSER` and a custom writable directory, exercising the same
installer without requesting elevation. To verify the normal all-users scope, run the test with
`-AllUsers` from an elevated PowerShell in a clean Windows test machine. This checks machine
registration; the interactive default remains Program Files and administrator approval.

For interactive review, launch setup normally and inspect each page. A silent deployment is
also available:

```powershell
.\wordcraft-0.7.0-windows-x64-setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /DIR="D:\Apps\WordCraft ChatGPT"
```

Silent setup never launches WordCraft automatically. Normal interactive setup offers launch on
its finish page. Review privacy and licensing before unattended deployment.
