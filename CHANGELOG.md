# Changelog

## Unreleased

Automate any website, with changes only where you allow them.

### Safety

- Every website starts read-only. Its change actions are hidden from MCP and refused by the CLI with `writes_disabled` until you turn on **Allow changes** for that website in the dashboard. Turning it off cancels pending approvals.
- The dashboard grants its session only to the address `hycli dashboard` opens. A plain local request can no longer obtain the session that approves changes. The session persists in the private data directory, so an open dashboard still reconnects after a restart.
- Values extracted by `preflight` steps or `derived` authentication are unconditionally treated as secrets for output redaction, and `{{step.*}}` interpolation is restricted strictly to request headers and bodies (never URL paths or query strings).

### Deterministic Recon & SiteSpec v2

- `hycli recon <URL>` and MCP `hycli_recon` deterministically inspect SSR state payloads (`__NEXT_DATA__`, `__NUXT_DATA__`, Remix, Apollo, JSON-LD) and same-site JavaScript bundles, and can emit a starter `SiteSpec v2` draft without requiring a built-in LLM key.
- `SiteSpec v2` adds `derived` authentication (`cookie:<name>`, `storage:<key>`, `header:<name>`), `preflight` step extraction (`json:/pointer`, `css:selector@attr`, `header:name`, `cookie:name`), `body.json_template`, and response preprocessing (`strip_prefix`, `ssr_json`, `extract_pointer`).
- Same-site (`eTLD+1`) subdomain API hosts share verified browser cookies and origin storage within the same site integration, and Chromium observation captures preflight `fetch`/`XHR` headers and bodies via CDP `Fetch.requestPaused` without executing state-changing requests.

### Checks without AI

- `hycli check [SITE]` and MCP `hycli_check` replay the reads that passed during preparation, including chained reads, after confirming the saved account. Verdicts separate `signed_out` from `site_changed`, `blocked` and `unreachable`, and the exit status follows the verdict. It replaces `hycli health`, which remains an alias.

### Complete website CLIs

- Preparation plans complete user workflows, reviews every step, and verifies chained reads before installing.
- After the workflows, workers continue through documented API operations that have no action yet. The job reports `coverage` with any remaining operations.

### Native desktop & installers

- Open Hycli from a native app icon or `hycli open` without keeping a terminal or manually starting a server. Reuse running instances and select a free local port when necessary.
- Quit from Settings in all 20 languages, or use `hycli status` / `hycli stop`. Preserve active work and saved data.
- Build per-user Windows installers, macOS app/DMG packages and Linux DEBs, plus portable archives with a Linux menu installer, for x64 and ARM64.
- Verify background lifecycle and actual installer payloads in each native CI job. Support optional Windows signing and macOS Developer ID signing/notarization.

## 0.1.0

The first packaged Hycli release: websites, ready for AI.

### Website tools

- Prepare typed CLI and MCP actions from a website URL and an optional task.
- Use REST, JSON and YAML OpenAPI, GraphQL, URL-encoded forms, HTML extraction and rendered Chromium GET reads.
- Follow three preparation workers, verify useful reads and repair definitions using observed failures.
- Keep the existing working definition while preparing a replacement.

### Your local workspace

- Manage websites, accounts, provider connections, results and approvals from the embedded dashboard.
- Connect multiple accounts and import available browser sessions, origin storage or API credentials.
- Connect AI assistants through MCP, with generic execution for newly installed actions.
- Use 20 dashboard languages, including right-to-left Arabic, with matching README entry points.

### Packaging and verification

- Ship a single native executable with the dashboard and instructions embedded.
- Publish a verified Linux x64 archive and a source snapshot with manifests and SHA-256 checksums.
- Exercise CLI, MCP, dashboard, lifecycle, provider, layout and browser-companion flows against isolated fixtures.
- Define native x64 and ARM64 CI jobs for Linux, macOS and Windows; consult the actual workflow results for platform verification.

The Linux download requires glibc 2.39 or newer. Arbitrary browser click sequences, multipart uploads and binary downloads are outside the current operation contract. See [SITESPEC.md](docs/SITESPEC.md) for supported behavior and [the release](https://github.com/Hybirdss/Hycli/releases/tag/v0.1.0) for downloadable assets.
