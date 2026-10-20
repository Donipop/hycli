# Portable website actions

Hycli normally prepares this definition itself from the user's URL and intent. This reference is for agents and contributors that need to inspect or extend an integration. A definition describes evidence-backed operations; it contains no executable scripts, credentials or machine paths.

To add missing functionality without authoring a schema, use `hycli request SITE "Add draft editing and publishing with full post content"` (alias: `extend`). MCP exposes the same operation as `hycli_request` with `site` and `instruction`. The job reports `added_actions`, `available_actions` and `capability_gaps`; existing installed actions are preserved.

Validate with `hycli spec validate FILE`, install with `hycli spec install FILE`, and inspect the resulting interface with `hycli describe SITE ACTION` or `hycli SITE ACTION --help`. Use an isolated synthetic server for mutation tests.

## Requests

The root has `spec_version` (`1`, `2`, or `3` for private pagination), `site` (`name`, `title`, `base_url`, optional `source_url`), optional `defaults.headers`, `auth`, and `operations`. Each operation has a unique `name`, `desc`, `method`, `/path`, `effect`, and `evidence`. `effect` is `read`, `write`, or `unknown`; an agent's declaration never overrides the runtime's classification.

| Field | Meaning |
| --- | --- |
| `params` | Named `{variables}` in the path, escaped as individual segments. |
| `preflight` | Optional `spec_version: 2` read-only GET steps (`name`, `path`, `extract`) that extract dynamic tokens, CSRF nonces, or `buildId` values (`json:/pointer`, `ssr:<key>#<pointer>`, `css:selector@attr`, `header:name`, `cookie:name`) for `{{step.<name>.<var>}}` interpolation across `path`, `query`, `headers`, and `body`. |
| `query` | URL query parameters (supports `{{step.<name>.<var>}}` and `{{<input>}}` interpolation in `spec_version: 2`). |
| `body.json` | Fields serialized in a JSON object. |
| `body.json_template` | Optional `spec_version: 2` nested JSON structure interpolating `{{<input>}}` and `{{step.<name>.<var>}}` values. |
| `body.form` | Fields encoded as `application/x-www-form-urlencoded`. Choose one body encoding. |
| `base_url` | Optional operation-specific server supported by published evidence. |
| `transport` | Omit or use `http` for HTTP. `browser` is for rendered HTML GET reads. |
| `response` | Expected format, required structure, optional SSR JSON extraction (`ssr_json`, `extract_pointer`, `strip_prefix`), and optional HTML extraction. |

Input definitions contain `type` (`string`, `int`, `float`, `bool`, `json`), `required`, and optional `default`. Required inputs have no defaults. `json` accepts a nested object or array as one input, such as a GraphQL `variables` object. Input names are exact: a literal hyphenated name is preserved. CLI `--arg 'name=value'` splits on the first equals sign.

GraphQL query documents are parsed independently of the endpoint's name. One evidenced query can run as a read; mutations, subscriptions, malformed candidate documents and ambiguous multi-operation documents require review. Form search endpoints can also use a documented POST read.

Use documented page, cursor, offset and limit inputs to navigate results. Return page information or an observed next-page link and continue only as far as the user's task requires. Do not invent cursor values or unobserved endpoints.

## HTML results

An HTML action declares `response.format: html` and `required_html_fields`: observed `{selector, attribute}` fields that must each select a unique, nonempty value. Empty `attribute` means text. Choose a page-specific field that distinguishes useful content from a login page.

`response.html` can turn a page into structured output:

```yaml
response:
  format: html
  required_html_fields:
    - selector: main h1
  html:
    items: article.reference
    fields:
      title:
        selector: h2 a
        required: true
      url:
        selector: h2 a
        attribute: href
        absolute_url: true
    page:
      next_url:
        selector: a.next
        attribute: href
        absolute_url: true
    limit: 100
```

These selectors are illustrative, not installed website definitions. Selectors must come from the observed page. `items` selects repeated records; fields are relative to each record. An empty field selector selects the record itself. `page` extracts page-level values. Missing optional fields become null. Missing or ambiguous required fields fail validation of the response. `absolute_url` resolves safe HTTP(S) links against the final response URL. Results are bounded to 100 records by default and at most 1,000 when explicitly configured.

