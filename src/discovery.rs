//! Published documentation discovery. Links and redirects are evidence, never executable instructions.
use crate::{
    apperr::{AppError, AppResult},
    net::Response,
    policy,
    runtime::Runtime,
};
use std::collections::BTreeSet;
use url::Url;

pub fn documentation_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    [
        "/docs",
        "/reference",
        "/resources",
        "/developers",
        "/documentation",
        "/openapi",
        "swagger",
        "llms.txt",
        "llms-full.txt",
        "docs.",
        "developer.",
        "api-reference",
        ".md",
        ".yaml",
        ".yml",
        ".json",
    ]
    .iter()
    .any(|part| lower.contains(part))
}

pub fn priority(url: &str) -> i32 {
    let lower = url.to_ascii_lowercase();
    if [
        ".mp4", ".webm", ".png", ".jpg", ".jpeg", ".gif", ".svg", ".woff", ".woff2", ".ttf",
        ".css", ".ico", ".mp3", ".zip", ".exe", ".dmg",
    ]
    .iter()
    .any(|suffix| lower.ends_with(suffix))
    {
        return -100;
    }
    if lower.contains("llms.txt") {
        return 1000;
    }
    if lower.contains("llms-full.txt") {
        return 50;
    }
    if lower.contains("openapi") || lower.contains("swagger") {
        return 950;
    }
    if lower.ends_with("/reference.md") || lower.ends_with("/reference") {
        return 920;
    }
    if lower.contains("/docs/") || lower.ends_with("/docs") {
        return 850;
    }
    if lower.contains("docs.") || lower.contains("developer.") {
        return 830;
    }
    if lower.contains("reference") && !lower.contains("/resources/") {
        return 800;
    }
    if lower.contains("oauth")
        || lower.contains("authentication")
        || lower.contains("authorization")
    {
        return 780;
    }
    if lower.contains("/resources/") || lower.contains("api-reference/") {
        return 750;
    }
    if lower.ends_with(".md") {
        return 700;
    }
    if lower.ends_with(".js") {
        if ["polyfill", "vendor", "framework", "runtime", "turbopack"]
            .iter()
            .any(|noise| lower.contains(noise))
        {
            return 100;
        }
        return 300;
    }
    200
}

