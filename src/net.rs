//! Bounded first-party requests. Redirects never carry a credential to another origin.
use crate::{
    apperr::{AppError, AppResult},
    defend,
    policy::Policy,
    util,
};
use futures_util::StreamExt;
use reqwest::{
    Method,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde_json::Value;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

pub struct Network {
    pub policy: Arc<Policy>,
    pub allow_local: bool,
    pub clients: std::sync::Mutex<std::collections::BTreeMap<String, reqwest::Client>>,
}
#[derive(Clone)]
pub struct Request {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub account_id: String,
    pub limit: usize,
    pub timeout: Duration,
    pub read_only: bool,
    pub follow_redirects: bool,
    /// Compatibility metadata; requests always use the standard HTTP/TLS stack.
    pub tls_profile: String,
    /// multipart/form-data parts. When present, sent in place of `body`; the
    /// network layer reads any file parts from disk and sets the boundary.
    pub multipart: Option<Vec<crate::spec::MultipartPart>>,
}
impl Request {
    fn check_read_headers(&self) -> AppResult<()> {
        if self.read_only
            && self.multipart.as_ref().is_some_and(|parts| {
                parts
                    .iter()
                    .any(|part| matches!(part, crate::spec::MultipartPart::File { .. }))
            })
        {
            return Err(AppError::api("approval_required", 409));
        }
        if self.read_only
            && crate::policy::request_headers_effect(
                self.headers
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
            ) != crate::policy::Effect::Read
        {
            return Err(AppError::api("approval_required", 409));
        }
        Ok(())
    }
}
pub struct Response {
    pub url: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub bytes: Vec<u8>,
    pub duration_ms: i64,
    pub defense: String,
}
impl Response {
    pub fn header(&self, key: &str) -> &str {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    }
    pub fn cookie_value(&self, name: &str) -> Option<String> {
        let prefix = format!("{name}=");
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
            .find_map(|(_, v)| {
                v.split(';')
                    .next()
                    .and_then(|pair| pair.trim().strip_prefix(&prefix))
                    .map(str::to_owned)
            })
    }
    pub fn json(&self) -> Option<Value> {
        serde_json::from_slice(&self.bytes).ok()
    }
    pub fn successful(&self) -> bool {
        (200..300).contains(&self.status) && self.defense == "none"
    }
}
impl Network {
    pub fn new(policy: Arc<Policy>, allow_local: bool) -> Self {
        Self {
            policy,
            allow_local,
            clients: std::sync::Mutex::new(std::collections::BTreeMap::new()),
        }
    }

    fn pooled_client(
        &self,
        host: &str,
        ua: &str,
        addresses: &[SocketAddr],
        timeout: Duration,
    ) -> AppResult<reqwest::Client> {
        let mut addr_keys: Vec<String> = addresses.iter().map(ToString::to_string).collect();
        addr_keys.sort();
        let pool_key = format!(
            "{}|{}|{}|{}",
            host,
            ua,
            timeout.as_secs(),
            addr_keys.join(",")
        );
        if let Ok(guard) = self.clients.lock() {
            if let Some(existing) = guard.get(&pool_key) {
                return Ok(existing.clone());
            }
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(timeout.min(Duration::from_secs(90)))
            .connect_timeout(Duration::from_secs(10))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(6)
            .tcp_keepalive(Duration::from_secs(30))
            .user_agent(ua)
            .resolve_to_addrs(host, addresses)
            .build()
            .map_err(|_| AppError::api("internal", 500))?;
        if let Ok(mut guard) = self.clients.lock() {
            if guard.len() >= 64
                && let Some(first_key) = guard.keys().next().cloned()
            {
                guard.remove(&first_key);
            }
            guard.insert(pool_key, client.clone());
        }
        Ok(client)
    }

    /// Execute an in-page `window.fetch()` inside an isolated world on the origin's browser tab.
    pub async fn fetch_browser_in_page(
        &self,
        credential: &crate::accounts::Credential,
        input: &Request,
    ) -> AppResult<Response> {
        self.fetch_browser_in_page_guarded(credential, input, None)
            .await
    }

    pub(crate) async fn fetch_browser_in_page_guarded(
        &self,
        credential: &crate::accounts::Credential,
        input: &Request,
        check_authority: Option<&(dyn Fn() -> AppResult<()> + Sync)>,
    ) -> AppResult<Response> {
        input.check_read_headers()?;
        if input.multipart.is_some() {
            return Err(AppError::api("unsupported_transport", 400));
        }
        let initial = validate_url(&input.url)?;
        let origin_str = initial.origin().ascii_serialization();
        let strategy_key = Policy::strategy_key(
            &input.method,
            &input.url,
            "browser-fetch",
            &input.account_id,
        );
        let host_scope = format!("{origin_str}|*");
        let host_token = self.policy.acquire(&host_scope).await?;
        let mut host_lease = Lease {
            policy: &self.policy,
            scope: &host_scope,
            token: &host_token,
            released: false,
        };
        let scope = format!("{origin_str}|{}", input.account_id);
        if input.account_id.is_empty() {
            self.policy.resume_auth(&scope)?;
        }
        let token = self.policy.acquire(&scope).await?;
        let mut lease = Lease {
            policy: &self.policy,
            scope: &scope,
            token: &token,
            released: false,
        };
        if let Some(check) = check_authority {
            check()?;
        }
        let started = Instant::now();
        let fetched =
            crate::browser_session::fetch_in_page(crate::browser_session::InPageFetchRequest {
                credential,
                method: &input.method,
                url: &input.url,
                headers: &input.headers,
                body: input.body.as_deref(),
                allow_local: self.allow_local,
            })
            .await?;
        if !same_site_origin(&validate_url(&fetched.url)?, &initial)
            || fetched.bytes.len() > input.limit
        {
            return Err(AppError::api("response_mismatch", 502));
        }
        let (defense, _) = defend::classify(&defend::Observation {
            status_code: fetched.status,
            headers: fetched.headers.clone(),
            body_snippet: String::from_utf8_lossy(
                &fetched.bytes[..fetched.bytes.len().min(16_384)],
            )
            .into_owned(),
        });
        let challenged = defense != defend::Class::None && defense != defend::Class::RateLimit;
        if challenged {
            let _ = self
                .policy
                .record_blocked_strategy(&strategy_key, defense.as_str(), 180_000);
        } else if (200..300).contains(&fetched.status) {
            let _ = self.policy.clear_strategy(&strategy_key);
            let _ = self
                .policy
                .record_preferred_transport(&origin_str, "browser-fetch");
        }
        let retry_after = retry_after(&fetched.headers);
        self.policy
            .release(&scope, &token, fetched.status, retry_after, challenged)?;
        lease.released = true;
        self.policy.release(
            &host_scope,
            &host_token,
            if fetched.status == 429 { 429 } else { 0 },
            retry_after,
            false,
        )?;
        host_lease.released = true;
        Ok(Response {
            url: fetched.url,
            status: fetched.status,
            headers: fetched.headers,
            bytes: fetched.bytes,
            duration_ms: started.elapsed().as_millis() as i64,
            defense: defense.as_str().into(),
        })
    }

    /// Rendered reads share HTTP pacing and account gates with ordinary operations.
    pub async fn fetch_rendered(
        &self,
        credential: &crate::accounts::Credential,
        entry: &str,
        account: &str,
    ) -> AppResult<Response> {
        self.fetch_rendered_guarded(credential, entry, account, None)
            .await
    }

    pub(crate) async fn fetch_rendered_guarded(
        &self,
        credential: &crate::accounts::Credential,
        entry: &str,
        account: &str,
        check_authority: Option<&(dyn Fn() -> AppResult<()> + Sync)>,
    ) -> AppResult<Response> {
        let initial = validate_url(entry)?;
        let origin_str = initial.origin().ascii_serialization();
        let strategy_key = Policy::strategy_key("GET", entry, "browser", account);
        let host_scope = format!("{origin_str}|*");
        let host_token = self.policy.acquire(&host_scope).await?;
        let mut host_lease = Lease {
            policy: &self.policy,
            scope: &host_scope,
            token: &host_token,
            released: false,
        };
        let scope = format!("{origin_str}|{account}");
        if account.is_empty() {
            self.policy.resume_auth(&scope)?;
        }
        let token = self.policy.acquire(&scope).await?;
        let mut lease = Lease {
            policy: &self.policy,
            scope: &scope,
            token: &token,
            released: false,
        };
        if let Some(check) = check_authority {
            check()?;
        }
        let started = Instant::now();
        let capture = crate::browser_session::capture(credential, entry, self.allow_local).await?;
        if !same_site_origin(&validate_url(&capture.final_url)?, &initial)
            || capture.html.len() > 8 * 1024 * 1024
        {
            return Err(AppError::api("response_mismatch", 502));
        }
        let headers = vec![("content-type".into(), "text/html; charset=utf-8".into())];
        let (defense, _) = defend::classify(&defend::Observation {
            status_code: capture.status,
            headers: headers.clone(),
            body_snippet: capture.html.chars().take(16_384).collect(),
        });
        let challenged = defense != defend::Class::None && defense != defend::Class::RateLimit;
        if challenged {
            let _ = self
                .policy
                .record_blocked_strategy(&strategy_key, defense.as_str(), 180_000);
        } else if (200..300).contains(&capture.status) {
            let _ = self.policy.clear_strategy(&strategy_key);
        }
        self.policy
            .release(&scope, &token, capture.status, None, challenged)?;
        lease.released = true;
        self.policy.release(
            &host_scope,
            &host_token,
            if capture.status == 429 { 429 } else { 0 },
            None,
            false,
        )?;
        host_lease.released = true;
        Ok(Response {
            url: capture.final_url,
            status: capture.status,
            headers,
            bytes: capture.html.into_bytes(),
            duration_ms: started.elapsed().as_millis() as i64,
            defense: defense.as_str().into(),
        })
    }
    pub async fn fetch(&self, input: Request) -> AppResult<Response> {
        self.fetch_inner(input, None, None, None).await
    }
    /// The continuation key/value is supplied only by the private runtime resolver.
    pub(crate) async fn fetch_paginated(
        &self,
        mut input: Request,
        continuation: Option<(&str, &str)>,
    ) -> AppResult<Response> {
        if input.method != "GET" || !input.read_only {
            return Err(AppError::api("unsafe_target", 400));
        }
        input.follow_redirects = false;
        self.fetch_inner(input, continuation, None, None).await
    }
    /// Recheck private account authority after every wait and immediately before dispatch.
    /// The callback is also run for each bounded transport retry.
    pub(crate) async fn fetch_guarded(
        &self,
        input: Request,
        continuation: Option<(&str, &str)>,
        check_authority: &(dyn Fn() -> AppResult<()> + Sync),
    ) -> AppResult<Response> {
        self.fetch_inner(input, continuation, Some(check_authority), None)
            .await
    }
    pub(crate) async fn fetch_oauth_guarded(
        &self,
        mut input: Request,
        continuation: Option<(&str, &str)>,
        check_authority: &(dyn Fn() -> AppResult<()> + Sync),
        dispatched: Option<&std::sync::atomic::AtomicBool>,
    ) -> AppResult<Response> {
        input.follow_redirects = false;
        self.fetch_inner(input, continuation, Some(check_authority), dispatched)
            .await
    }
    async fn fetch_inner(
        &self,
        input: Request,
        continuation: Option<(&str, &str)>,
        check_authority: Option<&(dyn Fn() -> AppResult<()> + Sync)>,
        dispatched: Option<&std::sync::atomic::AtomicBool>,
    ) -> AppResult<Response> {
        input.check_read_headers()?;
        let mut initial = validate_url(&input.url)?;
        if let Some((key, value)) = continuation {
            if initial.query_pairs().any(|(name, _)| name == key) {
                return Err(AppError::api("data_invalid", 400));
            }
            initial.query_pairs_mut().append_pair(key, value);
            if initial.as_str().len() > 8192 {
                return Err(AppError::api("bad_url", 400));
            }
        }
        let mut target = initial.clone();
        let mut redirects = 0;
        let mut retries = 0;
        let start = Instant::now();
        loop {
            if let Some(check) = check_authority {
                check()?;
            }
            let addresses = resolve(&target, self.allow_local).await?;
            let host_scope = format!("{}|*", target.origin().ascii_serialization());
            let host_token = self.policy.acquire(&host_scope).await?;
            let mut host_lease = Lease {
                policy: &self.policy,
                scope: &host_scope,
                token: &host_token,
                released: false,
            };
            let scope = format!(
                "{}|{}",
                target.origin().ascii_serialization(),
                input.account_id
            );
            if input.account_id.is_empty() {
                // A protected public URL does not imply that anonymous access to the
                // entire website has lost a session. Also repair older persisted gates.
                self.policy.resume_auth(&scope)?;
            }
            let token = self.policy.acquire(&scope).await?;
            let mut lease = Lease {
                policy: &self.policy,
                scope: &scope,
                token: &token,
                released: false,
            };
            const CHROME_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36";
            let ua = if input.tls_profile == "hycli" {
                concat!("Hycli/", env!("CARGO_PKG_VERSION"))
            } else {
                CHROME_UA
            };
            let client = self.pooled_client(
                target.host_str().expect("validated host"),
                ua,
                &addresses,
                input.timeout,
            )?;
            let mut headers = HeaderMap::new();
            if matches!(
                input.tls_profile.as_str(),
                "go" | "chrome" | "browser" | "stealth" | "default" | "standard" | ""
            ) {
                let path_lower = target.path().to_ascii_lowercase();
                let is_mutation = !matches!(input.method.as_str(), "GET" | "HEAD" | "OPTIONS");
                let has_json_hint = input.body.is_some()
                    || path_lower.contains("/api/")
                    || path_lower.starts_with("/v1/")
                    || path_lower.starts_with("/v2/")
                    || path_lower.starts_with("/v3/")
                    || path_lower.contains("graphql")
                    || path_lower.ends_with(".json")
                    || input.headers.iter().any(|(k, _)| {
                        k.eq_ignore_ascii_case("content-type")
                            || k.eq_ignore_ascii_case("x-requested-with")
                            || k.eq_ignore_ascii_case("authorization")
                    });
                headers.insert(
                    HeaderName::from_static("accept"),
                    HeaderValue::from_static(if has_json_hint {
                        "application/json, text/plain, */*"
                    } else {
                        "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8"
                    }),
                );
                headers.insert(
                    HeaderName::from_static("accept-language"),
                    HeaderValue::from_static("en-US,en;q=0.9,ko;q=0.8"),
                );
                headers.insert(
                    HeaderName::from_static("sec-ch-ua"),
                    HeaderValue::from_static(
                        "\"Chromium\";v=\"134\", \"Not:A-Brand\";v=\"24\", \"Google Chrome\";v=\"134\"",
                    ),
                );
                headers.insert(
                    HeaderName::from_static("sec-ch-ua-mobile"),
                    HeaderValue::from_static("?0"),
                );
                headers.insert(
                    HeaderName::from_static("sec-ch-ua-platform"),
                    HeaderValue::from_static("\"Linux\""),
                );
                if has_json_hint || is_mutation {
                    headers.insert(
                        HeaderName::from_static("sec-fetch-site"),
                        HeaderValue::from_static("same-origin"),
                    );
                    headers.insert(
                        HeaderName::from_static("sec-fetch-mode"),
                        HeaderValue::from_static("cors"),
                    );
                    headers.insert(
                        HeaderName::from_static("sec-fetch-dest"),
                        HeaderValue::from_static("empty"),
                    );
                } else {
                    headers.insert(
                        HeaderName::from_static("sec-fetch-site"),
                        HeaderValue::from_static(if redirects > 0 {
                            "same-origin"
                        } else {
                            "none"
                        }),
                    );
                    headers.insert(
                        HeaderName::from_static("sec-fetch-mode"),
                        HeaderValue::from_static("navigate"),
                    );
                    headers.insert(
                        HeaderName::from_static("sec-fetch-dest"),
                        HeaderValue::from_static("document"),
                    );
                    headers.insert(
                        HeaderName::from_static("upgrade-insecure-requests"),
                        HeaderValue::from_static("1"),
                    );
                }
                let origin_str = target.origin().ascii_serialization();
                if (has_json_hint || is_mutation || !input.account_id.is_empty())
                    && let Ok(ref_val) = HeaderValue::from_str(&format!("{origin_str}/"))
                {
                    headers.insert(HeaderName::from_static("referer"), ref_val);
                }
                if is_mutation && let Ok(orig_val) = HeaderValue::from_str(&origin_str) {
                    headers.insert(HeaderName::from_static("origin"), orig_val);
                }
            }
            for (key, value) in &input.headers {
                let name = HeaderName::from_bytes(key.as_bytes())
                    .map_err(|_| AppError::api("data_invalid", 400))?;
                let mut value =
                    HeaderValue::from_str(value).map_err(|_| AppError::api("data_invalid", 400))?;
                if util::secret_field(key) {
                    value.set_sensitive(true);
                }
                headers.insert(name, value);
            }
            if input.multipart.is_some() {
                headers.remove(reqwest::header::CONTENT_TYPE);
            }
            let method = Method::from_bytes(input.method.as_bytes())
                .map_err(|_| AppError::api("data_invalid", 400))?;
            if let Some(check) = check_authority {
                check()?;
            }
            let mut request = client.request(method, target.clone()).headers(headers);
            if let Some(parts) = &input.multipart {
                request = request.multipart(build_multipart_form(parts).await?);
            } else if let Some(body) = &input.body {
                request = request.body(body.clone());
            }
            if let Some(check) = check_authority {
                check()?;
            }
            if let Some(dispatched) = dispatched {
                dispatched.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            let response = request.send().await;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    // This no-retry OAuth POST failed before an HTTP request could
                    // be sent. Other transport failures remain dispatch-uncertain.
                    if !input.read_only && !input.follow_redirects && error.is_connect() {
                        if let Some(dispatched) = dispatched {
                            dispatched.store(false, std::sync::atomic::Ordering::SeqCst);
                        }
                    }
                    let is_timeout = error.is_timeout();
                    drop(lease);
                    if input.read_only && retries < 1 && (is_timeout || error.is_connect()) {
                        retries += 1;
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                    return Err(AppError::api(
                        if is_timeout {
                            "timeout"
                        } else {
                            "request_failed"
                        },
                        502,
                    ));
                }
            };
            let status = response.status().as_u16();
            let gate_status = if status == 401 && input.account_id.is_empty() {
                0
            } else {
                status
            };
            let headers: Vec<(String, String)> = response
                .headers()
                .iter()
                .filter_map(|(key, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|v| (key.as_str().to_owned(), v.to_owned()))
                })
                .collect();
            let retry_after = retry_after(&headers);
            if response
                .content_length()
                .is_some_and(|length| length > input.limit as u64)
            {
                self.policy
                    .release(&scope, &token, gate_status, retry_after, false)?;
                lease.released = true;
                self.policy.release(
                    &host_scope,
                    &host_token,
                    if status == 429 { 429 } else { 0 },
                    retry_after,
                    false,
                )?;
                host_lease.released = true;
                return Err(AppError::api("too_large", 413));
            }
            let mut bytes = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| AppError::api("request_failed", 502))?;
                if bytes.len() + chunk.len() > input.limit {
                    self.policy
                        .release(&scope, &token, gate_status, retry_after, false)?;
                    lease.released = true;
                    self.policy.release(
                        &host_scope,
                        &host_token,
                        if status == 429 { 429 } else { 0 },
                        retry_after,
                        false,
                    )?;
                    host_lease.released = true;
                    return Err(AppError::api("too_large", 413));
                }
                bytes.extend_from_slice(&chunk);
            }
            let (defense, _) = defend::classify(&defend::Observation {
                status_code: status,
                headers: headers.clone(),
                body_snippet: String::from_utf8_lossy(&bytes[..bytes.len().min(16_384)])
                    .into_owned(),
            });
            let challenged = defense != defend::Class::None && defense != defend::Class::RateLimit;
            let strategy_key =
                Policy::strategy_key(&input.method, target.as_str(), "http", &input.account_id);
            if challenged {
                let _ =
                    self.policy
                        .record_blocked_strategy(&strategy_key, defense.as_str(), 300_000);
            } else if (200..300).contains(&status) {
                let _ = self.policy.clear_strategy(&strategy_key);
            }
            self.policy
                .release(&scope, &token, gate_status, retry_after, challenged)?;
            lease.released = true;
            self.policy.release(
                &host_scope,
                &host_token,
                if status == 429 { 429 } else { 0 },
                retry_after,
                false,
            )?;
            host_lease.released = true;
            if input.follow_redirects
                && input.read_only
                && matches!(status, 301 | 302 | 303 | 307 | 308)
            {
                let location = headers
                    .iter()
                    .find(|(key, _)| key == "location")
                    .map(|(_, value)| value);
                if let Some(location) = location {
                    let next = target
                        .join(location)
                        .map_err(|_| AppError::api("bad_url", 400))?;
                    validate_url(next.as_str())?;
                    if next.origin() == initial.origin()
                        && crate::policy::safe_read_url(&next)
                        && redirects < 4
                    {
                        target = next;
                        redirects += 1;
                        continue;
                    }
                }
            }
            // Automatic retries are bounded, read-only and never bypass a site's cooldown.
            if input.read_only
                && status == 503
                && defense == defend::Class::None
                && retries < 1
                && retry_after.is_none()
            {
                retries += 1;
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            return Ok(Response {
                url: if continuation.is_some() {
                    input.url.clone()
                } else {
                    target.to_string()
                },
                status,
                headers,
                bytes,
                duration_ms: start.elapsed().as_millis() as i64,
                defense: defense.as_str().into(),
            });
        }
    }
}
struct Lease<'a> {
    policy: &'a Policy,
    scope: &'a str,
    token: &'a str,
    released: bool,
}
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.policy.release(self.scope, self.token, 0, None, false);
        }
    }
}

