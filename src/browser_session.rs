//! Restore one site's session in a private browser and observe its own read requests.
use crate::{
    accounts::{self, Credential, Identity, Observation},
    apperr::{AppError, AppResult},
    util,
};
use chromiumoxide::{
    Browser, Page,
    browser::BrowserConfig,
    cdp::browser_protocol::{fetch, network},
};
use futures_util::StreamExt;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

pub const STEALTH_INIT_SCRIPT: &str = r#"try {
  delete Object.getPrototypeOf(navigator).webdriver;
  Object.defineProperty(navigator, 'webdriver', { get: () => false, configurable: true });
} catch (_) {}
try {
  if (!window.chrome) {
    Object.defineProperty(window, 'chrome', {
      value: { runtime: {}, app: { isInstalled: false }, csi: () => ({}), loadTimes: () => ({}) },
      configurable: true
    });
  }
} catch (_) {}
try {
  if (!navigator.languages || navigator.languages.length === 0) {
    Object.defineProperty(navigator, 'languages', { get: () => ['en-US', 'en'], configurable: true });
  }
} catch (_) {}
try {
  if (navigator.plugins && navigator.plugins.length === 0) {
    Object.defineProperty(navigator, 'plugins', {
      get: () => [{ name: 'PDF Viewer', filename: 'internal-pdf-viewer', description: 'Portable Document Format' }],
      configurable: true
    });
  }
} catch (_) {}
try {
  if (window.Notification && window.Permissions && Permissions.prototype.query) {
    const origQuery = Permissions.prototype.query;
    Permissions.prototype.query = function(parameters) {
      return parameters && parameters.name === 'notifications'
        ? Promise.resolve({ state: Notification.permission, onchange: null })
        : origQuery.apply(this, arguments);
    };
  }
} catch (_) {}
try {
  if (!window.__hycli_apis && typeof window.fetch === 'function') {
    window.__hycli_apis = [];
    const origFetch = window.fetch;
    window.fetch = async function(...args) {
      const req = args[0];
      const init = args[1] || {};
      const url = typeof req === 'string' ? req : (req && req.url ? req.url : '');
      const method = String((init && init.method) || (req && req.method) || 'GET').toUpperCase();
      const body = typeof init.body === 'string' ? init.body.slice(0, 4096) : '';
      const resp = await origFetch.apply(this, args);
      try {
        if (window.__hycli_apis.length < 40 && resp && resp.ok) {
          const ct = (resp.headers && resp.headers.get && resp.headers.get('content-type')) || '';
          if (ct.includes('json')) {
            const clone = resp.clone();
            clone.json().then((data) => {
              if (window.__hycli_apis.length < 40) {
                window.__hycli_apis.push({ url, method, status: resp.status, body, data });
              }
            }).catch(() => {});
          }
        }
      } catch (_) {}
      return resp;
    };
  }
} catch (_) {}"#;

pub fn is_cloakbrowser_engine(engine: &crate::environment::BrowserEngine) -> bool {
    engine.name.eq_ignore_ascii_case("CloakBrowser")
        || engine.discovered_from == "cloakbrowser_stealth"
        || engine
            .executable
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("cloakbrowser")
}

pub fn fingerprint_seed(seed_input: &str) -> u32 {
    let digest = util::hash(seed_input.as_bytes());
    let prefix = u32::from_str_radix(&digest[..8], 16).unwrap_or(42_000);
    10_000 + (prefix % 90_000)
}

pub fn can_run_headed() -> bool {
    if std::env::var_os("HYCLI_DISABLE_HEADED").is_some() {
        return false;
    }
    if std::env::var_os("HYCLI_HEADED").is_some() {
        return true;
    }
    #[cfg(any(target_os = "macos", windows))]
    {
        true
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
    }
}

pub fn build_stealth_browser_config(
    engine: &crate::environment::BrowserEngine,
    profile_dir: Option<&std::path::Path>,
    seed_input: &str,
    headed: bool,
) -> Result<BrowserConfig, String> {
    let mut builder = BrowserConfig::builder()
        .chrome_executable(&engine.executable)
        .disable_default_args()
        .no_sandbox()
        .viewport(None)
        .env("CLOAKBROWSER_SUPPRESS_FONT_WARNING", "1");
    if let Some(dir) = profile_dir {
        builder = builder.user_data_dir(dir);
    }
    if headed {
        builder = builder
            .with_head()
            .arg("ignore-gpu-blocklist")
            .arg(("window-position", "-32000,-32000"))
            .window_size(1920, 1080);
    } else {
        builder = builder.new_headless_mode().window_size(1920, 947);
        #[cfg(windows)]
        {
            builder = builder.arg("ignore-gpu-blocklist").arg("disable-gpu");
        }
    }
    // Pass clean Chromium argument keys without leading `--` because chromiumoxide's
    // ArgsBuilder formats keys with `format!("--{}", key)`.
    builder = builder
        .arg("no-first-run")
        .arg("no-default-browser-check")
        .arg("disable-background-networking")
        .arg("disable-background-timer-throttling")
        .arg("disable-backgrounding-occluded-windows")
        .arg("disable-breakpad")
        .arg("disable-client-side-phishing-detection")
        .arg("disable-component-extensions-with-background-pages")
        .arg("disable-default-apps")
        .arg("disable-dev-shm-usage")
        .arg("disable-hang-monitor")
        .arg("disable-ipc-flooding-protection")
        .arg("disable-popup-blocking")
        .arg("disable-prompt-on-repost")
        .arg("disable-renderer-backgrounding")
        .arg("disable-sync")
        .arg(("password-store", "basic"))
        .arg("use-mock-keychain")
        .arg((
            "disable-features",
            "OptimizationHints,MediaRouter,DialMediaRouteProvider,Translate",
        ))
        .arg(("disable-blink-features", "AutomationControlled"));

    if is_cloakbrowser_engine(engine) {
        let seed = fingerprint_seed(seed_input).to_string();
        let platform = if cfg!(target_os = "macos") {
            "macos"
        } else {
            "windows"
        };
        builder = builder
            .arg(("fingerprint", seed.as_str()))
            .arg(("fingerprint-platform", platform));
    }
    builder.build()
}