pub fn api_prefixes(initial: &Url, text: &str) -> BTreeSet<String> {
    let mut prefixes: BTreeSet<String> = regex::Regex::new(r#"https?://[^\s<>"'`\\)\]}]+"#)
        .expect("constant")
        .find_iter(text)
        .filter_map(|m| Url::parse(m.as_str()).ok())
        .filter(|url| url.origin() == initial.origin() && url.query().is_none())
        .map(|url| url.path().trim_end_matches('/').to_string())
        .filter(|path| {
            regex::Regex::new(r"^/(?:api(?:/v[0-9]+)?|v[0-9]+)$")
                .unwrap()
                .is_match(path)
        })
        .collect();
    // Some references publish /api/v{version} plus an explicit version availability table.
    if text.contains(&format!(
        "{}/api/v{{",
        initial.origin().ascii_serialization()
    )) {
        for row in regex::Regex::new(r"(?i)\b([0-9]{1,4})\s*(?:\|\s*)?(?:Available|Supported)\b")
            .unwrap()
            .captures_iter(text)
        {
            prefixes.insert(format!("/api/v{}", &row[1]));
        }
    }
    prefixes
}

/// Exact published server addresses; origin changes never imply a credential grant.
pub fn api_bases(base: &Url, text: &str) -> BTreeSet<String> {
    let mut servers = BTreeSet::new();
    if let Some(document) = api_schema(text) {
        if document.get("openapi").is_some() {
            let mut nodes = vec![&document];
            if let Some(paths) = document.get("paths").and_then(|v| v.as_object()) {
                for path in paths.values() {
                    nodes.push(path);
                    if let Some(operations) = path.as_object() {
                        nodes.extend(operations.values().filter(|v| v.is_object()));
                    }
                }
            }
            let mut root_has_server = false;
            for (index, node) in nodes.into_iter().enumerate() {
                for server in node
                    .get("servers")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                {
                    if let Some(raw) = server.get("url").and_then(|v| v.as_str()) {
                        let mut raw = raw.to_string();
                        if let Some(variables) = server.get("variables").and_then(|v| v.as_object())
                        {
                            for (name, variable) in variables {
                                if let Some(default) =
                                    variable.get("default").and_then(|v| v.as_str())
                                {
                                    raw = raw.replace(&format!("{{{name}}}"), default);
                                }
                            }
                        }
                        if !raw.contains(['{', '}']) {
                            if let Ok(url) = base.join(&raw) {
                                if safe_server(&url) {
                                    servers.insert(url.to_string().trim_end_matches('/').into());
                                    root_has_server |= index == 0;
                                }
                            }
                        }
                    }
                }
            }
            if !root_has_server
                && document
                    .get("servers")
                    .is_none_or(|v| v.as_array().is_some_and(Vec::is_empty))
            {
                servers.insert(base.origin().ascii_serialization());
            }
        }
        if document.get("swagger").is_some() {
            let prefix = document
                .get("basePath")
                .and_then(|v| v.as_str())
                .unwrap_or("/");
            let schemes: Vec<_> = document
                .get("schemes")
                .and_then(|v| v.as_array())
                .map(|values| values.iter().filter_map(|v| v.as_str()).collect())
                .filter(|values: &Vec<&str>| !values.is_empty())
                .unwrap_or_else(|| vec![base.scheme()]);
            for scheme in schemes {
                let origin = document
                    .get("host")
                    .and_then(|v| v.as_str())
                    .map(|host| format!("{scheme}://{host}"))
                    .unwrap_or_else(|| {
                        let mut url = base.clone();
                        let _ = url.set_scheme(scheme);
                        url.origin().ascii_serialization()
                    });
                if let Ok(url) = Url::parse(&format!("{origin}{prefix}")) {
                    if safe_server(&url) {
                        servers.insert(url.to_string().trim_end_matches('/').into());
                    }
                }
            }
        }
    }
    let visible = crate::runtime::visible_text(text);
    let pattern = regex::Regex::new(r#"(?i)(?:base\s+url|api\s+(?:base\s+)?(?:url|endpoint|server))\s*[:=]?\s*(https?://[^\s<>"'`]+)"#).unwrap();
    for matched in pattern.captures_iter(&visible) {
        if let Ok(url) = Url::parse(matched[1].trim_end_matches(['.', ','])) {
            if url.query().is_none()
                && url.fragment().is_none()
                && url.username().is_empty()
                && url.password().is_none()
            {
                servers.insert(url.to_string().trim_end_matches('/').into());
            }
        }
    }
    // A published absolute request example also establishes its server origin,
    // even when the documentation does not use the literal words "Base URL".
    let routes = documented_routes(&visible);
    for matched in regex::Regex::new(r#"https?://[^\s<>"'`\\)\]]+"#)
        .unwrap()
        .find_iter(&visible)
    {
        if let Ok(url) = Url::parse(matched.as_str().trim_end_matches(['.', ','])) {
            if url.username().is_empty()
                && url.password().is_none()
                && url.path() != "/"
                && routes
                    .iter()
                    .any(|(_, path)| route_matches(url.path(), path, &BTreeSet::new()))
            {
                servers.insert(url.origin().ascii_serialization());
            }
        }
    }
    servers
}

fn safe_server(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

/// OpenAPI can be published as JSON or YAML. Never interpret an ordinary page as a schema.
pub fn api_schema(text: &str) -> Option<serde_json::Value> {
    let document = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .or_else(|| {
            if regex::Regex::new(r"(?m)^(?:openapi|swagger)\s*:")
                .unwrap()
                .is_match(text)
            {
                serde_yaml::from_str::<serde_json::Value>(text).ok()
            } else {
                None
            }
        })?;
    if document.get("openapi").is_some() || document.get("swagger").is_some() {
        Some(document)
    } else {
        None
    }
}

pub fn route_matches(path: &str, documented: &str, prefixes: &BTreeSet<String>) -> bool {
    fn shape(path: &str) -> String {
        regex::Regex::new(r"\{\{[^}]+\}\}|\{[^}]+\}")
            .unwrap()
            .replace_all(path, "{}")
            .into_owned()
    }
    fn matches(path: &str, template: &str) -> bool {
        if shape(path) == shape(template) {
            return true;
        }
        let variables = regex::Regex::new(r"\{\{[^}/]+\}\}|\{[^}/]+\}").unwrap();
        let mut pattern = String::from("^");
        let mut end = 0;
        for variable in variables.find_iter(template) {
            pattern.push_str(&regex::escape(&template[end..variable.start()]));
            pattern.push_str("[^/]+");
            end = variable.end();
        }
        pattern.push_str(&regex::escape(&template[end..]));
        pattern.push('$');
        regex::Regex::new(&pattern).is_ok_and(|expression| expression.is_match(path))
    }
    matches(path, documented)
        || prefixes
            .iter()
            .any(|prefix| matches(path, &format!("{prefix}{documented}")))
}

pub fn ranked<'a>(links: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut result: Vec<_> = links.into_iter().cloned().collect();
    result.sort_by_key(|url| {
        (
            std::cmp::Reverse(priority(url) + if url.ends_with(".md") { 30 } else { 0 }),
            url.clone(),
        )
    });
    result
}

pub fn published_links(base: &Url, text: &str) -> BTreeSet<String> {
    let mut links = BTreeSet::new();
    let expressions = [
        r#"(?i)(?:href|src)\s*=\s*["']([^"'<>\s]+)["']"#,
        r#"\]\((https?://[^\s)]+|/[^\s)]+)\)"#,
        r#"["']((?:/|https?://)[^"'<>\s]{1,1000})["']"#,
    ];
    for expression in expressions {
        for capture in regex::Regex::new(expression)
            .expect("constant")
            .captures_iter(text)
            .take(4000)
        {
            let raw = capture[1].replace("&amp;", "&");
            if raw.contains(['{', '}', '(', ')', '*']) || raw.contains("/:") {
                continue;
            }
            let Ok(mut target) = base.join(&raw) else {
                continue;
            };
            target.set_fragment(None);
            if !matches!(target.scheme(), "http" | "https")
                || (base.scheme() == "https" && target.scheme() != "https")
                || !target.username().is_empty()
                || target.password().is_some()
                || target.query().is_some()
                || target.path().contains(['{', '}', '(', ')', '*'])
                || target.path().contains("/:")
                || !crate::browser_session::navigation_safe(&target)
            {
                continue;
            }
            // Link meaning is evaluated by the preparation agent, not a site-specific path list.
            links.insert(target.to_string());
        }
    }
    links
}

pub fn header_links(base: &Url, value: &str) -> BTreeSet<String> {
    regex::Regex::new(r"<([^>]+)>[^,]*")
        .expect("constant")
        .captures_iter(value)
        .filter(|capture| {
            [
                "llms-txt",
                "alternate",
                "service-desc",
                "describedby",
                "api-catalog",
            ]
            .iter()
            .any(|rel| capture[0].contains(rel))
        })
        .filter_map(|capture| base.join(&capture[1]).ok())
        .filter(|url| {
            matches!(url.scheme(), "https" | "http")
                && policy::safe_read_url(url)
                && url.query().is_none()
        })
        .map(|url| url.to_string())
        .collect()
}

/// HTTP routes written in reference prose/Markdown, rather than arbitrary navigation paths.
pub fn documented_routes(text: &str) -> BTreeSet<(String, String)> {
    let text = text
        .replace("\\/", "/")
        .replace("&lbrace;", "{")
        .replace("&rbrace;", "}");
    let parameter_links =
        regex::Regex::new(r"\[((?:\\)?\{[A-Za-z0-9_.-]+(?:\\)?\})\]\([^)]*\)").expect("constant");
    let text = parameter_links
        .replace_all(&text, "$1")
        .replace("\\{", "{")
        .replace("\\}", "}");
    let expression = regex::Regex::new(r"(?i)\b(GET|HEAD|POST|PUT|PATCH|DELETE)\s+(?:https?://[^/\s`<>]+)?(/[A-Za-z0-9_/@{}.:%+~!$&*=-]+)").expect("constant");
    expression
        .captures_iter(&text)
        .take(500)
        .filter_map(|capture| {
            let path = capture[2].trim_end_matches(['.', ':']).to_owned();
            if path.len() > 500 || path == "/" || path.contains("...") {
                return None;
            }
            Some((capture[1].to_ascii_uppercase(), path))
        })
        .collect()
}

fn normalize_bundle_path(raw: &str) -> Option<String> {
    let mut cleaned = raw.trim().replace("\\/", "/");
    if let Some((before_q, _)) = cleaned.split_once('?') {
        cleaned = before_q.to_owned();
    }
    if let Some((before_semi, _)) = cleaned.split_once(';') {
        cleaned = before_semi.to_owned();
    }
    if let Some((before_hash, _)) = cleaned.split_once('#') {
        cleaned = before_hash.to_owned();
    }
    // Convert JS template expressions `${encodeURIComponent(slug)}` or `${item.id}` -> `{slug}` / `{id}`
    let template_re = regex::Regex::new(r"\$\{([^}]+)\}").expect("constant");
    cleaned = template_re
        .replace_all(&cleaned, |caps: &regex::Captures<'_>| {
            let expr = &caps[1];
            let ident = regex::Regex::new(r"[A-Za-z_][A-Za-z0-9_]*")
                .expect("constant")
                .find_iter(expr)
                .map(|m| m.as_str())
                .filter(|w| !matches!(*w, "encodeURIComponent" | "String" | "Number" | "this"))
                .last()
                .unwrap_or("id");
            format!("{{{ident}}}")
        })
        .into_owned();
    // Convert Express/router style `:id` segments -> `{id}`
    let colon_re = regex::Regex::new(r"/:([A-Za-z_][A-Za-z0-9_]*)").expect("constant");
    cleaned = colon_re.replace_all(&cleaned, "/{$1}").into_owned();
    // Sanitize all `{...}` segments to valid lowercase parameter IDs
    let brace_re = regex::Regex::new(r"\{([^}]+)\}").expect("constant");
    cleaned = brace_re
        .replace_all(&cleaned, |caps: &regex::Captures<'_>| {
            let raw_id = caps[1].trim();
            let sanitized = if matches!(
                raw_id,
                "String" | "Number" | "encodeURIComponent" | "this" | "undefined" | "null"
            ) {
                "id".to_string()
            } else {
                raw_id
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() {
                            c.to_ascii_lowercase()
                        } else {
                            '_'
                        }
                    })
                    .collect::<String>()
                    .trim_matches('_')
                    .to_string()
            };
            let final_id = if crate::util::valid_id(&sanitized) {
                sanitized
            } else {
                "id".to_string()
            };
            format!("{{{final_id}}}")
        })
        .into_owned();
    let trimmed = cleaned.trim_end_matches('/').to_owned();
    let path = if trimmed.is_empty() {
        "/".to_owned()
    } else {
        trimmed
    };
    if !path.starts_with('/')
        || path.starts_with("//")
        || path == "/"
        || path.len() > 300
        || path.contains([' ', '\t', '\r', '\n', '<', '>', '"', '\'', '`', '\\', '*'])
    {
        return None;
    }
    let lower = path.to_ascii_lowercase();
    if [
        ".js", ".css", ".png", ".jpg", ".jpeg", ".gif", ".svg", ".woff", ".woff2", ".ttf", ".ico",
        ".map", ".webp", ".mp4", ".mp3",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
    {
        return None;
    }
    Some(path)
}

