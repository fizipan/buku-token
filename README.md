<h1 align="center">Buku Token</h1>

<p align="center">
  A macOS menu bar app that shows how much quota your AI coding agents have left and what they cost.
</p>

<p align="center">
  <a href="LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <img alt="Platform: macOS" src="https://img.shields.io/badge/platform-macOS-lightgrey">
  <img alt="Built with Tauri 2" src="https://img.shields.io/badge/Tauri-2-24C8DB">
</p>

<p align="center">
  <img src="docs/screenshots/menubar.png" alt="Menu bar item showing the quota left for Claude Code, Codex and Cursor" width="360">
</p>

<p align="center">
  <img src="docs/screenshots/overview-dark.png" alt="Overview tab in dark mode with a cost donut per agent and quota rows" width="340">
  <img src="docs/screenshots/overview-light.png" alt="Overview tab in light mode" width="340">
</p>

<p align="center"><sub>All screenshots use example data.</sub></p>

*Buku* is Indonesian for book. If you run several agents at once, it is easy to lose track of which subscription is about to run out and where the money went. Buku Token reads the logs your agents already write on your Mac and puts the answer one click away. There is no account to create and no telemetry. The interface is available in English and Indonesian.

[Baca dalam Bahasa Indonesia](README.id.md)

## What it shows

- **Quota in the menu bar**: the logo and percentage left for each agent with a quota.
- **Cost today**: a donut split by agent, plus the total in Rupiah and tokens.
- **Quota per agent**: 5-hour and weekly windows, or Cursor's monthly Total, Auto and API, with reset times and a burn projection for Claude.
- **Sessions, models, projects and history**: tokens and API-equivalent cost, broken down each way.
- **Subscription value**: what you pay against what the same usage would cost at API prices.
- **Notifications** (off by default): when a quota drops below 15%, or Claude is projected to run out before it resets.
- **A share card**: an image you can copy or save, with an option to hide costs.

<p align="center">
  <img src="docs/screenshots/projects.png" alt="Projects tab" width="250">
  <img src="docs/screenshots/models.png" alt="Models tab" width="250">
  <img src="docs/screenshots/history.png" alt="History tab" width="250">
</p>

<p align="center">
  <img src="docs/screenshots/share.png" alt="Share panel with a preview of the usage card" width="300">
  <img src="docs/screenshots/settings.png" alt="Settings window" width="380">
</p>

## Supported tools