For JavaScript-rendered pages, set `transport: browser`, `method: GET` and the same HTML expectations. Hycli discovers a local Chromium engine and restores the selected verified session if needed. It navigates and extracts rendered content; it does not click buttons or submit browser forms. Raw HTTP remains available without a browser engine.

## Accounts and secrets

Public operations omit `auth`. Header authentication declares a named strategy with `kind: header`, the documented `header`, optional `prefix`, and `value_from: store:SITE/KEY`. The user supplies the value in **Website API keys**. A browser strategy uses `kind: browser` and a verified account from the local vault. A `spec_version: 2` derived strategy uses `kind: derived`, `header`, optional `prefix`, and `value_from: cookie:<name>`, `storage:<key>[#<json_pointer>]`, or `header:<captured_name>` to project a verified browser session value into a request header without exposing secrets to prompts or outputs. Operations refer to a strategy by name.

An operation on a different API origin does not inherit the website's cookies or token. Header authentication has its own exact origin scope. A website's browser login cannot substitute for a separately documented API key. Generated recipes reference local key names and structure while credential values stay outside model prompts and exports.

## Verification and limits

JSON expectations use `format: json` and observed JSON pointers in `required_pointers`. Other formats are `text`, `xml`, and `empty`. A non-success HTTP response or a successful response of the wrong expected shape fails the action and CLI exit status. Preparation never verifies a mutation by running it.

The current request contract covers JSON and URL-encoded bodies, typed path/query inputs, HTML extraction and rendered GET reads. Multipart uploads, binary file downloads, arbitrary browser click sequences and automatic streaming are not implemented. Preparation must report a concrete missing capability for those workflows; it must not turn them into a fictitious JSON request or claim completion.

## Private pagination (version 3)

An HTTP GET read operation may declare `pagination: {input: cursor, query: pageToken, next: /nextPageToken}`. Use `spec_version: 3`, so older binaries reject the definition instead of silently losing the privacy contract. Do not also declare `cursor` or `pageToken` in ordinary query inputs. The input name is an optional local handle parameter, `query` is the documented wire parameter, and `next` is an exact JSON pointer. Absent, null or empty continuation fields end pagination. Non-string, shorter than 8 characters, larger than 4096 bytes, credential-shaped or known-secret values are rejected with `pagination_invalid`. Short page-number markers are deliberately unsupported: hiding their literal echoes could silently corrupt ordinary content. This size bound is not an entropy or credential-safety guarantee. A continuation echoed in a JSON key is rejected rather than rewriting keys and corrupting the payload or model paths.

Run the first page through the existing action without `cursor`. Paginated direct CLI/action-job output is `{data, next_cursor}`; `try` and MCP retain their normal result envelope with an additional `next_cursor`. Pass that opaque handle to the same action's `cursor` input for the next page. No new command or automatic page following is involved. Legacy nonpaginated output is unchanged.

The raw continuation stays in the existing private local credential store, using its existing protection level (OS-keyring encryption when available, otherwise private-file protection). It never appears in public action results, mapped output, exported definitions or job inputs. Handles expire after 15 minutes and are bound to the exact site, action, definition, account/credential context, URL and noncursor arguments. Credential or definition changes retire handles; retries may reuse a handle until its original expiry. Restart preserves unexpired handles. Expired records are cleaned on runtime startup/access, and at most 256 bounded records are retained. An expired or mismatched handle requires restarting at the first page.

This initial contract supports direct HTTP GET only. It does not support bodies, preflight steps, browser transports, transformed HTML responses or redirects. Global credential redaction and request policy remain enabled; a pagination declaration cannot turn a mutation into a read or send a handle to another operation/account.

## Explicit OAuth refresh bindings

A header strategy can opt into bounded OAuth refresh without changing legacy API-key behavior. This is an explicit provider contract, not automatic OAuth discovery or browser login. Use only an approved existing grant. Hycli does not create grants, request additional scopes or change website write settings.