/// Extract HTTP routes from inline `<script>` blocks and JS bundles (`fetch`, `axios`, `/api/...`, GraphQL).
pub fn bundle_routes(text: &str) -> BTreeSet<(String, String)> {
    let mut routes = BTreeSet::new();
    // 1. Normalize JS string concatenation inside fetch(...) e.g.
    //    fetch('/api/entries/' + encodeURIComponent(id) + '/comments', { method: 'POST' })
    let concat_re = regex::Regex::new(
        r#"["'`]\s*\+\s*(?:(?:encodeURIComponent|String|Number)\s*\(\s*)?([A-Za-z_][A-Za-z0-9_.]*)\s*\)?\s*(?:\+\s*["'`])?"#,
    )
    .expect("constant");
    let normalized = concat_re
        .replace_all(text, |caps: &regex::Captures<'_>| {
            let raw_ident = caps[1].rsplit('.').next().unwrap_or("id");
            let ident = if matches!(
                raw_ident,
                "String" | "Number" | "encodeURIComponent" | "this"
            ) {
                "id".to_string()
            } else {
                raw_ident.to_ascii_lowercase()
            };
            let has_trailing_quote =
                caps[0].ends_with('"') || caps[0].ends_with('\'') || caps[0].ends_with('`');
            if has_trailing_quote {
                format!("{{{ident}}}")
            } else {
                format!("{{{ident}}}\"")
            }
        })
        .into_owned();

    // 2. Match `fetch("...", { ... })` calls with optional `method: "..."`
    let fetch_re = regex::Regex::new(
        r#"fetch\s*\(\s*["'`]((?:https?://[^/"'`\s]+)?/[^"'`\s]{1,260})["'`](?:\s*,\s*\{([^}]{0,320})\})?"#,
    )
    .expect("constant");
    let method_re =
        regex::Regex::new(r#"(?i)\bmethod\s*:\s*["'`](GET|HEAD|POST|PUT|PATCH|DELETE)["'`]"#)
            .expect("constant");
    for cap in fetch_re.captures_iter(&normalized).take(1000) {
        let raw_target = &cap[1];
        let raw_path = if raw_target.starts_with("http://") || raw_target.starts_with("https://") {
            let Ok(u) = Url::parse(&raw_target.replace('{', "_").replace('}', "_")) else {
                continue;
            };
            let Some(idx) = raw_target
                .find("://")
                .and_then(|i| raw_target[i + 3..].find('/').map(|j| i + 3 + j))
            else {
                continue;
            };
            let _ = u;
            &raw_target[idx..]
        } else {
            raw_target
        };
        let Some(path) = normalize_bundle_path(raw_path) else {
            continue;
        };
        let method = cap
            .get(2)
            .and_then(|opts| method_re.captures(opts.as_str()))
            .map(|m| m[1].to_ascii_uppercase())
            .unwrap_or_else(|| "GET".into());
        routes.insert((method, path));
    }

    // 3. Match `axios.<method>("/path")` or `.get("/api/...")` / `.post("/api/...")`
    let axios_re = regex::Regex::new(
        r#"(?i)\b(?:axios|client|http|api)\s*\.\s*(get|post|put|patch|delete|head)\s*\(\s*["'`]((?:https?://[^/"'`\s]+)?/[^"'`\s]{1,260})["'`]"#,
    )
    .expect("constant");
    for cap in axios_re.captures_iter(&normalized).take(1000) {
        let method = cap[1].to_ascii_uppercase();
        let raw_target = &cap[2];
        let raw_path = if let Some(idx) = raw_target
            .find("://")
            .and_then(|i| raw_target[i + 3..].find('/').map(|j| i + 3 + j))
        {
            &raw_target[idx..]
        } else {
            raw_target
        };
        if let Some(path) = normalize_bundle_path(raw_path) {
            routes.insert((method, path));
        }
    }

    // 4. Match explicit API path string literals: `/api/...`, `/_api/...`, `/v1/...`, `/v2/...`, `/v3/...`, `/graphql`, `/gql`, `/_next/data/...`
    let api_lit_re = regex::Regex::new(
        r#"["'`](/(?:_?api(?:/v[0-9]+)?|v[1-9][0-9]*|graphql|gql|rest|_next/data)(?:/[A-Za-z0-9_@.{}:$;=-]+)*)["'`]"#,
    )
    .expect("constant");
    for m in api_lit_re.captures_iter(&normalized).take(1500) {
        let Some(path) = normalize_bundle_path(&m[1]) else {
            continue;
        };
        let start = m.get(0).map(|x| x.start()).unwrap_or(0);
        let end = m.get(0).map(|x| x.end()).unwrap_or(0);
        let window_start = normalized.floor_char_boundary(start.saturating_sub(120));
        let window_end = normalized.ceil_char_boundary((end + 180).min(normalized.len()));
        let nearby = &normalized[window_start..window_end];
        if let Some(mc) = method_re.captures(nearby) {
            routes.insert((mc[1].to_ascii_uppercase(), path.clone()));
        } else if path.contains("graphql") || path.ends_with("/gql") {
            routes.insert(("POST".into(), path));
        } else if crate::policy::dangerous_intent(&path) {
            routes.insert(("POST".into(), path));
        } else {
            routes.insert(("GET".into(), path));
        }
    }

    // 5. Match `new URL("/path", ...)` and Schema.org `urlTemplate` routes
    let new_url_re = regex::Regex::new(
        r#"(?:new\s+URL\s*\(\s*|["']urlTemplate["']\s*:\s*)["'`]((?:https?://[^/"'`\s]+)?/[^"'`\s]{1,260})["'`]"#,
    )
    .expect("constant");
    for cap in new_url_re.captures_iter(&normalized).take(500) {
        let raw_target = &cap[1];
        let raw_path = if let Some(idx) = raw_target
            .find("://")
            .and_then(|i| raw_target[i + 3..].find('/').map(|j| i + 3 + j))
        {
            &raw_target[idx..]
        } else {
            raw_target
        };
        if let Some(path) = normalize_bundle_path(raw_path) {
            routes.insert(("GET".into(), path));
        }
    }
    let search_lit_re = regex::Regex::new(
        r#"["'`](/(?:[a-z]{2}/)?search(?:/[A-Za-z0-9_.-]+)?)(?:\?[A-Za-z0-9_=&.-]*)?["'`]"#,
    )
    .expect("constant");
    for cap in search_lit_re.captures_iter(&normalized).take(200) {
        if let Some(path) = normalize_bundle_path(&cap[1]) {
            routes.insert(("GET".into(), path));
        }
    }

    // 6. Match HTML `<form action="..." method="...">` routes
    let form_re = regex::Regex::new(r"(?is)<form\b([^>]*)>").expect("constant");
    let action_re = regex::Regex::new(
        r#"(?i)\baction\s*=\s*["']((?:https?://[^/"'\s]+)?/[^"'\s<>]{1,260})["']"#,
    )
    .expect("constant");
    let form_method_re =
        regex::Regex::new(r#"(?i)\bmethod\s*=\s*["'](GET|POST|PUT|PATCH|DELETE)["']"#)
            .expect("constant");
    for cap in form_re.captures_iter(text).take(200) {
        let attrs = &cap[1];
        let Some(act) = action_re.captures(attrs) else {
            continue;
        };
        let raw_target = &act[1];
        let raw_path = if let Some(idx) = raw_target
            .find("://")
            .and_then(|i| raw_target[i + 3..].find('/').map(|j| i + 3 + j))
        {
            &raw_target[idx..]
        } else {
            raw_target
        };
        if let Some(path) = normalize_bundle_path(raw_path) {
            let method = form_method_re
                .captures(attrs)
                .map(|m| m[1].to_ascii_uppercase())
                .unwrap_or_else(|| "GET".into());
            routes.insert((method, path));
        }
    }
    routes
}

fn bundle_priority(url: &str) -> usize {
    let lower = url.to_ascii_lowercase();
    if [
        "polyfill",
        "vendor",
        "webpack-",
        "framework-",
        "runtime-",
        "main-app-",
        "commons-",
        "fd9d1056-",
    ]
    .iter()
    .any(|low| lower.contains(low))
    {
        return 3;
    }
    if [
        "/app/",
        "/pages/",
        "/routes/",
        "/_remix/",
        "/page-",
        "/_index-",
        "/index-",
        "entry.client",
        "search",
        "query",
        "mutation",
        "graphql",
        "api",
        "manifest",
    ]
    .iter()
    .any(|hi| lower.contains(hi))
    {
        return 0;
    }
    2
}

/// Extract application JS bundle URLs from `<script src>`, `<link rel="modulepreload">`, and inline ES module imports.
pub fn bundle_urls(base: &Url, html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let script_re = regex::Regex::new(r#"(?is)<script\b[^>]*\bsrc\s*=\s*["']([^"'<>\s]+)["']"#)
        .expect("constant");
    let link_re = regex::Regex::new(
        r#"(?is)<link\b[^>]*\b(?:rel\s*=\s*["']modulepreload["']|as\s*=\s*["']script["'])[^>]*\bhref\s*=\s*["']([^"'<>\s]+)["']|<link\b[^>]*\bhref\s*=\s*["']([^"'<>\s]+)["'][^>]*\b(?:rel\s*=\s*["']modulepreload["']|as\s*=\s*["']script["'])"#,
    )
    .expect("constant");
    let import_re = regex::Regex::new(
        r#"\bimport\s+(?:[^"'`;\n]{1,120}\s+from\s+)?["']((?:https?://[^/"'\s]+)?/[^"'\s]+\.m?js(?:\?[^"'\s]*)?)["']"#,
    )
    .expect("constant");

    let raw_candidates = script_re
        .captures_iter(html)
        .take(120)
        .filter_map(|c| c.get(1).map(|m| m.as_str()))
        .chain(
            link_re
                .captures_iter(html)
                .take(120)
                .filter_map(|c| c.get(1).or_else(|| c.get(2)).map(|m| m.as_str())),
        )
        .chain(
            import_re
                .captures_iter(html)
                .take(120)
                .filter_map(|c| c.get(1).map(|m| m.as_str())),
        );

    for raw_match in raw_candidates {
        let raw = raw_match.replace("&amp;", "&");
        let Ok(mut url) = base.join(&raw) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https") {
            continue;
        }
        url.set_fragment(None);
        let lower = url.as_str().to_ascii_lowercase();
        if [
            "googletagmanager",
            "google-analytics",
            "doubleclick",
            "clarity.ms",
            "hotjar",
            "sentry",
            "facebook.net",
            "connect.facebook",
            "amplitude.com",
            "datadoghq",
            "braze",
            "intercom",
            "channel.io",
            "wcs.naver.net",
            "kakao.com/sdk",
        ]
        .iter()
        .any(|skip| lower.contains(skip))
        {
            continue;
        }
        let path_lower = url.path().to_ascii_lowercase();
        if crate::net::same_site_origin(&url, base)
            || path_lower.ends_with(".js")
            || path_lower.ends_with(".mjs")
            || lower.contains("/_next/static/")
            || lower.contains("/_remix/")
            || lower.contains("/assets/")
        {
            let s = url.to_string();
            if seen.insert(s.clone()) {
                out.push(s);
            }
        }
        if out.len() >= 80 {
            break;
        }
    }
    let base_pri: Vec<usize> = out.iter().map(|u| bundle_priority(u)).collect();
    let mut indexed: Vec<(usize, String)> = out.into_iter().enumerate().collect();
    indexed.sort_by_key(|(i, _)| {
        let p = base_pri[*i];
        if p == 2 && (base_pri.get(i + 1) == Some(&0) || base_pri.get(i + 2) == Some(&0)) {
            1
        } else {
            p
        }
    });
    let mut out: Vec<String> = indexed.into_iter().map(|(_, u)| u).collect();
    out.truncate(24);
    out
}

/// Extract same-site API server origins referenced inside HTML or JS bundles.
pub fn bundle_api_bases(base: &Url, text: &str) -> BTreeSet<String> {
    let mut bases = BTreeSet::new();
    let url_re = regex::Regex::new(
        r#"https?://[A-Za-z0-9.-]+(?::[0-9]+)?(?:/api(?:/v[0-9]+)?|/v[1-9][0-9]*|/graphql|/gql|/rest)?"#,
    )
    .expect("constant");
    for m in url_re.find_iter(text).take(1000) {
        if let Ok(url) = Url::parse(m.as_str()) {
            if !safe_server(&url) || !crate::net::same_site_origin(&url, base) {
                continue;
            }
            let host = url.host_str().unwrap_or("").to_ascii_lowercase();
            if [
                "images.", "img.", "cdn.", "assets.", "static.", "media.", "cache.", "fonts.",
            ]
            .iter()
            .any(|p| host.starts_with(p))
            {
                continue;
            }
            let has_api_path = url.path() != "/" && !url.path().is_empty();
            let has_api_host = [
                "api.",
                "api-",
                "v1.",
                "v2.",
                "v3.",
                "gql.",
                "graphql.",
                "gateway.",
                "backend.",
                "studio-api",
            ]
            .iter()
            .any(|p| host.starts_with(p));
            if has_api_path || has_api_host || url.origin() == base.origin() {
                bases.insert(url.origin().ascii_serialization());
            }
        }
    }
    bases
}

impl Runtime {
    /// Public documentation can redirect to a published documentation host. Cookies and all
    /// other account material stay on the original origin; every hop still uses Network checks.
    pub(crate) async fn read_evidence_page(&self, raw: &str, account: &str) -> AppResult<Response> {
        let original = crate::net::validate_url(raw)?;
        let mut next = original.clone();
        for hop in 0..5 {
            if !policy::safe_read_url(&next)
                || (original.scheme() == "https" && next.scheme() != "https")
            {
                return Err(AppError::api("unsafe_target", 400));
            }
            let identity = if next.origin() == original.origin() {
                account
            } else {
                ""
            };
            let response = self.read_page(next.as_str(), identity).await?;
            if !matches!(response.status, 301 | 302 | 303 | 307 | 308) || hop == 4 {
                return Ok(response);
            }
            let location = response.header("location");
            if location.is_empty() {
                return Ok(response);
            }
            next = crate::net::validate_url(&response.url)?
                .join(&location)
                .map_err(|_| AppError::api("bad_url", 400))?;
            // Some sites still publish an HTTP location on their secure entry
            // point. Try its secure equivalent; never send a downgraded request.
            if original.scheme() == "https" && next.scheme() == "http" {
                next.set_scheme("https")
                    .map_err(|_| AppError::api("bad_url", 400))?;
            }
        }
        Err(AppError::api("request_failed", 502))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn yaml_servers_resolve_defaults_variables_and_operation_overrides() {
        let source = Url::parse("https://docs.example.test:8443/reference/openapi.yaml").unwrap();
        let text = "openapi: 3.0.4\nservers:\n  - url: https://{tenant}.example.test/{version}\n    variables:\n      tenant: {default: api}\n      version: {default: v2}\npaths:\n  /items:\n    get:\n      servers:\n        - url: ../special\n";
        assert!(api_schema(text).is_some());
        let bases = api_bases(&source, text);
        assert!(bases.contains("https://api.example.test/v2"));
        assert!(bases.contains("https://docs.example.test:8443/special"));
        assert_eq!(
            api_bases(&source, "openapi: 3.0.4\npaths: {}"),
            BTreeSet::from(["https://docs.example.test:8443".into()])
        );
        assert_eq!(
            api_bases(&source, "swagger: '2.0'\nbasePath: /v1\npaths: {}"),
            BTreeSet::from(["https://docs.example.test:8443/v1".into()])
        );
        assert!(
            api_bases(
                &source,
                r#"{"openapi":"3.0.4","servers":[{"url":"https://user:secret@api.example.test"}]}"#
            )
            .is_empty()
        );
        assert!(api_schema("<html>openapi: is not a schema</html>").is_none());
    }
    #[test]
    fn api_servers_are_published_evidence_with_independent_origins() {
        let source = Url::parse("https://docs.example.net/openapi.json").unwrap();
        let bases = api_bases(
            &source,
            r#"{"openapi":"3.1.0","servers":[{"url":"https://api.example.com/v2"},{"url":"/local-api"}]}"#,
        );
        assert!(bases.contains("https://api.example.com/v2"));
        assert!(bases.contains("https://docs.example.net/local-api"));
        assert!(
            api_bases(&source, "<p>Base URL https://api.example.com/v3</p>")
                .contains("https://api.example.com/v3")
        );
        assert!(api_bases(&source, "<a href='https://unrelated.example.com'>Link</a>").is_empty());
        let examples = "GET /search/widgets\ncurl -H 'Accept: application/json' https://api.example.com/search/widgets?q=sample\nSee https://unrelated.example.net/elsewhere";
        let bases = api_bases(&source, examples);
        assert_eq!(bases, BTreeSet::from(["https://api.example.com".into()]));
        assert!(route_matches(
            "/teams/demo/items/123",
            "/teams/{team}/items/{item}",
            &BTreeSet::new()
        ));
        assert!(!route_matches(
            "/teams/demo/items/123/delete",
            "/teams/{team}/items/{item}",
            &BTreeSet::new()
        ));
    }
    #[test]
    fn follows_published_docs_across_hosts_and_markdown_indices() {
        let home = Url::parse("https://example.com/").unwrap();
        let links = published_links(
            &home,
            r#"<a href="https://docs.example.net/reference#start">Docs</a><a href="/logout">exit</a><script src="/assets/app.js"></script>"#,
        );
        assert!(links.contains("https://docs.example.net/reference"));
        assert!(!links.iter().any(|url| url.contains("logout")));
        let invalid = published_links(
            &home,
            r#"<a href='/resources/:type(.:format)'>Template</a><a href='/teams/{team}'>Template</a>"#,
        );
        assert!(invalid.is_empty());
        assert!(
            priority("https://docs.example.net/")
                > priority("https://example.com/resources/articles")
        );
        let docs = Url::parse("https://docs.example.net/llms.txt").unwrap();
        let index = published_links(
            &docs,
            "[Users](https://docs.example.net/resources/users.md)\n[Auth](/reference/authentication.md)",
        );
        assert_eq!(index.len(), 2);
        assert_eq!(
            ranked(links.iter())[0],
            "https://docs.example.net/reference"
        );
    }
    #[test]
    fn reference_routes_keep_methods_and_parameter_evidence() {
        let routes = documented_routes(
            "## Current user % GET /users/@me\n## Messages % GET /channels/{channel.id}/messages\n## Send % POST /channels/{channel.id}/messages",
        );
        assert!(routes.contains(&("GET".into(), "/users/@me".into())));
        assert!(routes.contains(&("POST".into(), "/channels/{channel.id}/messages".into())));
        assert!(!routes.contains(&("DELETE".into(), "/users/@me".into())));
        let base = Url::parse("https://example.com").unwrap();
        let prefixes = api_prefixes(
            &base,
            "Base https://example.com/api Version URL https://example.com/api/v{version_number} Version Status Default 10 Available 9 Available 8 Deprecated",
        );
        assert!(route_matches("/api/v10/users/@me", "/users/@me", &prefixes));
        assert!(!prefixes.contains("/api/v8"));
    }
    #[test]
    fn documentation_components_preserve_verbs_and_linked_parameters() {
        let text = crate::runtime::visible_text(
            r##"<Endpoint method="GET">/current-context</Endpoint>
<ApiRoute className="route" method='PATCH'>/teams/[\{team.id\}](/reference/team)/items/[\{item.id\}](/reference/item)</ApiRoute>"##,
        );
        let routes = documented_routes(&text);
        assert!(routes.contains(&("GET".into(), "/current-context".into())));
        assert!(routes.contains(&("PATCH".into(), "/teams/{team.id}/items/{item.id}".into())));
        assert_eq!(routes.len(), 2);
        let wrapped = crate::runtime::visible_text(
            r#"<span>get</span><span><wbr/>/teams<wbr/>/{team}<wbr/>/items</span><span>post</span><span><wbr>/teams<wbr>/{team}<wbr>/items</span>"#,
        );
        let routes = documented_routes(&wrapped);
        assert_eq!(
            routes,
            BTreeSet::from([
                ("GET".into(), "/teams/{team}/items".into()),
                ("POST".into(), "/teams/{team}/items".into())
            ])
        );
    }
    #[test]
    fn bundle_routes_and_urls_handle_multibyte_utf8_remix_preload_and_priority_sorting() {
        let korean_js = format!(
            "{}\"/api/v1/topics\"{}; new URL(\"/api/trending-posts?limit=20\", window.location.origin); {{\"urlTemplate\":\"https://www.daangn.com/kr/search/?q={{search_string}}\"}}",
            "가나다라마바사아자차카타파하을를이히".repeat(4),
            "한글텍스트경계테스트입니다".repeat(4)
        );
        let routes = bundle_routes(&korean_js);
        assert!(routes.contains(&("GET".into(), "/api/v1/topics".into())));
        assert!(routes.contains(&("GET".into(), "/api/trending-posts".into())));
        assert!(routes.contains(&("GET".into(), "/kr/search".into())));

        let base = Url::parse("https://www.daangn.com/kr/").unwrap();
        let html = r#"
            <script src="/_next/static/chunks/polyfills-abc.js"></script>
            <script src="https://assets.velcdn.com/_next/static/chunks/app/(list)/page-123.js"></script>
            <link rel="modulepreload" href="/_remix/entry.client-xyz.js" />
            <script type="module">import * as route0 from "/_remix/routes/kr.search.buy-sell._index-def.js";</script>
        "#;
        let urls = bundle_urls(&base, html);
        assert_eq!(urls.len(), 4);
        assert!(urls[0].contains("page-123.js") || urls[0].contains("_remix"));
        assert!(urls.last().unwrap().contains("polyfills-abc.js"));
    }
}