fn cookie_param_for(cookie: &accounts::Cookie, fallback_origin: &str) -> network::CookieParam {
    let mut value = network::CookieParam::new(&cookie.name, &cookie.value);
    let scheme = if fallback_origin.starts_with("http://") && !cookie.secure {
        "http"
    } else {
        "https"
    };
    let clean_domain = cookie.domain.trim_start_matches('.');
    let path = if cookie.path.starts_with('/') {
        cookie.path.as_str()
    } else {
        "/"
    };
    if !clean_domain.is_empty() {
        value.url = Some(format!("{scheme}://{clean_domain}{path}"));
    } else {
        value.url = Some(format!("{fallback_origin}/"));
    }
    if !cookie.host_only && !clean_domain.is_empty() {
        value.domain = Some(format!(".{clean_domain}"));
    }
    value.path = Some(path.to_string());
    value.secure = Some(cookie.secure);
    value.http_only = Some(cookie.http_only);
    value.same_site = match cookie.same_site.as_str() {
        "strict" => Some(network::CookieSameSite::Strict),
        "lax" => Some(network::CookieSameSite::Lax),
        "no_restriction" => Some(network::CookieSameSite::None),
        _ => None,
    };
    value
}

fn capture_needs_headed_escalation(capture: &Capture) -> bool {
    let (cls, _) = crate::defend::classify(&crate::defend::Observation {
        status_code: capture.status,
        headers: Vec::new(),
        body_snippet: capture.html.chars().take(4096).collect(),
    });
    if matches!(
        cls,
        crate::defend::Class::Cloudflare
            | crate::defend::Class::BotManager
            | crate::defend::Class::Captcha
    ) {
        return true;
    }
    if matches!(capture.status, 403 | 429 | 503) {
        let lower = capture.html.to_ascii_lowercase();
        if lower.contains("access denied")
            || lower.contains("edgesuite")
            || lower.contains("akamai")
            || lower.contains("just a moment")
            || lower.contains("cf-chl")
        {
            return true;
        }
    }
    false
}

pub struct InPageFetchRequest<'a> {
    pub credential: &'a Credential,
    pub method: &'a str,
    pub url: &'a str,
    pub headers: &'a [(String, String)],
    pub body: Option<&'a [u8]>,
    pub allow_local: bool,
}

pub struct InPageFetchResponse {
    pub url: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub bytes: Vec<u8>,
}

#[derive(Default)]
pub struct Capture {
    pub identity: Option<Identity>,
    pub identity_url: String,
    pub headers: BTreeMap<String, String>,
    pub observations: Vec<Observation>,
    pub html: String,
    pub final_url: String,
    pub status: u16,
    pub authenticated_requests: usize,
}
fn unavailable<T>(_: T) -> AppError {
    AppError::api("browser_unavailable", 422)
}

pub fn navigation_safe(url: &url::Url) -> bool {
    crate::policy::safe_read_url(url)
        || (url.query().is_none()
            && matches!(
                url.path().trim_end_matches('/').rsplit('/').next(),
                Some("login" | "signin" | "log-in" | "sign-in")
            ))
}

