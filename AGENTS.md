# Clipboard Avenger release versions

The current release is **1.0.2**. The next release is **1.0.3**.

Follow the user's numbering rule for each subsequent app release: increment the
last digit, rolling 1.0.9 over to 1.1.0, then 1.1.9 to 1.2.0, and 1.9.9 to 2.0.0.
Do not use 1.0.10. Multiple edits for one release share that release's version.

Keep Cargo.toml, the clipboard-avenger package entry in Cargo.lock, and
VERSIONING.md and the version references in this file in sync. Documentation-only
changes do not advance the app version. Rebuild the EXE and desktop ZIP when publishing a new app
release so their version metadata matches the source. The GitHub repository
contains source and assets only; do not upload EXEs or ZIPs.

Update the Changelog sections in README.md and README.txt for each app release,
describing actual changes since the previous recorded release. Do not invent
changes for version numbers that were not separately recorded.