/// Normalize a user-entered website address. Request and SiteSpec validation
/// still require an explicit scheme via `validate_url`.
pub fn website_url(raw: &str) -> AppResult<url::Url> {
    let raw = raw.trim();
    let address = if raw.starts_with("//") {
        format!("https:{raw}")
    } else if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("https://{raw}")
    };
    validate_url(&address)
}

pub fn validate_url(raw: &str) -> AppResult<url::Url> {
    if raw.len() > 8192 || raw.chars().any(char::is_control) {
        return Err(AppError::api("bad_url", 400));
    }
    let mut url = url::Url::parse(raw).map_err(|_| AppError::api("bad_url", 400))?;
    if !["http", "https"].contains(&url.scheme())
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::api("bad_url", 400));
    }
    if url.query_pairs().any(|(k, _)| util::secret_field(&k)) {
        return Err(AppError::api("bad_url", 400));
    }
    url.set_fragment(None);
    Ok(url)
}

pub fn registrable_domain(host: &str) -> Option<String> {
    let host = host.trim_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host == "localhost"
        || host.parse::<IpAddr>().is_ok()
        || host.starts_with('[')
    {
        return None;
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let tail2 = format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]);
    let multipart_tlds = [
        "co.kr", "or.kr", "ne.kr", "go.kr", "ac.kr", "co.jp", "ne.jp", "or.jp", "ac.jp", "go.jp",
        "co.uk", "org.uk", "ac.uk", "gov.uk", "com.au", "net.au", "org.au", "com.br", "com.cn",
        "com.tw", "com.hk", "com.sg",
    ];
    if parts.len() >= 3 && multipart_tlds.contains(&tail2.as_str()) {
        Some(format!(
            "{}.{}.{}",
            parts[parts.len() - 3],
            parts[parts.len() - 2],
            parts[parts.len() - 1]
        ))
    } else {
        Some(tail2)
    }
}