fn discover_live_cdp_endpoint(credential: &Credential) -> Option<String> {
    for env_key in ["HYCLI_CDP", "CHROME_CDP"] {
        if let Ok(val) = std::env::var(env_key) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    if !credential.source_profile.is_empty() {
        let profile_path = std::path::Path::new(&credential.source_profile);
        let mut candidates = vec![profile_path.join("DevToolsActivePort")];
        if let Some(parent) = profile_path.parent() {
            candidates.push(parent.join("DevToolsActivePort"));
        }
        for port_file in candidates {
            if let Ok(bytes) = util::read_bounded(&port_file, 4096) {
                if let Ok(text) = String::from_utf8(bytes) {
                    let mut lines = text.lines();
                    if let (Some(port_str), Some(ws_path)) = (lines.next(), lines.next()) {
                        if let Ok(port) = port_str.trim().parse::<u16>() {
                            let ws = ws_path.trim();
                            if ws.starts_with('/') {
                                return Some(format!("ws://127.0.0.1:{port}{ws}"));
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn acquire_profile_dir(
    origin: &str,
) -> Result<
    (
        std::path::PathBuf,
        Option<std::fs::File>,
        Option<crate::browser_storage::Snapshot>,
    ),
    (),
> {
    use sha2::{Digest, Sha256};
    let base = std::env::var_os("HYCLI_TEST_DATA")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            directories::ProjectDirs::from("", "", "hycli").map(|d| d.cache_dir().to_path_buf())
        })
        .or_else(|| Some(crate::platform::data_dir()));
    if let Some(root) = base {
        let mut hasher = Sha256::new();
        hasher.update(origin.as_bytes());
        let key: String = hasher
            .finalize()
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect();
        let warm_dir = root.join("browser-profiles").join(key);
        if util::private_dir(&warm_dir).is_ok() {
            let lock_path = warm_dir.join(".hycli-profile.lock");
            if let Ok(lock) = util::file_lock(&lock_path, true) {
                let singleton = warm_dir.join("SingletonLock");
                let live_owner = std::fs::read_link(&singleton)
                    .ok()
                    .and_then(|target| {
                        target
                            .to_str()
                            .and_then(|s| s.rsplit_once('-'))
                            .and_then(|(_, p)| p.parse::<u32>().ok())
                    })
                    .is_some_and(util::is_pid_alive);
                if !live_owner {
                    for name in ["SingletonLock", "SingletonSocket", "SingletonCookie"] {
                        let _ = std::fs::remove_file(warm_dir.join(name));
                    }
                    return Ok((warm_dir, Some(lock), None));
                }
            }
        }
    }
    let snapshot = crate::browser_storage::Snapshot::new()?;
    let path = snapshot.0.clone();
    Ok((path, None, Some(snapshot)))
}

pub async fn capture(
    credential: &Credential,
    entry: &str,
    allow_local: bool,
) -> AppResult<Capture> {
    if let Some(cdp_url) = discover_live_cdp_endpoint(credential) {
        if let Ok(Ok((browser, mut handler))) =
            tokio::time::timeout(Duration::from_secs(3), Browser::connect(cdp_url)).await
        {
            let url = crate::net::validate_url(entry)?;
            let cred_origin = crate::net::validate_url(&credential.origin)?;
            if !crate::net::same_site_origin(&url, &cred_origin) || !navigation_safe(&url) {
                return Err(AppError::api("unsafe_target", 400));
            }
            crate::net::resolve(&url, allow_local).await?;
            let drive = tokio::spawn(async move { while handler.next().await.is_some() {} });
            let result = tokio::time::timeout(
                Duration::from_secs(35),
                observe(&browser, credential, entry, allow_local),
            )
            .await
            .map_err(unavailable)
            .and_then(|r| r);
            drive.abort();
            if result.is_ok() {
                return result;
            }
        }
    }
    let environment = tokio::task::spawn_blocking(crate::environment::inspect)
        .await
        .map_err(unavailable)?;
    let engine = environment
        .browsers
        .iter()
        .find(|b| b.protocol == "chromium-cdp")
        .ok_or_else(|| AppError::api("browser_unavailable", 422))?;
    capture_with_engine(credential, entry, allow_local, engine).await
}

async fn run_capture_with_mode(
    credential: &Credential,
    entry: &str,
    allow_local: bool,
    engine: &crate::environment::BrowserEngine,
    headed: bool,
) -> AppResult<Capture> {
    let (profile_dir, _profile_lock, _snapshot) =
        acquire_profile_dir(&credential.origin).map_err(unavailable)?;
    let config =
        build_stealth_browser_config(engine, Some(&profile_dir), &credential.origin, headed)
            .map_err(unavailable)?;
    let (mut browser, mut handler) =
        tokio::time::timeout(Duration::from_secs(35), Browser::launch(config))
            .await
            .map_err(unavailable)?
            .map_err(unavailable)?;
    let drive = tokio::spawn(async move { while handler.next().await.is_some() {} });
    let result = tokio::time::timeout(
        Duration::from_secs(35),
        observe(&browser, credential, entry, allow_local),
    )
    .await
    .map_err(unavailable)
    .and_then(|r| r);
    let _ = browser.close().await;
    drive.abort();
    result
}

pub async fn capture_with_engine(
    credential: &Credential,
    entry: &str,
    allow_local: bool,
    engine: &crate::environment::BrowserEngine,
) -> AppResult<Capture> {
    let url = crate::net::validate_url(entry)?;
    let cred_origin = crate::net::validate_url(&credential.origin)?;
    if !crate::net::same_site_origin(&url, &cred_origin) || !navigation_safe(&url) {
        return Err(AppError::api("unsafe_target", 400));
    }
    crate::net::resolve(&url, allow_local).await?;
    if engine.protocol != "chromium-cdp" {
        return Err(AppError::api("browser_unavailable", 422));
    }
    let force_headed = std::env::var_os("HYCLI_HEADED").is_some();
    let first = run_capture_with_mode(credential, entry, allow_local, engine, force_headed).await?;
    if !force_headed && capture_needs_headed_escalation(&first) && can_run_headed() {
        if let Ok(escalated) =
            run_capture_with_mode(credential, entry, allow_local, engine, true).await
        {
            if !capture_needs_headed_escalation(&escalated)
                || escalated.observations.len() > first.observations.len()
            {
                return Ok(escalated);
            }
        }
    }
    Ok(first)
}

async fn run_fetch_in_page_with_mode(
    req: &InPageFetchRequest<'_>,
    origin_str: &str,
    engine: &crate::environment::BrowserEngine,
    headed: bool,
) -> AppResult<InPageFetchResponse> {
    let (profile_dir, _profile_lock, _snapshot) =
        acquire_profile_dir(origin_str).map_err(unavailable)?;
    let config = build_stealth_browser_config(engine, Some(&profile_dir), origin_str, headed)
        .map_err(unavailable)?;
    let (mut browser, mut handler) =
        tokio::time::timeout(Duration::from_secs(30), Browser::launch(config))
            .await
            .map_err(unavailable)?
            .map_err(unavailable)?;
    let drive = tokio::spawn(async move { while handler.next().await.is_some() {} });
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        execute_in_page_fetch(&browser, req, origin_str),
    )
    .await
    .map_err(unavailable)
    .and_then(|r| r);
    let _ = browser.close().await;
    drive.abort();
    result
}

pub async fn fetch_in_page(req: InPageFetchRequest<'_>) -> AppResult<InPageFetchResponse> {
    let target = crate::net::validate_url(req.url)?;
    crate::net::resolve(&target, req.allow_local).await?;
    let origin_str = if req.credential.origin.is_empty() {
        target.origin().ascii_serialization()
    } else {
        let cred_origin = crate::net::validate_url(&req.credential.origin)?;
        if !crate::net::same_site_origin(&target, &cred_origin) {
            return Err(AppError::api("unsafe_target", 400));
        }
        cred_origin.origin().ascii_serialization()
    };

    if let Some(cdp_url) = discover_live_cdp_endpoint(req.credential) {
        if let Ok(Ok((browser, mut handler))) =
            tokio::time::timeout(Duration::from_secs(3), Browser::connect(cdp_url)).await
        {
            let drive = tokio::spawn(async move { while handler.next().await.is_some() {} });
            let res = tokio::time::timeout(
                Duration::from_secs(30),
                execute_in_page_fetch(&browser, &req, &origin_str),
            )
            .await
            .map_err(unavailable)
            .and_then(|r| r);
            drive.abort();
            if res.is_ok() {
                return res;
            }
        }
    }

    let environment = tokio::task::spawn_blocking(crate::environment::inspect)
        .await
        .map_err(unavailable)?;
    let engine = environment
        .browsers
        .iter()
        .find(|b| b.protocol == "chromium-cdp")
        .ok_or_else(|| AppError::api("browser_unavailable", 422))?;
    let force_headed = std::env::var_os("HYCLI_HEADED").is_some();
    let first = run_fetch_in_page_with_mode(&req, &origin_str, engine, force_headed).await?;
    let (cls, _) = crate::defend::classify(&crate::defend::Observation {
        status_code: first.status,
        headers: first.headers.clone(),
        body_snippet: String::from_utf8_lossy(&first.bytes)
            .chars()
            .take(4096)
            .collect(),
    });
    if !force_headed
        && matches!(
            cls,
            crate::defend::Class::Cloudflare
                | crate::defend::Class::BotManager
                | crate::defend::Class::Captcha
        )
        && can_run_headed()
    {
        if let Ok(escalated) = run_fetch_in_page_with_mode(&req, &origin_str, engine, true).await {
            return Ok(escalated);
        }
    }
    Ok(first)
}

async fn execute_in_page_fetch(
    browser: &Browser,
    req: &InPageFetchRequest<'_>,
    origin_str: &str,
) -> AppResult<InPageFetchResponse> {
    let page = browser.new_page("about:blank").await.map_err(unavailable)?;
    for cookie in &req.credential.cookies {
        let value = cookie_param_for(cookie, origin_str);
        page.set_cookie(value).await.map_err(unavailable)?;
    }
    let origin_json = serde_json::to_string(origin_str)?;
    let storage_json = serde_json::to_string(&req.credential.local_storage)?;
    page.evaluate_on_new_document(format!(
        "{STEALTH_INIT_SCRIPT}\nif(location.origin==={origin_json}){{for(const [key,value] of Object.entries({storage_json})){{try{{localStorage.setItem(key,value);sessionStorage.setItem(key,value)}}catch(_){{}}}}}}"
    ))
    .await
    .map_err(unavailable)?;

    let bootstrap_url = format!("{origin_str}/");
    let _ = tokio::time::timeout(Duration::from_secs(12), page.goto(&bootstrap_url)).await;
    for _ in 0..10 {
        if let Ok(dom) = page.content().await {
            let lower = dom.to_ascii_lowercase();
            if !lower.contains("just a moment")
                && !lower.contains("cf-chl")
                && !lower.contains("challenge-platform")
            {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    use base64::Engine;
    let body_b64 = req
        .body
        .map(|b| base64::engine::general_purpose::STANDARD.encode(b));
    let cfg = serde_json::json!({
        "url": req.url,
        "method": req.method.to_ascii_uppercase(),
        "headers": req.headers,
        "bodyBase64": body_b64,
    });
    let script = format!(
        r#"(async () => {{
  const cfg = {cfg};
  const headers = {{}};
  for (const [k, v] of cfg.headers) {{
    const lower = String(k).toLowerCase();
    if (!['cookie', 'cookie2', 'host', 'origin', 'referer', 'user-agent', 'content-length', 'transfer-encoding', 'connection'].includes(lower) && !lower.startsWith('sec-') && !lower.startsWith('proxy-')) {{
      headers[k] = v;
    }}
  }}
  const init = {{
    method: cfg.method,
    headers,
    credentials: 'include',
    mode: 'cors',
    redirect: 'follow'
  }};
  if (cfg.bodyBase64) {{
    const bin = atob(cfg.bodyBase64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    init.body = bytes;
  }}
  const resp = await fetch(cfg.url, init);
  const buf = await resp.arrayBuffer();
  if (buf.byteLength > 8 * 1024 * 1024) {{
    throw new Error('too_large');
  }}
  const bytes = new Uint8Array(buf);
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {{
    binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
  }}
  const outHeaders = [];
  resp.headers.forEach((v, k) => outHeaders.push([String(k).toLowerCase(), String(v)]));
  return {{
    status: resp.status,
    url: resp.url || cfg.url,
    headers: outHeaders,
    bodyBase64: btoa(binary)
  }};
}})()"#
    );

    let isolated_ctx = if let Ok(Some(frame_id)) = page.mainframe().await {
        page.execute(
            chromiumoxide::cdp::browser_protocol::page::CreateIsolatedWorldParams::builder()
                .frame_id(frame_id)
                .world_name("hycli_isolated_fetch")
                .grant_univeral_access(true)
                .build()
                .map_err(unavailable)?,
        )
        .await
        .ok()
        .map(|r| r.result.execution_context_id)
    } else {
        None
    };

    let mut eval_builder = chromiumoxide::cdp::js_protocol::runtime::EvaluateParams::builder()
        .expression(script)
        .await_promise(true)
        .return_by_value(true);
    if let Some(ctx_id) = isolated_ctx {
        eval_builder = eval_builder.context_id(ctx_id);
    }
    let eval_params = eval_builder.build().map_err(unavailable)?;
    let eval_res = page.evaluate_expression(eval_params).await;
    let _ = page.close().await;
    let val: Value = eval_res
        .map_err(unavailable)?
        .into_value()
        .map_err(unavailable)?;
    let status = val["status"].as_u64().unwrap_or(0) as u16;
    let final_url = val["url"].as_str().unwrap_or(req.url).to_string();
    let headers: Vec<(String, String)> = val["headers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|pair| {
            let arr = pair.as_array()?;
            Some((
                arr.first()?.as_str()?.to_string(),
                arr.get(1)?.as_str()?.to_string(),
            ))
        })
        .collect();
    let b64 = val["bodyBase64"].as_str().unwrap_or("");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(unavailable)?;
    Ok(InPageFetchResponse {
        url: final_url,
        status,
        headers,
        bytes,
    })
}

fn is_safe_graphql_read_post(url: &url::Url, post_data: Option<&str>) -> bool {
    let lower = url.path().to_ascii_lowercase();
    if !lower.contains("graphql") && !lower.ends_with("/gql") {
        return false;
    }
    let Some(raw) = post_data else {
        return false;
    };
    let Ok(val) = serde_json::from_str::<Value>(raw) else {
        return false;
    };
    let Some(query) = val.get("query").and_then(Value::as_str) else {
        return false;
    };
    use graphql_parser::query::{Definition, OperationDefinition};
    let Ok(doc) = graphql_parser::parse_query::<&str>(query) else {
        return false;
    };
    let mut has_query = false;
    for def in &doc.definitions {
        if let Definition::Operation(op) = def {
            match op {
                OperationDefinition::Mutation(_) | OperationDefinition::Subscription(_) => {
                    return false;
                }
                OperationDefinition::Query(_) | OperationDefinition::SelectionSet(_) => {
                    has_query = true;
                }
            }
        }
    }
    has_query
}

fn extract_post_body_schema(post_data: Option<&str>) -> BTreeMap<String, String> {
    let mut schema = BTreeMap::new();
    let Some(raw) = post_data else {
        return schema;
    };
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(raw) {
        for (k, v) in map.into_iter().take(40) {
            if !util::secret_field(&k) {
                let kind = match v {
                    Value::Number(_) => "number",
                    Value::Bool(_) => "bool",
                    Value::Array(_) => "array",
                    Value::Object(_) => "object",
                    _ => "string",
                };
                schema.insert(k, kind.into());
            }
        }
    } else {
        for (k, _) in url::form_urlencoded::parse(raw.as_bytes()).take(40) {
            if !util::secret_field(&k) {
                schema.insert(k.into_owned(), "string".into());
            }
        }
    }
    schema
}

async fn observe(
    browser: &Browser,
    credential: &Credential,
    entry: &str,
    allow_local: bool,
) -> AppResult<Capture> {
    let page = browser.new_page("about:blank").await.map_err(unavailable)?;
    for cookie in &credential.cookies {
        let value = cookie_param_for(cookie, &credential.origin);
        page.set_cookie(value).await.map_err(unavailable)?;
    }
    let origin = serde_json::to_string(&credential.origin)?;
    let storage = serde_json::to_string(&credential.local_storage)?;
    page.evaluate_on_new_document(format!(
        "{STEALTH_INIT_SCRIPT}\nif(location.origin==={origin}){{for(const [key,value] of Object.entries({storage})){{try{{localStorage.setItem(key,value);sessionStorage.setItem(key,value)}}catch(_){{}}}}}}"
    ))
    .await
    .map_err(unavailable)?;
    page.execute(network::EnableParams::default())
        .await
        .map_err(unavailable)?;
    let mut paused = page
        .event_listener::<fetch::EventRequestPaused>()
        .await
        .map_err(unavailable)?;
    let mut responses = page
        .event_listener::<network::EventResponseReceived>()
        .await
        .map_err(unavailable)?;
    let mut finished = page
        .event_listener::<network::EventLoadingFinished>()
        .await
        .map_err(unavailable)?;
    page.execute(fetch::EnableParams::default())
        .await
        .map_err(unavailable)?;
    let result = Arc::new(Mutex::new(Capture::default()));
    let captured = result.clone();
    let task_page = page.clone();
    let target_origin = credential.origin.clone();
    let target_url = crate::net::validate_url(&target_origin)?;
    let listener_target_url = target_url.clone();
    let listener = tokio::spawn(async move {
        let mut requests = std::collections::HashMap::new();
        let mut identities = std::collections::HashMap::new();
        let mut obs_bodies: std::collections::HashMap<network::RequestId, (String, String)> =
            std::collections::HashMap::new();
        let mut checked_origins = BTreeMap::new();
        loop {
            tokio::select! {
                Some(event) = paused.next() => {
                    let parsed = crate::net::validate_url(&event.request.url).ok();
                    let same = parsed.as_ref().is_some_and(|u| crate::net::same_site_origin(u, &listener_target_url));
                    let method = event.request.method.to_ascii_uppercase();
                    let post_data = event
                        .request
                        .post_data_entries
                        .as_ref()
                        .and_then(|entries| entries.first())
                        .and_then(|entry| entry.bytes.as_ref())
                        .and_then(|b| {
                            let slice: &[u8] = b.as_ref();
                            String::from_utf8(slice.to_vec()).ok()
                        });
                    let is_safe_method = ["GET", "HEAD", "OPTIONS"].contains(&method.as_str())
                        || (method == "POST"
                            && parsed
                                .as_ref()
                                .is_some_and(|u| is_safe_graphql_read_post(u, post_data.as_deref())));
                    let mut permitted = parsed.as_ref().is_some_and(|url| {
                        crate::policy::safe_read_url(url)
                            || (event.resource_type == network::ResourceType::Document
                                && same
                                && navigation_safe(url))
                    }) && is_safe_method;
                    if let Some(url) = &parsed {
                        let origin = url.origin().ascii_serialization();
                        let public = if let Some(public) = checked_origins.get(&origin) { *public } else {
                            let public = crate::net::resolve(url, allow_local).await.is_ok();
                            checked_origins.insert(origin, public);
                            public
                        };
                        permitted &= public;
                    }
                    let values = event.request.headers.inner().as_object();
                    let auth_header_names: Vec<String> = values
                        .into_iter()
                        .flatten()
                        .filter(|(k, _)| accounts::browser_header(k))
                        .map(|(k, _)| k.to_ascii_lowercase())
                        .collect();
                    if !permitted {
                        // Intercept-and-Abort: record write/mutation schema before blocking network send.
                        if same
                            && ["POST", "PUT", "PATCH", "DELETE"].contains(&method.as_str())
                        {
                            if let Some(url) = &parsed {
                                if let Ok(mut capture) = captured.lock() {
                                    if capture.observations.len() < 80 {
                                        capture.observations.push(Observation {
                                            method,
                                            path: url.path().into(),
                                            query: url
                                                .query_pairs()
                                                .filter(|(k, _)| !util::secret_field(k))
                                                .map(|(k, _)| (k.into_owned(), "string".into()))
                                                .collect(),
                                            body: extract_post_body_schema(post_data.as_deref()),
                                            status: 0,
                                            observed_at: util::now(),
                                            response_pointers: Vec::new(),
                                            auth_headers: auth_header_names,
                                        });
                                    }
                                }
                            }
                        }
                        let _ = task_page.execute(fetch::FailRequestParams::new(event.request_id.clone(), network::ErrorReason::BlockedByClient)).await;
                        continue;
                    }
                    let mut command = fetch::ContinueRequestParams::new(event.request_id.clone());
                    let values = event.request.headers.inner().as_object();
                    if same && (method == "GET" || method == "POST") {
                        if let Some(id) = &event.network_id {
                            let headers: BTreeMap<String, String> = values.into_iter().flatten()
                                .filter(|(k, _)| accounts::browser_header(k))
                                .filter_map(|(k,v)| v.as_str().map(|v| (k.to_ascii_lowercase(), v.into())))
                                .collect();
                            if !headers.is_empty() {
                                if let Ok(mut capture) = captured.lock() {
                                    capture.authenticated_requests += 1;
                                    for (k, v) in &headers {
                                        capture.headers.entry(k.clone()).or_insert_with(|| v.clone());
                                    }
                                }
                            }
                            let body_schema = extract_post_body_schema(post_data.as_deref());
                            requests.insert(id.clone(), (parsed.unwrap(), headers, method, body_schema));
                        }
                    } else if !same {
                        // Cross-origin resource requests must never carry the restored session.
                        command.headers = Some(values.into_iter().flatten()
                            .filter(|(k, _)| !accounts::browser_header(k) && !k.eq_ignore_ascii_case("cookie"))
                            .filter_map(|(k,v)| v.as_str().map(|v| fetch::HeaderEntry::new(k, v))).collect());
                    }
                    let _ = task_page.execute(command).await;
                }
                Some(event) = responses.next() => {
                    if let Some((url, headers, req_method, body_schema)) = requests.remove(&event.request_id) {
                        let status = event.response.status as u16;
                        if event.r#type == network::ResourceType::Document {
                            if let Ok(mut capture) = captured.lock() { capture.status = status; }
                        }
                        if (200..300).contains(&status) && matches!(event.r#type, network::ResourceType::Xhr | network::ResourceType::Fetch) {
                            let path_str: String = url.path().into();
                            let auth_headers: Vec<String> = headers.keys().cloned().collect();
                            if let Ok(mut capture) = captured.lock() {
                                if capture.observations.len() < 80 {
                                    capture.observations.push(Observation {
                                        method: req_method.clone(),
                                        path: path_str.clone(),
                                        query: url.query_pairs().filter(|(k, _)| !util::secret_field(k)).map(|(k, _)| (k.into_owned(), "string".into())).collect(),
                                        body: body_schema,
                                        status,
                                        observed_at: util::now(),
                                        response_pointers: Vec::new(),
                                        auth_headers,
                                    });
                                }
                            }
                            if obs_bodies.len() < 32 {
                                obs_bodies.insert(event.request_id.clone(), (req_method, path_str));
                            }
                            if accounts::identity_endpoint(&url) { identities.insert(event.request_id.clone(), (url, headers)); }
                        }
                    }
                }
                Some(event) = finished.next() => {
                    let is_id = identities.remove(&event.request_id);
                    let obs_target = obs_bodies.remove(&event.request_id);
                    if is_id.is_some() || obs_target.is_some() {
                        if let Ok(value) = identity_body(&task_page, event.request_id.clone()).await {
                            if let Some((url, headers)) = is_id {
                                if let Some(identity) = accounts::extract_identity(&value) {
                                    if let Ok(mut capture) = captured.lock() {
                                        capture.identity = Some(identity);
                                        capture.identity_url = url.to_string();
                                        for (k, v) in headers {
                                            capture.headers.insert(k, v);
                                        }
                                    }
                                }
                            }
                            if let Some((req_method, path_str)) = obs_target {
                                let pointers = extract_json_pointers(&value);
                                if !pointers.is_empty() {
                                    if let Ok(mut capture) = captured.lock() {
                                        if let Some(obs) = capture
                                            .observations
                                            .iter_mut()
                                            .rev()
                                            .find(|o| o.method == req_method && o.path == path_str)
                                        {
                                            obs.response_pointers = pointers;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                else => break,
            }
        }
    });
    // Navigation only: never click a control or submit a form to discover a session.
    let _ = tokio::time::timeout(Duration::from_secs(18), page.goto(entry)).await;
    let anonymous = credential.cookies.is_empty() && credential.local_storage.is_empty();
    for tick in 0..24 {
        if tick == 2 {
            // Trigger lazy-loaded list items and scroll-activated XHR/fetch endpoints.
            let _ = page
                .evaluate("try { window.scrollBy(0, 2000); } catch (_) {}")
                .await;
        }
        if result.lock().is_ok_and(|c| c.identity.is_some()) {
            break;
        }
        if anonymous && tick >= 3 && result.lock().is_ok_and(|c| (200..400).contains(&c.status)) {
            if let Ok(dom) = page.content().await {
                let lower = dom.to_ascii_lowercase();
                if dom.len() > 256
                    && !lower.contains("just a moment")
                    && !lower.contains("cf-chl")
                    && !lower.contains("challenge-platform")
                {
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    listener.abort();
    let _ = listener.await;
    if let Ok(eval) = page
        .evaluate("() => Array.isArray(window.__hycli_apis) ? window.__hycli_apis : []")
        .await
    {
        if let Ok(Value::Array(items)) = eval.into_value::<Value>() {
            if let Ok(mut capture) = result.lock() {
                for item in items.into_iter().take(40) {
                    let Some(raw_url) = item.get("url").and_then(Value::as_str) else {
                        continue;
                    };
                    let Ok(parsed) = target_url.join(raw_url) else {
                        continue;
                    };
                    if !crate::net::same_site_origin(&parsed, &target_url) {
                        continue;
                    }
                    let method = item
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or("GET")
                        .to_ascii_uppercase();
                    let status = item.get("status").and_then(Value::as_u64).unwrap_or(200) as u16;
                    let path_str = parsed.path().to_string();
                    let pointers = item
                        .get("data")
                        .map(extract_json_pointers)
                        .unwrap_or_default();
                    if capture.identity.is_none() && accounts::identity_endpoint(&parsed) {
                        if let Some(data) = item.get("data") {
                            if let Some(identity) = accounts::extract_identity(data) {
                                capture.identity = Some(identity);
                                capture.identity_url = parsed.to_string();
                            }
                        }
                    }
                    if let Some(existing) = capture
                        .observations
                        .iter_mut()
                        .find(|o| o.method == method && o.path == path_str)
                    {
                        if existing.response_pointers.is_empty() && !pointers.is_empty() {
                            existing.response_pointers = pointers;
                        }
                    } else if capture.observations.len() < 80 {
                        let auth_headers: Vec<String> = capture.headers.keys().cloned().collect();
                        capture.observations.push(Observation {
                            method,
                            path: path_str,
                            query: parsed
                                .query_pairs()
                                .filter(|(k, _)| !util::secret_field(k))
                                .map(|(k, _)| (k.into_owned(), "string".into()))
                                .collect(),
                            body: extract_post_body_schema(
                                item.get("body").and_then(Value::as_str),
                            ),
                            status,
                            observed_at: util::now(),
                            response_pointers: pointers,
                            auth_headers,
                        });
                    }
                }
            }
        }
    }
    let html = page.content().await.unwrap_or_default();
    let final_url = page
        .url()
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| entry.into());
    let _ = page.close().await;
    let mut result = result.lock().map_err(unavailable)?;
    result.html = html;
    result.final_url = final_url;
    Ok(std::mem::take(&mut *result))
}

fn extract_json_pointers(value: &Value) -> Vec<String> {
    let mut pointers = Vec::new();
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter().take(6) {
                if !util::secret_field(k) && !k.contains('/') && !k.contains('~') {
                    pointers.push(format!("/{k}"));
                    if let Value::Array(arr) = v {
                        if let Some(Value::Object(first)) = arr.first() {
                            for sub_k in first.keys().take(2) {
                                if !util::secret_field(sub_k)
                                    && !sub_k.contains('/')
                                    && !sub_k.contains('~')
                                {
                                    pointers.push(format!("/{k}/0/{sub_k}"));
                                }
                            }
                        }
                    }
                }
            }
        }
        Value::Array(arr) => {
            if let Some(Value::Object(first)) = arr.first() {
                for k in first.keys().take(4) {
                    if !util::secret_field(k) && !k.contains('/') && !k.contains('~') {
                        pointers.push(format!("/0/{k}"));
                    }
                }
            }
        }
        _ => {}
    }
    pointers.truncate(6);
    pointers
}

async fn identity_body(page: &Page, id: network::RequestId) -> AppResult<Value> {
    let body = page
        .execute(network::GetResponseBodyParams::new(id))
        .await
        .map_err(unavailable)?
        .result;
    if body.body.len() > 1024 * 1024 {
        return Err(AppError::api("too_large", 413));
    }
    let bytes = if body.base64_encoded {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(body.body)
            .map_err(unavailable)?
    } else {
        body.body.into_bytes()
    };
    serde_json::from_slice(&bytes).map_err(|_| AppError::api("data_invalid", 400))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn stealth_browser_config_activates_cloakbrowser_flags_without_double_dashes() {
        let engine = crate::environment::BrowserEngine {
            id: "browser-cloak".into(),
            name: "CloakBrowser".into(),
            protocol: "chromium-cdp".into(),
            discovered_from: "cloakbrowser_stealth".into(),
            executable: PathBuf::from("/opt/cloakbrowser/chromium-146.0.7680.177.5/chrome"),
        };
        assert!(is_cloakbrowser_engine(&engine));
        let seed = fingerprint_seed("https://discord.com");
        assert!((10_000..=99_999).contains(&seed));
        assert_eq!(seed, fingerprint_seed("https://discord.com"));

        let cfg = build_stealth_browser_config(&engine, None, "https://discord.com", false)
            .expect("config should build");
        let debug_str = format!("{cfg:?}");
        assert!(debug_str.contains("disable_default_args: true"));
        assert!(debug_str.contains("viewport: None"));
        assert!(debug_str.contains("sandbox: false"));
        assert!(debug_str.contains("New"));
        assert!(debug_str.contains("fingerprint"));
        assert!(debug_str.contains("fingerprint-platform"));
        assert!(!debug_str.contains("\"--"));
    }

    #[test]
    fn cookie_param_for_preserves_host_only_and_subdomain_scoping() {
        let host_only = accounts::Cookie {
            name: "__Host-user_session".into(),
            value: "tok1".into(),
            domain: "github.com".into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            host_only: true,
            expires: None,
            same_site: "lax".into(),
            store_id: String::new(),
            partition_key: None,
        };
        let param = cookie_param_for(&host_only, "https://github.com");
        assert_eq!(param.url.as_deref(), Some("https://github.com/"));
        assert!(param.domain.is_none());

        let domain_scoped = accounts::Cookie {
            name: "ad962bd2sb".into(),
            value: "oauth2_tok".into(),
            domain: ".upwork.com".into(),
            path: "/nx/find-work/".into(),
            secure: true,
            http_only: true,
            host_only: false,
            expires: None,
            same_site: "lax".into(),
            store_id: String::new(),
            partition_key: None,
        };
        let param2 = cookie_param_for(&domain_scoped, "https://www.upwork.com");
        assert_eq!(
            param2.url.as_deref(),
            Some("https://upwork.com/nx/find-work/")
        );
        assert_eq!(param2.domain.as_deref(), Some(".upwork.com"));
        assert_eq!(param2.path.as_deref(), Some("/nx/find-work/"));
    }
}
