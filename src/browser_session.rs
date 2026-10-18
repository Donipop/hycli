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

pub async fn capture(
    credential: &Credential,
    entry: &str,
    allow_local: bool,
) -> AppResult<Capture> {
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
    let snapshot = crate::browser_storage::Snapshot::new().map_err(unavailable)?;
    if engine.protocol != "chromium-cdp" {
        return Err(AppError::api("browser_unavailable", 422));
    }
    let builder = BrowserConfig::builder()
        .chrome_executable(&engine.executable)
        .user_data_dir(&snapshot.0)
        .arg("--no-first-run")
        .arg("--disable-background-networking")
        .arg("--disable-dev-shm-usage")
        .arg("--disable-blink-features=AutomationControlled");
    let config = builder.build().map_err(unavailable)?;
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
        let mut value = network::CookieParam::new(&cookie.name, &cookie.value);
        value.url = Some(format!("{}/", credential.origin));
        if !cookie.host_only && !cookie.domain.is_empty() {
            value.domain = Some(cookie.domain.clone());
        }
        value.path = Some(cookie.path.clone());
        value.secure = Some(cookie.secure);
        value.http_only = Some(cookie.http_only);
        value.same_site = match cookie.same_site.as_str() {
            "strict" => Some(network::CookieSameSite::Strict),
            "lax" => Some(network::CookieSameSite::Lax),
            "no_restriction" => Some(network::CookieSameSite::None),
            _ => None,
        };
        page.set_cookie(value).await.map_err(unavailable)?;
    }
    let origin = serde_json::to_string(&credential.origin)?;
    let storage = serde_json::to_string(&credential.local_storage)?;
    page.evaluate_on_new_document(format!(
        "try{{Object.defineProperty(navigator,'webdriver',{{get:()=>undefined}})}}catch(_){{}}if(location.origin==={origin}){{for(const [key,value] of Object.entries({storage})){{localStorage.setItem(key,value)}}}}"
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
    let listener = tokio::spawn(async move {
        let mut requests = std::collections::HashMap::new();
        let mut identities = std::collections::HashMap::new();
        let mut checked_origins = BTreeMap::new();
        loop {
            tokio::select! {
                Some(event) = paused.next() => {
                    let parsed = crate::net::validate_url(&event.request.url).ok();
                    let same = parsed.as_ref().is_some_and(|u| crate::net::same_site_origin(u, &target_url));
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
                                    if capture.headers.is_empty() { capture.headers = headers.clone(); }
                                }
                            }
                            requests.insert(id.clone(), (parsed.unwrap(), headers, method));
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
                    if let Some((url, headers, req_method)) = requests.remove(&event.request_id) {
                        let status = event.response.status as u16;
                        if event.r#type == network::ResourceType::Document {
                            if let Ok(mut capture) = captured.lock() { capture.status = status; }
                        }
                        if (200..300).contains(&status) && matches!(event.r#type, network::ResourceType::Xhr | network::ResourceType::Fetch) {
                            if let Ok(mut capture) = captured.lock() {
                                if capture.observations.len() < 80 {
                                    capture.observations.push(Observation { method: req_method, path: url.path().into(), query: url.query_pairs().map(|(k, _)| (k.into_owned(), "string".into())).collect(), body: BTreeMap::new(), status, observed_at: util::now() });
                                }
                            }
                            if accounts::identity_endpoint(&url) { identities.insert(event.request_id.clone(), (url, headers)); }
                        }
                    }
                }
                Some(event) = finished.next() => {
                    if let Some((url, headers)) = identities.remove(&event.request_id) {
                        if let Ok(value) = identity_body(&task_page, event.request_id.clone()).await {
                            if let Some(identity) = accounts::extract_identity(&value) {
                                if let Ok(mut capture) = captured.lock() {
                                    capture.identity = Some(identity);
                                    capture.identity_url = url.to_string();
                                    capture.headers = headers;
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
        if result.lock().is_ok_and(|c| c.identity.is_some()) {
            break;
        }
        if anonymous && tick >= 2 && result.lock().is_ok_and(|c| (200..400).contains(&c.status)) {
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
