# Synthetic cookie permission proof

This separate GitHub Actions job measures native cookie API visibility. It does
not load or change the production companion, transfer cookies to Hycli, or prove
that a real login works. No website or account is contacted.

## Supported target and safety boundary

The target is GitHub's `windows-2022` runner with the Chromium version installed by
the existing dashboard Playwright lockfile. Playwright documents Windows Server
2019+ support and headless extension loading with bundled Chromium, a persistent
context and `channel: 'chromium'`:

- https://playwright.dev/docs/intro#system-requirements
- https://playwright.dev/docs/chrome-extensions
- https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md

The test explicitly enables `chromiumSandbox`. It does not change machine/browser
policies, AppArmor, sysctl, security permissions or installed browser profiles.
If loading or sandbox initialization is restricted, the job fails rather than
weakening those settings. It cannot select a different executable through an
environment variable. The native runner entry point refuses non-Windows or
non-GitHub-Actions execution. Do not override this guard to work around a managed
desktop policy.

The existing `extension-e2e.mjs` integration test is separate and is not reused:
this proof uses its own minimal synthetic manifest and service worker, not the
production extension or dashboard fixture. It adds no product CLI or MCP tool.

## Actual scope

Three fresh profiles use required manifest grants: none, exact HTTPS www, and
exact HTTPS apex plus www. There are no wildcard-host grants. Synthetic cookies
cover www host-only, apex host-only, parent domain, www domain, unrelated domain,
and a separately keyed partitioned parent cookie. All names and values are fake.

The test first verifies that all fixtures exist, then checks native `getAll`
results with no filter, URL filter, explicit normal store and partition key.
Negative controls reject unrelated-origin access, apex host-only URL leakage,
missing permitted fixtures and partition leakage into default queries. The
www-only parent-domain result is recorded, not assumed. Both possible observations
remain distinguishable in the report with the actual browser version.

Required manifest grants are NOT a test of optional permission prompts, denial,
revocation, private stores, Firefox containers/FPI or branded browsers. Those
remain unverified. A green result is a Chromium normal-store API proof only,
not an all-browser pass and not a universal requirement for apex permission.

Official semantics:
- https://developer.chrome.com/docs/extensions/reference/api/cookies
- https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/cookies

## Run and inspect

Use the `Synthetic cookie permission matrix` workflow on a reviewed repository
revision. It installs dependencies using `npm ci`, then the matching bundled
Chromium. The pure contract tests can run without a browser:

    cd dashboard
    node --test cookie-permission-contract.test.mjs

The native entry point is run only by the dedicated job:

    node cookie-permission-e2e.mjs

The artifact contains only `test-results/cookie-permission-report.json`, including
started/completed cases and any error. Browser profiles are created in a unique
owned temporary directory and removed after context shutdown. Profiles, cookie
values, credentials, and browser traces are not uploaded. A setup failure with no
report is still a failed CI step, never a passing native test.

Until that workflow has actually run, the native matrix is unverified. Passing
pure contract tests validates the report checker, not browser behavior.
