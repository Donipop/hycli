<p align="center">
  <img src="docs/brand/concepts/hycli-shima-banner-v4.png" alt="Hycli — Websites, ready for AI." width="100%" />
</p>

<p align="center">
  <strong>Automate any website.</strong>
</p>

<p align="center">
  <a href="https://github.com/Hybirdss/Hycli/releases/tag/v0.1.0"><img src="docs/brand/badges/release.svg" alt="Release 0.1.0" /></a>
  <a href="LICENSE"><img src="docs/brand/badges/license.svg" alt="Apache-2.0 license" /></a>
  <a href="#use-hycli-with-your-ai"><img src="docs/brand/badges/interface.svg" alt="CLI and MCP" /></a>
  <a href="#languages"><img src="docs/brand/badges/languages.svg" alt="20 languages" /></a>
</p>

<p align="center">
  <a href="https://github.com/Hybirdss/Hycli/releases/tag/v0.1.0">Download</a> ·
  <a href="#get-started">Get started</a> ·
  <a href="#what-your-ai-can-do">What your AI can do</a> ·
  <a href="#common-concerns">Common concerns</a> ·
  <a href="#use-hycli-with-your-ai">Use with your AI</a> ·
  <a href="docs/i18n/README.ko.md">한국어</a> ·
  <a href="docs/i18n/README.ja.md">日本語</a>
</p>

<details>
<summary>Read in your language · 20 languages</summary>

[English](README.md) · [한국어](docs/i18n/README.ko.md) · [日本語](docs/i18n/README.ja.md) · [简体中文](docs/i18n/README.zh-CN.md) · [繁體中文](docs/i18n/README.zh-TW.md) · [Español](docs/i18n/README.es.md) · [Français](docs/i18n/README.fr.md) · [Deutsch](docs/i18n/README.de.md) · [Português (Brasil)](docs/i18n/README.pt-BR.md) · [Bahasa Indonesia](docs/i18n/README.id.md) · [Italiano](docs/i18n/README.it.md) · [Nederlands](docs/i18n/README.nl.md) · [Polski](docs/i18n/README.pl.md) · [Türkçe](docs/i18n/README.tr.md) · [Русский](docs/i18n/README.ru.md) · [Українська](docs/i18n/README.uk.md) · [Tiếng Việt](docs/i18n/README.vi.md) · [ไทย](docs/i18n/README.th.md) · [العربية](docs/i18n/README.ar.md) · [हिन्दी](docs/i18n/README.hi.md)

</details>

Give Hycli a website and an optional goal. It learns the site's interface, prepares typed CLI and MCP actions (both reads and writes), and verifies reads safely. Your AI can then use those actions with the same accounts, write permissions, and results you see in the local dashboard.

| Connect | Prepare | Use |
| --- | --- | --- |
| Add a website, choose your AI and describe what you want to do. | Follow three workers as they inspect, connect and verify. | Use the dashboard, terminal or your connected AI assistant. |

<p align="center">
  <img src="docs/images/dashboard.png" alt="Hycli example workspace with Discord, GitHub, Notion, Naver Blog, Netflix and Slack." width="100%" />
</p>
<p align="center"><sub>Example workspace with sample tools and accounts.</sub></p>

<a id="get-started"></a>
## Get started

Hycli packs its website engine and local dashboard into a single executable. Rust and Node.js are only needed when building from source.

### Download and run