/// True when two URLs share the exact origin or belong to the same registrable domain (eTLD+1)
/// with the same scheme and port (e.g. `https://suno.com` and `https://studio-api-prod.suno.com`).
pub fn same_site_origin(a: &url::Url, b: &url::Url) -> bool {
    if a.origin() == b.origin() {
        return true;
    }
    if a.scheme() != b.scheme() || a.port_or_known_default() != b.port_or_known_default() {
        return false;
    }
    let (Some(ha), Some(hb)) = (a.host_str(), b.host_str()) else {
        return false;
    };
    match (registrable_domain(ha), registrable_domain(hb)) {
        (Some(da), Some(db)) => da == db,
        _ => false,
    }
}
pub async fn resolve(url: &url::Url, allow_local: bool) -> AppResult<Vec<SocketAddr>> {
    let host = url
        .host_str()
        .ok_or_else(|| AppError::api("bad_url", 400))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| AppError::api("bad_url", 400))?;
    let addresses: Vec<SocketAddr> = tokio::time::timeout(
        Duration::from_secs(8),
        tokio::net::lookup_host((host.trim_matches(['[', ']']), port)),
    )
    .await
    .map_err(|_| AppError::api("timeout", 502))?
    .map_err(|_| AppError::api("request_failed", 502))?
    .collect();
    if addresses.is_empty() || (!allow_local && addresses.iter().any(|a| !public_ip(a.ip()))) {
        return Err(AppError::api("unsafe_target", 400));
    }
    Ok(addresses)
}
fn public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_documentation()
                || o[0] == 0
                || o[0] >= 240
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
                || (o[0] == 192 && o[1] == 0 && o[2] == 0))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(v4));
            }
            let s = ip.segments();
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || s[0] & 0xe000 != 0x2000
                || (s[0] == 0x2001 && (s[1] == 0xdb8 || s[1] == 0 || s[1] == 2))
                || s[0] == 0x2002)
        }
    }
}
pub fn retry_after(headers: &[(String, String)]) -> Option<Duration> {
    let value = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("retry-after"))
        .map(|(_, value)| value)?;
    if let Ok(seconds) = value.trim().parse::<u64>() {
        return Some(Duration::from_secs(seconds.max(1).min(86_400)));
    }
    httpdate::parse_http_date(value).ok().map(|date| {
        date.duration_since(std::time::SystemTime::now())
            .unwrap_or(Duration::from_secs(1))
            .min(Duration::from_secs(86_400))
    })
}