| Tool | Tokens and cost | Quota | Source |
|---|---|---|---|
| Claude Code | Yes | Yes | `~/.claude/projects`, and Anthropic's usage endpoint with your existing login |
| Codex | Yes | Yes, as fresh as your last session | `~/.codex/sessions` and `~/.codex/archived_sessions` |
| OpenCode | Yes | No | `~/.local/share/opencode/opencode.db` |
| Hermes | Yes | No | `~/.hermes/state.db` |
| Cursor | Yes, after you connect it | Yes, after you connect it | cursor.com, with a session cookie you paste in |
| OpenRouter | Spend and balance only | Key limit, if set | openrouter.ai, with an API key you paste in |
| Antigravity | No, activity only | No | File names and dates, and three columns of its summaries database |
| [RTK](https://github.com/rtk-ai/rtk) | Tokens saved | No | The `rtk` command |

OpenRouter figures are kept out of the totals because OpenCode or Hermes may already have counted the same spend.

## Install

There are no prebuilt releases yet, so you build it yourself. You need macOS (developed on Apple Silicon), Node.js, [pnpm](https://pnpm.io), [Rust](https://rustup.rs) and the Xcode Command Line Tools.

**1. Create a local signing certificate.** You only do this once. A stable identity means macOS remembers your Keychain answer after a rebuild.

1. Open Keychain Access, then choose Keychain Access > Certificate Assistant > Create a Certificate.
2. Set Name to `Buku Token Local`, Identity Type to Self-Signed Root, and Certificate Type to Code Signing.
3. Click Create, then Continue until it finishes.

`security find-identity -p codesigning` will list it as "not trusted". That is expected and does not stop signing. To use another identity, change `bundle.macOS.signingIdentity` in `src-tauri/tauri.conf.json`.

**2. Build it.**

```bash
git clone https://github.com/fizipan/buku-token.git
cd buku-token
pnpm install
pnpm tauri build --bundles app
```

The app lands in `src-tauri/target/release/bundle/macos/Buku Token.app`. Copy it to `/Applications` to keep it.

**3. First run.** macOS asks for access to the Keychain item `Claude Code-credentials`. Choose **Always Allow** to see your Claude quota. If you refuse, everything else still works.

### Connecting Cursor and OpenRouter

Both are optional and stay off until you connect them. Open Settings, go to Agents, and choose **Manage**.

- **Cursor**: log in at cursor.com, open your browser's developer tools, and copy the `WorkosCursorSessionToken` cookie. Cursor encrypts the login it stores on your Mac, so this app does not read that one. Treat the cookie like a password, because it is a full account session.
- **OpenRouter**: paste the API key your tools already use. Usage is counted per key, so a brand-new key shows $0.

## Privacy

Your usage data stays on your Mac, in `~/Library/Application Support/local.toolslab.bukutoken`.

- **Network**: the app only ever talks to four places.
  - Anthropic's usage endpoint, to read your Claude quota.
  - `raw.githubusercontent.com`, for the LiteLLM price list, and only when you press Update.
  - `cursor.com`, once you connect Cursor.
  - `openrouter.ai`, once you connect OpenRouter.
- **Credentials**: the Claude login is read from Keychain, and it is never stored, logged or refreshed. Your Cursor cookie and OpenRouter key sit in Buku Token's own Keychain items. They are checked with the server before being saved, and **Disconnect** deletes them. Keychain items survive uninstalling, so disconnect first.
- **Other tools' files** are opened read-only. Hermes and OpenCode databases are read from a temporary copy that is deleted straight after. Cursor's database and its Keychain are never opened.
- **Antigravity**: only `.pb` file names and dates, plus id, step count and date from its summaries database. Its titles, previews, conversation files and Google token are never read.
- **Project names** come from walking folder names from `/` and checking for `.git`, so macOS may ask for folder access. Project files are not read.
- **The share card** is drawn locally and has no project names, paths, emails or conversation content. Custom model names show as "Other Model", since a name could contain something private.

There is no telemetry or analytics.

## Limitations

- macOS only.
- Cursor's endpoints are undocumented and can change without notice, which would stop its figures until the app is updated. The cookie is not renewed for you, so paste a new one when it expires. Connecting a different Cursor account mixes both accounts' history.
- Codex quota comes from session logs, so it is only as fresh as your last Codex session.
- Antigravity has no token, cost or quota figures here. Its summaries database may lag slightly behind a running app.
- Hiding figures during a screen share is best effort. It uses an undocumented macOS function, looked up at runtime, and can lag a few seconds or stop detecting in-app captures on a future macOS.
- Notifications use an older macOS API, and the app cannot tell whether one was shown. Use **Send Test** in Settings to check.
- Costs are estimates based on API prices unless the tool reports its own, and models with no known price show "–" rather than $0.

## Development

```bash
pnpm tauri dev                     # run the app
pnpm vitest run                    # frontend tests
pnpm tsc --noEmit                  # type check
cd src-tauri && cargo test         # Rust tests
cd src-tauri && cargo clippy --all-targets -- -D warnings
```

Built with Tauri 2, Rust, React, TypeScript, Tailwind CSS, shadcn/ui and Recharts. See [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request, and [SECURITY.md](SECURITY.md) to report a vulnerability.

## Credits

- [OpenUsage](https://github.com/robinebers/openusage) was the inspiration for the share feature and Settings layout, and the source of the Antigravity logo.
- Prices come from [LiteLLM](https://github.com/BerriAI/litellm), and most logos from [LobeHub Icons](https://github.com/lobehub/lobe-icons).

## License

[MIT](LICENSE). Product names and logos belong to their owners and are used only to identify each tool. This project is not affiliated with or endorsed by any of them.
