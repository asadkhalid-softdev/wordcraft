# Dependency notice supplements

Some Cargo packages omit their workspace-root license files. These text files retain the
upstream notices from the exact source revisions recorded in each package's
`.cargo_vcs_info.json`. Each text names its original URLs. `../license-overrides.json` maps
exact package versions to the supplements, revision, sources and SHA-256 (UTF-8, LF newlines).
Whitespace at line endings is normalized; license wording is retained. The installer build verifies those hashes and combines supplements with notices from cached
locked dependencies, including bundled font licenses. The build makes no notice downloads.

These files are upstream license and copyright notices, under their stated licenses. They do
not relicense the application. The enum-map supplements also retain source-file SPDX copyright
headers. No executable code, fonts, images or credentials are included here.

When upgrading a dependency, inspect its packaged notices. If it still omits its original
license files, retrieve the notices from its exact published source revision, preserve the
copyrights and source URLs, and add an exact-version mapping and checksum. A missing notice
stops packaging. Never substitute a current branch's license for a pinned version's license.