/// Build a reqwest multipart form, reading any file parts from disk. File parts
/// share a 100 MiB budget and their media type is inferred from the extension.
async fn build_multipart_form(
    parts: &[crate::spec::MultipartPart],
) -> AppResult<reqwest::multipart::Form> {
    build_multipart_form_bounded(parts, 100 * 1024 * 1024).await
}

async fn build_multipart_form_bounded(
    parts: &[crate::spec::MultipartPart],
    mut remaining: usize,
) -> AppResult<reqwest::multipart::Form> {
    use crate::spec::MultipartPart;
    let mut form = reqwest::multipart::Form::new();
    for part in parts {
        match part {
            MultipartPart::Text { name, value } => {
                form = form.text(name.clone(), value.clone());
            }
            MultipartPart::File { name, path } => {
                let bytes = read_upload_file(path, remaining).await?;
                remaining -= bytes.len();
                let filename = std::path::Path::new(path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "upload".into());
                let ext = std::path::Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase());
                let mime = match ext.as_deref() {
                    Some("gif") => "image/gif",
                    Some("png") => "image/png",
                    Some("jpg") | Some("jpeg") => "image/jpeg",
                    Some("webp") => "image/webp",
                    Some("mp4") => "video/mp4",
                    _ => "application/octet-stream",
                };
                let field = reqwest::multipart::Part::bytes(bytes)
                    .file_name(filename)
                    .mime_str(mime)
                    .map_err(|_| AppError::api("data_invalid", 400))?;
                form = form.part(name.clone(), field);
            }
        }
    }
    Ok(form)
}