Desktop packages are built for **Windows, macOS and Linux** (**x64 and ARM64**) via [native CI](https://github.com/Hybirdss/Hycli/actions/workflows/verify.yml) (see [distribution signing](docs/RELEASING.md#desktop-installers-and-signing)):

| Platform | Package | Open |
| --- | --- | --- |
| Windows | `*-setup.exe` | Install for your account, then open Hycli from Start or the desktop. |
| macOS 13+ | `.dmg` with `Hycli.app` | Drag Hycli to Applications, then open it. |
| Linux | `.deb` and `.tar.gz` | Install the DEB, or extract the archive and run `./install.sh`. |

For the verified **[Linux x64 v0.1.0 release](https://github.com/Hybirdss/Hycli/releases/tag/v0.1.0)** (requires glibc ≥ 2.39, e.g., Ubuntu 24.04):

```sh
sha256sum -c hycli-0.1.0-linux-x64.tar.gz.sha256
tar -xzf hycli-0.1.0-linux-x64.tar.gz
cd hycli-0.1.0-linux-x64
./hycli dashboard
```

<details>
<summary><strong>Build from source</strong> · Linux, macOS and Windows</summary>

Install the [build prerequisites](docs/BUILD.md) and run:

```sh
node scripts/build.mjs
./dist/hycli open
```

`node scripts/package-desktop.mjs` creates the native installer for your OS. Add `--install-dir <directory>` to install on your PATH. See [building and packaging](docs/BUILD.md) and the [release checklist](docs/RELEASING.md).

</details>

### Connect your first website

Hycli opens at `http://127.0.0.1:4318` (or a free loopback port). Use `hycli open`, `hycli status`, and `hycli stop` to manage the background app, or `hycli open --no-open --port 4320` for headless setup.

1. Open **AI connections** and connect a provider.
2. Add a website address, choose the AI, and optionally describe your task.
3. Review the prepared actions. Run one in the dashboard or open **Coding agents** to copy the MCP config once.

For signed-in sites, connect your account under **Accounts** or save API keys under **Explore actions → Website API keys**.

<a id="what-your-ai-can-do"></a>
## What your AI can do

Hycli turns website operations into named, typed tools with clear inputs and outputs:

| Website interface | Prepared capability |
| --- | --- |
| REST and OpenAPI | Typed requests grounded in published JSON/YAML schemas and documentation. |
| GraphQL | Parsed query/mutation documents and JSON variables. |
| HTML & JS-rendered pages | Structured records, links, pagination, and rendered GET reads via a local Chromium engine. |
| JSON and URL-encoded forms | Documented inputs, defaults, and account-scoped read/write requests. |

<table>
<tr><td width="50%"><img src="docs/images/ai-connections.png" alt="Real Hycli AI connection screen using demonstration data." /></td><td width="50%"><img src="docs/images/action-result.png" alt="A completed action in the real Hycli dashboard using a synthetic local service." /></td></tr>
<tr><td align="center">Choose the AI that prepares your websites.</td><td align="center">Review actions and keep the useful result.</td></tr>
</table>

Three independent AI workers handle reads/search, workflows, and account evidence. They inspect linked docs, API schemas, frontend JS bundles, and live browser network shapes, verifying reads before installing the CLI.

<p align="center">
  <img src="docs/images/preparation.gif" alt="Three workers. One very busy little bird." width="100%" />
</p>
<p align="center"><sub>Three workers. One very busy little bird.</sub></p>

| Action | Behavior |
| --- | --- |
| Read or search | Runs autonomously when grounded in evidence and classified as a read. |
| Create, send, edit or delete | Prepared into the CLI without ever running during discovery. Disabled by default (`writes: false`). Your AI agent asks you **once per CLI** whether to allow writes (`hycli writes SITE allow` or MCP `hycli_writes`). Once enabled, write actions execute directly when you ask for them. |
| Unclear effect | Held for review before sending. |
| Auth challenge or rate limit | Pauses affected requests or pivots transport strategy so you can reconnect or wait. |

See the [operation reference](docs/SITESPEC.md) for supported request shapes and extraction rules.

<a id="common-concerns"></a>
## Common concerns and how Hycli solves them

Connecting an AI agent to real websites and logged-in accounts raises natural concerns. Hycli is designed around strict boundaries for each one:

### 1. Unauthorized writes & accidental mutations
- **Zero live mutations during CLI creation:** While preparing or checking a website (`hycli prepare`, `hycli request`, `hycli recon`, `hycli check`), Hycli discovers and synthesizes both read and write operations into the CLI spec, but **never executes write requests**. Even disguised mutations (`GET` routes with mutating names like `/delete`, GraphQL `mutation` bodies, or HTML `<form method="post">` actions) are blocked from running during verification.
- **One-time per-CLI write permission:** Every newly created website CLI starts with `writes: false`. Before an AI agent executes a write action on a CLI for the first time, it asks you **once for that CLI**: *"Allow write actions for `<site>`?"* Upon your confirmation, it runs `hycli writes <site> allow` (or MCP `hycli_writes`), after which requested write actions run directly. You can revoke write permission anytime with `hycli writes <site> deny` or the dashboard's **Allow changes** toggle.
- **No automatic write retries:** Failed or interrupted write requests are never auto-retried. If a connection drops after a mutation is sent, Hycli returns `action_outcome_unknown` so you can check the website rather than duplicating a post or payment.

### 2. Login & credential privacy
- **Secrets never enter AI prompts:** Browser cookies, `localStorage` / `sessionStorage` tokens, and API keys are stored strictly in your local vault (encrypted via the OS keyring when available, or restricted to `0600` user-only permissions).
- **Symbolic connection recipes:** Preparation models only see account labels, storage key names, and redacted schemas. The AI constructs a symbolic `ConnectionRecipe` (referencing a cookie or token name), and Hycli injects the actual credential locally at runtime while scrubbing secrets from all responses and logs.
- **Non-destructive browser import:** Importing a session reads only the target website's cookies/storage and verifies the account against a real profile endpoint. It never weakens browser security or exports unrelated domains.

### 3. Scraping, CAPTCHAs & account bans
- **No fuzzing or brute-force scanning:** Hycli never guesses wordlists or fuzzes live servers. It extracts endpoints from docs, schemas, frontend bundles, and observed browser traffic.
- **Paced, serialized requests:** Traffic is serialized and rate-limited per website and per account, honoring `Retry-After` and bounding read retries.
- **Strategy circuit breaker & native browser execution:** When a plain HTTP request hits a 403/429 or bot challenge, Hycli never loops the same blocked method. Its circuit breaker immediately switches strategy—executing `window.fetch` directly inside your authenticated browser session or using a local stealth Chromium engine—or pauses cleanly to protect your account.

### 4. Cross-origin leaks & machine-specific data
- **Strict origin binding:** Auth headers and cookies are locked to the target origin and stripped immediately on cross-origin redirects. Private/loopback networks are blocked by default unless `--allow-local` is set.
- **Portable specs:** Local browser executable and profile paths remain in machine-local state and are never embedded in shareable website specs.

### 5. Blind retries when sessions expire or sites change
- **AI-free health check (`hycli check`):** Before wasting tokens or hitting login pages repeatedly, `hycli check` (and MCP `hycli_check`) replays verified read chains without an LLM and cleanly distinguishes `signed_out` (reconnect account) from `site_changed` (repair spec), `blocked`, and `unreachable`.

<a id="ai-connections"></a>
## AI connections

| Connection | Sign-in |
| --- | --- |
| ChatGPT · API key | Your OpenAI API key |
| Claude | Your Anthropic API key |
| xAI | Your xAI API key |
| Z.ai / Z.ai Coding Plan | Your Z.ai API key |
| ChatGPT · login | ChatGPT sign-in managed by the installed Codex app-server |

The Codex connection uses its official app-server protocol in an ephemeral thread with file, shell, browser, and external MCP execution disabled; Hycli never reads or copies Codex OAuth tokens.

<a id="use-hycli-with-your-ai"></a>
## Use Hycli with your AI

Copy the MCP configuration from **Coding agents → View connection settings** once:

```json
{
  "mcpServers": {
    "hycli": {
      "command": "hycli",
      "args": ["mcp"]
    }
  }
}
```

Use `hycli mcp --sites-only` to expose only installed website actions, or add `--only-site SITE` to scope to a single site. The MCP server provides `hycli_prepare`, `hycli_writes` (for enabling writes once per CLI after asking you), `hycli_run`, `hycli_check`, `hycli_result`, and `hycli_activity`—and never exposes a tool for reading raw credentials.

From the terminal, the same workflow is available directly:

```sh
hycli prepare https://your-website.example --intent "Find saved references"
hycli request SITE "Add draft editing and publishing with full post content"
hycli describe SITE
hycli SITE ACTION --help
hycli writes SITE allow
hycli SITE ACTION --query "design systems"
hycli check SITE
```

See the [Hycli skill](skills/hycli/SKILL.md) and [agent guide](agent/AGENTS.md) (`CLAUDE.md`) for how coding agents discover capabilities, present available read/write actions to you, ask once per CLI before enabling writes, and recover from failures.

<a id="browser-sign-in"></a>
## Browser sign-in

Hycli discovers local Chrome, Chromium, Edge, Brave, and Firefox profiles (plus local stealth Chromium binaries when installed) and imports only the requested website's session into the local vault. Profiles sharing the same verified website account are deduplicated automatically.

If a session is protected by OS App-Bound encryption, private containers, or an unimported browser, use **Accounts → Connect an account** to sign in via your browser or connect the [browser companion extension](browser-companion/guide.html) (or import a JSON/Netscape cookie file in the dashboard—never paste cookies into an AI chat).

<a id="languages"></a>
## Languages

The dashboard and AI-generated action descriptions support the 20 languages linked at the top of this README, including right-to-left Arabic.

<a id="local-data"></a>
## Local data and request limits

Website definitions, encrypted credentials, and activity logs live in your local data directory (override with `HYCLI_DATA_DIR`). Inspect active vault protection anytime under **Settings**.

<a id="contributing"></a>
## Development and verification

See [CONTRIBUTING.md](CONTRIBUTING.md) and [CHANGELOG.md](CHANGELOG.md). Run the full verification suite locally with:

```sh
npm --prefix dashboard ci
npm --prefix dashboard run check
npm --prefix dashboard run build
cargo test --all-targets --locked
cargo build --locked --bin hycli --example dashboard_fixture
node scripts/test-e2e.mjs --full
```

All tests run against synthetic local websites and fixtures—never test write operations against a real third-party account.

## License and intended use

Hycli is licensed under [Apache-2.0](LICENSE). Use it only with websites and accounts you are authorized to access. Provided "as is" without warranty; the authors and contributors are not liable for any losses, account restrictions, or bans resulting from its use.

## Star History

<a href="https://www.star-history.com/#Hybirdss/Hycli&Date">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=Hybirdss/Hycli&type=Date&theme=dark" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=Hybirdss/Hycli&type=Date" />
   <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=Hybirdss/Hycli&type=Date" />
 </picture>
</a>