```yaml
auth:
  - name: api
    kind: header
    header: authorization
    prefix: "Bearer "
    origin: https://api.example.test
    oauth_refresh:
      grant_ref: account-one
      token_endpoint: https://auth.example.test/token
      identity_endpoint: https://api.example.test/me
      subject_pointer: /sub
      resource: https://api.example.test
      scopes: [read]
```

Do not declare `value_from` for this strategy. The opaque `grant_ref` identifies one private record within this site. Reusing it across strategies is invalid. Older binaries require `store:` for header strategies and therefore reject this definition instead of treating grant JSON as an API token. Endpoint pins, the exact resource origin and scope names are public configuration. Client credentials, access/refresh tokens and the expected stable subject stay private.

Configure it with the existing `hycli auth set <site>/api` command and JSON on stdin. `--value` is rejected for an OAuth binding. The JSON shape is:

```json
{
  "kind": "oauth_refresh",
  "client_id": "provider-client-id",
  "client_secret": "optional-client-secret",
  "access_token": "current-access-token",
  "refresh_token": "current-refresh-token",
  "expires_at": 2000000000,
  "subject": "stable-provider-subject"
}
```

`expires_at` is an absolute Unix timestamp in seconds. Omit `client_secret` for a public client. This initial contract supports standard refresh-token form POSTs with client credentials in the form body and Bearer access tokens. The configured `resource` is a local continuity binding, not an OAuth extension parameter; refresh requests do not send `resource` or `audience`. It does not support arbitrary provider authentication schemes. Use a protected stdin source rather than pasting real secrets into shell arguments or history. Malformed typed input is rejected and never falls back to a raw API-key write. `hycli auth rm <site>/api` removes the grant through the same ownership lock.

The configured identity endpoint must return JSON over a nonredirecting GET. The configured subject pointer must yield the expected stable string subject. Emails and unverified JWT claims are not identity evidence. Optional `client_pointer`, `resource_pointer` and `scopes_pointer` compare provider-supported identity fields against the private client ID, configured resource and approved scopes. The optional scope field is a space-separated string. Do not invent fields that the provider does not expose. Client, resource, endpoints and approved scopes otherwise remain pinned to the grant configuration. A returned token response with a changed client, resource or scope is rejected. Omitted `scope` retains the approved scope set; omitted `refresh_token` retains the preceding refresh token.

Only direct read operations are supported. Browser transports, preflight steps and mixing a browser account into the operation are rejected. Token exchanges do not enable website writes. Token and identity requests never redirect. Token POSTs have no automatic transport retry. A valid verified access token makes no token exchange. An expired token or explicit Bearer `invalid_token` 401 may consume one refresh budget per operation. Only that explicit 401 can trigger one safe-read retry; ordinary 403s and challenges never trigger refresh or browser fallback.

One cross-process grant lock covers pending-state persistence, one bounded exchange and atomic pair replacement. No global vault lock spans network waits. Rotation writes both tokens before identity verification or normal resource use. Temporary identity failures retain a verification-pending pair; another read verifies that pair rather than refreshing it again while it is valid. If the pending access token expires, one bounded refresh can replace it, followed by identity verification. Identity or scope changes prevent resource use. A lost response or crash after exchange dispatch requires fresh grant recovery because the old refresh token might already have been consumed. A proven failure before dispatch preserves the unconsumed grant. An uncertain local commit is read back, never rolled back from a stale in-memory snapshot or followed by another exchange.

Grant storage keeps the existing vault protection contract. It uses OS-keyring encryption where available and private-file protection otherwise. An existing encrypted vault never falls back to plaintext when its key is unavailable. The actual protection mode remains visible through existing vault status. The existing store initialization behavior is unchanged; this feature adds no keyring setup flow or OS security-setting changes.

Private pagination handles survive verified token rotation within the same immutable grant owner and configuration. Replacing or removing the grant, changing the definition or changing verified authority invalidates them. An unverified rotated candidate never supplies a resource request. Legacy credential and pagination behavior is unchanged.