/// Read one regular file through the same handle used for metadata. Limit the
/// actual bytes, not just the observed length: files can grow during a read.
async fn read_upload_file(path: &str, limit: usize) -> AppResult<Vec<u8>> {
    let mut options = tokio::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    let file = options
        .open(path)
        .await
        .map_err(|_| AppError::api("data_invalid", 400))?;
    let metadata = file
        .metadata()
        .await
        .map_err(|_| AppError::api("data_invalid", 400))?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(AppError::api("data_invalid", 400));
    }
    read_bounded_upload(file, limit).await
}

async fn read_bounded_upload(
    reader: impl tokio::io::AsyncRead + Unpin,
    limit: usize,
) -> AppResult<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| AppError::api("data_invalid", 400))?;
    if bytes.len() > limit {
        return Err(AppError::api("data_invalid", 400));
    }
    Ok(bytes)
}

#[cfg(test)]
mod website_input_tests {
    use super::*;
    #[tokio::test]
    async fn file_reads_are_regular_bounded_and_share_one_multipart_budget() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".cache/upload-file-tests")
            .join(util::id());
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("owned-fixture.bin");
        std::fs::write(&path, b"abcd").unwrap();
        let path = path.to_str().unwrap();
        assert_eq!(read_upload_file(path, 4).await.unwrap(), b"abcd");
        assert!(read_upload_file(path, 3).await.is_err());
        assert!(read_upload_file(root.to_str().unwrap(), 4).await.is_err());
        // A stream that grows beyond the observed metadata is still capped.
        assert!(read_bounded_upload(&b"abcde"[..], 4).await.is_err());
        let parts = vec![
            crate::spec::MultipartPart::File {
                name: "one".into(),
                path: path.into(),
            },
            crate::spec::MultipartPart::File {
                name: "two".into(),
                path: path.into(),
            },
        ];
        assert!(build_multipart_form_bounded(&parts, 8).await.is_ok());
        assert!(build_multipart_form_bounded(&parts, 7).await.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fifo_is_rejected_without_waiting_for_a_writer() {
        use std::os::unix::ffi::OsStrExt;
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".cache/upload-file-tests")
            .join(util::id());
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("owned-fixture.fifo");
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        // A rescue writer bounds runtime shutdown even if nonblocking open regresses.
        let rescue_path = path.clone();
        let (cancel, canceled) = std::sync::mpsc::channel();
        let rescue_used = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let used = rescue_used.clone();
        let rescue = std::thread::spawn(move || {
            if canceled.recv_timeout(Duration::from_secs(2)).is_ok() {
                return;
            }
            used.store(true, std::sync::atomic::Ordering::SeqCst);
            use std::os::unix::fs::OpenOptionsExt;
            let _ = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(rescue_path);
        });
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            read_upload_file(path.to_str().unwrap(), 4),
        )
        .await;
        let _ = cancel.send(());
        rescue.join().unwrap();
        assert!(!rescue_used.load(std::sync::atomic::Ordering::SeqCst));
        assert!(result.unwrap().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn read_only_file_multipart_is_rejected_before_file_resolution() {
        let mut request = Request {
            url: "https://fixture.example.test/search".into(),
            method: "POST".into(),
            headers: vec![],
            body: None,
            account_id: String::new(),
            limit: 1024,
            timeout: Duration::from_secs(1),
            read_only: true,
            follow_redirects: false,
            tls_profile: String::new(),
            multipart: Some(vec![crate::spec::MultipartPart::File {
                name: "file".into(),
                path: "/synthetic/never-opened".into(),
            }]),
        };
        assert_eq!(
            request.check_read_headers().unwrap_err().public_code,
            "approval_required"
        );
        request.read_only = false;
        assert!(request.check_read_headers().is_ok());
    }
    #[test]
    fn read_headers_guard_is_pure_and_preserves_non_read_requests() {
        let mut request = Request {
            url: "https://fixture.example.test/items".into(),
            method: "GET".into(),
            headers: vec![("X-HTTP-Method-Override".into(), "synthetic-selector".into())],
            body: None,
            account_id: String::new(),
            limit: 1024,
            timeout: Duration::from_secs(1),
            read_only: true,
            follow_redirects: false,
            tls_profile: String::new(),
            multipart: None,
        };
        let error = request.check_read_headers().unwrap_err();
        assert_eq!(error.public_code, "approval_required");
        request.read_only = false;
        assert!(request.check_read_headers().is_ok());
        request.read_only = true;
        request.headers = vec![("Accept".into(), "application/json".into())];
        assert!(request.check_read_headers().is_ok());
    }
    #[tokio::test]
    async fn private_query_timeout_does_not_expose_wire_url() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".cache/pagination-tests")
            .join(util::id());
        std::fs::create_dir_all(&root).unwrap();
        let policy = Arc::new(Policy::open(&root.join("policy.db")).unwrap());
        let network = Network::new(policy, true);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/items", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    drop(stream);
                });
            }
        });
        let result = network
            .fetch_paginated(
                Request {
                    url,
                    method: "GET".into(),
                    headers: vec![],
                    body: None,
                    account_id: String::new(),
                    limit: 1024,
                    timeout: Duration::from_millis(30),
                    read_only: true,
                    follow_redirects: true,
                    tls_profile: String::new(),
                    multipart: None,
                },
                Some(("pageToken", "private-timeout-cursor")),
            )
            .await;
        let error = result.err().expect("slow response must time out");
        assert_eq!(error.public_code, "timeout");
        assert!(!format!("{error:?}").contains("private-timeout-cursor"));
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                assert!(
                    !String::from_utf8_lossy(&std::fs::read(path).unwrap())
                        .contains("private-timeout-cursor")
                );
            }
        }
        server.abort();
    }
    #[test]
    fn website_input_normalizes_domains_without_weakening_request_validation() {
        assert_eq!(
            website_url("  blog.example.com  ").unwrap().as_str(),
            "https://blog.example.com/"
        );
        assert_eq!(
            website_url("//example.org/path").unwrap().as_str(),
            "https://example.org/path"
        );
        assert!(validate_url("example.org").is_err());
        for value in [
            "",
            "file:///etc/passwd",
            "https://user:password@example.org",
            "javascript:alert(1)",
        ] {
            assert!(website_url(value).is_err(), "{value}");
        }
    }
}
