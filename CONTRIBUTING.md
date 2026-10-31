# Contributing

Bug reports, fixes and support for more tools are all welcome. For anything large, open an issue first so we can agree on the approach before you spend time on it.

## Setup

Follow the install steps in the [README](README.md), then run `pnpm tauri dev`.

## Before opening a pull request

Run all of these. They should pass with no warnings:

```bash
pnpm vitest run
pnpm tsc --noEmit
cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings
```

## Rules that keep the project what it is

- **Local first.** No telemetry or analytics. A new network host needs a good reason, and it must be listed in the README's Privacy section.
- **Other tools' files are read-only.** Never write into another tool's folder, and don't open SQLite files in a way that creates `-wal` or `-shm` files next to them.
- **Don't read conversation content.** Parse only what usage figures need.
- **Credentials stay out of logs, errors, IPC responses and test output.** Tests never touch the real Keychain, clipboard, Downloads folder, notifications or network, and fixtures use invented data.
- **Never show an unknown price as $0.** Show "–" instead.
- **Nothing private reaches the share card.** That means no project names, paths, emails or custom model names.
- **Text goes in both languages.** Add new interface text to `src/lib/i18n/` or `src-tauri/src/i18n.rs` in English and Indonesian.

## Adding a tool

Tokens and cost usually start in `src-tauri/src/ingest/`, and quota in `src-tauri/src/quota/`. Mention in your pull request what the tool stores, where you read it from, and how you checked the numbers against the tool's own figures.

By contributing, you agree that your work is released under the [MIT License](LICENSE).
