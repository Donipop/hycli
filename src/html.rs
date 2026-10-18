//! Bounded structural HTML evidence and declarative extraction, without script execution.
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Field {
    pub selector: String,
    /// Empty selects text; otherwise selects one named attribute.
    pub attribute: String,
}

impl Field {
    pub fn extract(&self, document: &Html) -> Option<String> {
        self.extract_bounded(document, 320)
    }
    /// Response guards check structure, not the size limit of an account identity.
    pub fn is_present(&self, document: &Html) -> bool {
        self.extract_bounded(document, usize::MAX).is_some()
    }
    fn extract_bounded(&self, document: &Html, max_length: usize) -> Option<String> {
        if self.selector.is_empty() || self.selector.len() > 500 || self.attribute.len() > 80 {
            return None;
        }
        let selector = Selector::parse(&self.selector).ok()?;
        let mut matches = document.select(&selector);
        let element = matches.next()?;
        // Never silently choose a person from a list of profiles or an account switcher.
        if matches.next().is_some() {
            return None;
        }
        let value = if self.attribute.is_empty() {
            element_text(element)
        } else {
            element.value().attr(&self.attribute)?.to_string()
        };
        let value = value.trim();
        if value.is_empty() || value.len() > max_length || value.contains(['\r', '\n']) {
            None
        } else {
            Some(value.into())
        }
    }
}

/// Repeated page records and optional page-level fields, declared from observed DOM evidence.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Extraction {
    pub items: String,
    pub fields: std::collections::BTreeMap<String, OutputField>,
    pub page: std::collections::BTreeMap<String, OutputField>,
    /// A hard output bound, never an instruction to fetch more pages.
    pub limit: usize,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OutputField {
    /// Empty selects the record itself; nonempty selectors are relative to the record.
    pub selector: String,
    pub attribute: String,
    pub required: bool,
    pub absolute_url: bool,
}
impl Extraction {
    pub fn valid(&self) -> bool {
        (!self.fields.is_empty() || !self.page.is_empty())
            && self.fields.len() + self.page.len() <= 40
            && self.limit <= 1000
            && self.items.len() <= 500
            && (self.items.is_empty() || Selector::parse(&self.items).is_ok())
            && self
                .fields
                .iter()
                .chain(self.page.iter())
                .all(|(name, field)| {
                    stable_name(name)
                        && !crate::util::secret_field(name)
                        && field.selector.len() <= 500
                        && field.attribute.len() <= 80
                        && (field.selector.is_empty() || Selector::parse(&field.selector).is_ok())
                        && !crate::util::secret_field(&field.attribute)
                })
    }
    pub fn extract(&self, html: &str, base: &str) -> Option<Value> {
        if !self.valid() {
            return None;
        }
        let document = Html::parse_document(html);
        let base = url::Url::parse(base).ok()?;
        let mut output = extract_fields(document.root_element(), &self.page, &base)?;
        if !self.fields.is_empty() {
            let records = if self.items.is_empty() {
                vec![document.root_element()]
            } else {
                let matched: Vec<_> = document
                    .select(&Selector::parse(&self.items).ok()?)
                    .take(if self.limit == 0 { 100 } else { self.limit })
                    .collect();
                if matched.is_empty() {
                    return None;
                }
                matched
            };
            let items: Option<Vec<_>> = records
                .into_iter()
                .map(|record| extract_fields(record, &self.fields, &base).map(Value::Object))
                .collect();
            output.insert("items".into(), Value::Array(items?));
        }
        Some(Value::Object(output))
    }
}
fn extract_fields(
    root: scraper::ElementRef<'_>,
    fields: &std::collections::BTreeMap<String, OutputField>,
    base: &url::Url,
) -> Option<serde_json::Map<String, Value>> {
    let mut output = serde_json::Map::new();
    for (name, field) in fields {
        let element = if field.selector.is_empty() {
            Some(root)
        } else {
            let selector = Selector::parse(&field.selector).ok()?;
            let mut matches = root.select(&selector);
            let first = matches.next();
            if matches.next().is_some() {
                None
            } else {
                first
            }
        };
        let value = element
            .and_then(|element| {
                if field.attribute.is_empty() {
                    Some(element_text(element))
                } else {
                    element.value().attr(&field.attribute).map(str::to_owned)
                }
            })
            .filter(|value| !value.is_empty() && value.len() <= 16_384);
        let value = if field.absolute_url {
            value
                .and_then(|value| base.join(&value).ok())
                .filter(|url| {
                    matches!(url.scheme(), "http" | "https")
                        && url.username().is_empty()
                        && url.password().is_none()
                })
                .map(|url| url.to_string())
        } else {
            value
        };
        if field.required && value.is_none() {
            return None;
        }
        output.insert(
            name.clone(),
            value.map(Value::String).unwrap_or(Value::Null),
        );
    }
    Some(output)
}

/// Bounded DOM skeleton for choosing selectors. Values from hidden inputs, inline scripts,
/// storage and data attributes are intentionally omitted; visible text is supplied separately.
pub fn structure(html: &str) -> Value {
    let document = Html::parse_document(html);
    let mut nodes = Vec::new();
    for element in document.select(&Selector::parse("*").unwrap()).take(600) {
        let tag = element.value().name();
        if matches!(tag, "script" | "style" | "meta" | "input" | "link") {
            continue;
        }
        let classes = element
            .value()
            .classes()
            .filter(|value| stable_name(value))
            .take(6)
            .collect::<Vec<_>>();
        let id = element.value().id().filter(|value| stable_name(value));
        let mut selector = tag.to_string();
        if let Some(id) = id {
            append_selector_value(&mut selector, "id", id);
        }
        for class in classes {
            append_selector_value(&mut selector, "class", class);
        }
        let count = Selector::parse(&selector)
            .ok()
            .map(|selector| document.select(&selector).count())
            .unwrap_or(0);
        let attributes = ["href", "src", "title", "alt", "datetime", "content"]
            .into_iter()
            .filter(|attribute| element.value().attr(attribute).is_some())
            .collect::<Vec<_>>();
        let parent = element
            .parent()
            .and_then(scraper::ElementRef::wrap)
            .map(|parent| {
                let mut name = parent.value().name().to_string();
                for class in parent
                    .value()
                    .classes()
                    .filter(|value| stable_name(value))
                    .take(3)
                {
                    append_selector_value(&mut name, "class", class);
                }
                name
            });
        let node =
            json!({"selector":selector,"matches":count,"attributes":attributes,"parent":parent});
        if !nodes.contains(&node) {
            nodes.push(node);
        }
        if nodes.len() >= 100 {
            break;
        }
    }
    json!(nodes)
}

fn stable_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_:.".contains(&b))
}

fn append_selector_value(selector: &mut String, attribute: &str, value: &str) {
    // Punctuation and a leading digit are valid HTML names but need CSS escaping.
    // stable_name excludes quotes, so an attribute selector safely preserves them.
    if value
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        selector.push(if attribute == "id" { '#' } else { '.' });
        selector.push_str(value);
    } else {
        let operator = if attribute == "class" { "~=" } else { "=" };
        selector.push_str(&format!("[{attribute}{operator}\"{value}\"]"));
    }
}

fn element_text(element: scraper::ElementRef<'_>) -> String {
    element
        .descendants()
        .filter_map(|node| {
            let text = node.value().as_text()?;
            let hidden = node
                .ancestors()
                .filter_map(scraper::ElementRef::wrap)
                .any(|parent| {
                    matches!(
                        parent.value().name(),
                        "script" | "style" | "noscript" | "template"
                    ) || parent.value().attr("hidden").is_some()
                        || parent.value().attr("aria-hidden") == Some("true")
                });
            (!hidden).then_some(text.as_ref())
        })
        .collect::<Vec<&str>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Describe selectable fields without exposing their values, including CSRF/session values.
/// The agent chooses the identity mapping; no website name or identity selector is built in.
pub fn fields(html: &str) -> Value {
    let document = Html::parse_document(html);
    let mut fields = Vec::new();
    let mut seen = BTreeSet::new();
    for element in document.select(&Selector::parse("*").unwrap()) {
        let tag = element.value().name();
        let mut candidates = Vec::new();
        if let Some(name) = element
            .value()
            .attr("name")
            .filter(|name| matches!(tag, "meta" | "input") && stable_name(name))
        {
            let lower = name.to_ascii_lowercase();
            if ![
                "token",
                "secret",
                "password",
                "csrf",
                "authenticity",
                "nonce",
            ]
            .iter()
            .any(|part| lower.contains(part))
                && element.value().attr("type") != Some("password")
            {
                candidates.push(Field {
                    selector: format!("{tag}[name=\"{name}\"]"),
                    attribute: if tag == "meta" { "content" } else { "value" }.into(),
                });
            }
        }
        for (name, _) in element.value().attrs().filter(|(name, _)| {
            name.starts_with("data-") && (name.contains("user") || name.contains("account"))
        }) {
            if stable_name(name) {
                candidates.push(Field {
                    selector: format!("{tag}[{name}]"),
                    attribute: name.into(),
                });
            }
        }
        for field in candidates {
            if seen.insert((field.selector.clone(), field.attribute.clone())) {
                fields.push(json!({"selector":field.selector,"attribute":field.attribute,"unique_nonempty":field.extract(&document).is_some()}));
            }
        }
        if fields.len() >= 100 {
            break;
        }
    }
    json!(fields)
}

fn collect_next_f_props(val: &Value, out: &mut serde_json::Map<String, Value>, depth: usize) {
    if depth > 24 {
        return;
    }
    match val {
        Value::Object(map) => {
            for (k, v) in map {
                if matches!(
                    k.as_str(),
                    "data"
                        | "posts"
                        | "items"
                        | "articles"
                        | "results"
                        | "initialData"
                        | "nodes"
                        | "edges"
                        | "list"
                        | "feed"
                        | "entries"
                        | "trending"
                        | "searchParams"
                        | "params"
                        | "buildId"
                ) {
                    let non_empty = match v {
                        Value::Null => false,
                        Value::Array(a) => !a.is_empty(),
                        Value::Object(o) => !o.is_empty(),
                        Value::String(s) => !s.is_empty(),
                        _ => true,
                    };
                    if non_empty {
                        out.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                } else if !matches!(
                    k.as_str(),
                    "children" | "fallback" | "parallelRoutes" | "styles" | "tree"
                ) && v
                    .as_array()
                    .is_some_and(|arr| !arr.is_empty() && arr.first().is_some_and(Value::is_object))
                {
                    out.entry(k.clone()).or_insert_with(|| v.clone());
                }
                collect_next_f_props(v, out, depth + 1);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_next_f_props(item, out, depth + 1);
            }
        }
        _ => {}
    }
}

/// Resolve a JSON Pointer (RFC 6901), with fallback matching for object keys that contain unescaped `/`.
pub fn resolve_pointer<'a>(value: &'a Value, pointer: &str) -> Option<&'a Value> {
    if pointer.is_empty() {
        return Some(value);
    }
    if let Some(found) = value.pointer(pointer) {
        return Some(found);
    }
    let rest = pointer.strip_prefix('/')?;
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if rest == k {
                    return Some(v);
                }
                if let Some(after) = rest
                    .strip_prefix(k.as_str())
                    .and_then(|s| s.strip_prefix('/'))
                {
                    if let Some(found) = resolve_pointer(v, &format!("/{after}")) {
                        return Some(found);
                    }
                }
            }
            None
        }
        Value::Array(arr) => {
            let (idx_str, after) = rest.split_once('/').unwrap_or((rest, ""));
            let idx: usize = idx_str.parse().ok()?;
            let elem = arr.get(idx)?;
            if after.is_empty() {
                Some(elem)
            } else {
                resolve_pointer(elem, &format!("/{after}"))
            }
        }
        _ => None,
    }
}

/// Parse embedded SSR JSON blocks (`__NEXT_DATA__`, `__NEXT_F__`, `__remixContext`, `__NUXT_DATA__`,
/// `application/ld+json`, `window.__INITIAL_STATE__`, `window.__STATE__`, `window.INIT_DATA`) from HTML without executing scripts.
pub fn extract_ssr_map(html: &str) -> std::collections::BTreeMap<String, Value> {
    let document = Html::parse_document(html);
    let mut payloads = std::collections::BTreeMap::new();
    let mut next_f_stream = String::new();
    let window_assign_re = regex::Regex::new(r"window\.([A-Za-z0-9_]+)\s*=\s*").expect("constant");

    if let Ok(sel) = Selector::parse("script") {
        for script in document.select(&sel).take(250) {
            let id = script.value().id().unwrap_or("");
            let script_key = if !id.is_empty() {
                id
            } else {
                script
                    .value()
                    .attr("data-target")
                    .or_else(|| script.value().attr("data-island"))
                    .or_else(|| script.value().attr("data-ssr"))
                    .unwrap_or("")
            };
            let r#type = script.value().attr("type").unwrap_or("");
            let raw: String = script.text().collect();
            let raw = raw.trim();
            if raw.is_empty() || raw.len() > 4 * 1024 * 1024 {
                continue;
            }
            if (!script_key.is_empty()
                && (script_key == "__NEXT_DATA__"
                    || script_key == "__NUXT_DATA__"
                    || r#type.contains("json")
                    || script_key.ends_with(".embeddedData")))
                || r#type.eq_ignore_ascii_case("application/ld+json")
            {
                if let Ok(val) = serde_json::from_str::<Value>(raw) {
                    if !script_key.is_empty() {
                        payloads.insert(script_key.to_owned(), val);
                    } else if let Some(existing) = payloads.get_mut("ld+json") {
                        if let (Value::Object(ex_map), Value::Object(new_map)) = (existing, &val) {
                            for (k, v) in new_map {
                                ex_map.entry(k.clone()).or_insert_with(|| v.clone());
                            }
                            let blocks = ex_map
                                .entry("blocks".to_owned())
                                .or_insert_with(|| Value::Array(vec![]));
                            if let Value::Array(arr) = blocks {
                                arr.push(val);
                            }
                        }
                    } else {
                        payloads.insert("ld+json".to_owned(), val);
                    }
                }
                continue;
            }
            for cap in window_assign_re.captures_iter(raw).take(20) {
                let ident = &cap[1];
                if ident != "INIT_DATA" && !ident.starts_with("__") {
                    continue;
                }
                let Some(m) = cap.get(0) else {
                    continue;
                };
                let rhs = raw[m.end()..].trim_start();
                if !rhs.starts_with('{') && !rhs.starts_with('[') {
                    continue;
                }
                let mut stream = serde_json::Deserializer::from_str(rhs).into_iter::<Value>();
                if let Some(Ok(mut val)) = stream.next() {
                    if ident == "__remixContext" {
                        if let Some(Value::Object(loader_map)) =
                            val.pointer("/state/loaderData").cloned()
                        {
                            let mut route_data = serde_json::Map::new();
                            for (rk, rv) in loader_map {
                                if rk != "root" {
                                    if let Value::Object(rmap) = rv {
                                        for (fk, fv) in rmap {
                                            route_data.entry(fk).or_insert(fv);
                                        }
                                    }
                                }
                            }
                            if route_data
                                .get("buySellArticles")
                                .is_none_or(|v| v.as_array().is_some_and(|a| a.is_empty()))
                            {
                                if let Some(results) = route_data
                                    .get("sections")
                                    .and_then(Value::as_array)
                                    .and_then(|secs| {
                                        secs.iter().find(|s| {
                                            s.get("type").and_then(Value::as_str)
                                                == Some("buy-sell")
                                        })
                                    })
                                    .and_then(|s| s.get("results"))
                                    .cloned()
                                {
                                    route_data.insert("buySellArticles".into(), results);
                                }
                            }
                            if !route_data.is_empty() {
                                if let Value::Object(top) = &mut val {
                                    top.insert("routeData".into(), Value::Object(route_data));
                                }
                            }
                        }
                    } else if ident == "__APOLLO_STATE__" {
                        if let Value::Object(top) = &mut val {
                            let mut items = Vec::new();
                            let mut remove_keys = Vec::new();
                            for (k, v) in top.iter() {
                                if k.contains(':') && !k.starts_with('$') {
                                    if let Value::Object(obj) = v {
                                        if obj.contains_key("title") || obj.contains_key("url_slug")
                                        {
                                            items.push(v.clone());
                                        }
                                        remove_keys.push(k.clone());
                                    }
                                }
                            }
                            for k in remove_keys {
                                top.remove(&k);
                            }
                            if !items.is_empty() {
                                top.insert("items".into(), Value::Array(items));
                            }
                        }
                    }
                    payloads.insert(ident.to_owned(), val);
                }
            }
            let mut search_from = 0;
            while let Some(pos) = raw[search_from..].find("__next_f.push(") {
                let start = search_from + pos + "__next_f.push(".len();
                let rhs = raw[start..].trim_start();
                if rhs.starts_with('[') {
                    let mut stream = serde_json::Deserializer::from_str(rhs).into_iter::<Value>();
                    if let Some(Ok(Value::Array(arr))) = stream.next() {
                        if let Some(Value::String(chunk)) = arr.get(1) {
                            next_f_stream.push_str(chunk);
                        }
                    }
                }
                search_from = start;
            }
        }
    }
    if !next_f_stream.is_empty() {
        let mut next_f_obj = serde_json::Map::new();
        for line in next_f_stream.lines().take(500) {
            let Some((_, rhs)) = line.split_once(':') else {
                continue;
            };
            let rhs = rhs.trim();
            if rhs.starts_with('[') || rhs.starts_with('{') {
                if let Ok(parsed) = serde_json::from_str::<Value>(rhs) {
                    collect_next_f_props(&parsed, &mut next_f_obj, 0);
                }
            }
        }
        if !next_f_obj.is_empty() {
            payloads.insert("__NEXT_F__".to_owned(), Value::Object(next_f_obj));
        }
    }
    payloads
}

/// Extract a specific SSR JSON payload (or `"auto"` for the first available) and optional JSON pointer.
pub fn extract_ssr_json(html: &str, source: &str, pointer: &str) -> Option<Value> {
    let map = extract_ssr_map(html);
    if map.is_empty() {
        return None;
    }
    let root = if source.is_empty() || source == "auto" {
        map.get("__NEXT_DATA__")
            .or_else(|| map.get("__remixContext"))
            .or_else(|| map.get("__NEXT_F__"))
            .or_else(|| map.get("__APOLLO_STATE__"))
            .or_else(|| map.get("__NUXT_DATA__"))
            .or_else(|| map.get("__INITIAL_STATE__"))
            .or_else(|| map.get("__PRELOADED_STATE__"))
            .or_else(|| map.get("__STATE__"))
            .or_else(|| map.get("INIT_DATA"))
            .or_else(|| map.values().next())?
    } else {
        map.get(source).or_else(|| {
            if source == "ld+json" {
                None
            } else {
                map.get("__NEXT_DATA__")
                    .or_else(|| map.get("__remixContext"))
                    .or_else(|| map.get("__NEXT_F__"))
                    .or_else(|| map.get("__APOLLO_STATE__"))
                    .or_else(|| map.get("__NUXT_DATA__"))
                    .or_else(|| map.get("__INITIAL_STATE__"))
            }
        })?
    };
    resolve_pointer(root, pointer).cloned()
}

/// Deterministically infer a repeating DOM record `Extraction` when an HTML page contains >= 3 structured item rows.
pub fn infer_html_extraction(raw: &str, base: &str) -> Option<Extraction> {
    let document = Html::parse_document(raw);
    let _base_url = url::Url::parse(base).ok()?;
    let mut candidates = vec![
        "div.topic_row".to_string(),
        "tr.athing".to_string(),
        "article".to_string(),
        "li.story".to_string(),
        "div.post-item".to_string(),
        "div.search-result".to_string(),
        "div.result".to_string(),
        "div.item".to_string(),
        "li.item".to_string(),
        "li.post".to_string(),
    ];
    if let Ok(scan_sel) = Selector::parse("main *[class], body *[class]") {
        for el in document.select(&scan_sel).take(600) {
            let tag = el.value().name();
            if !matches!(tag, "div" | "li" | "article" | "tr") {
                continue;
            }
            if let Some(cls) = el.value().classes().find(|c| {
                !c.is_empty()
                    && c.len() <= 40
                    && c.chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
                    && [
                        "row", "item", "topic", "post", "card", "story", "result", "entry",
                        "article", "thread",
                    ]
                    .iter()
                    .any(|kw| c.to_ascii_lowercase().contains(kw))
            }) {
                let cand = format!("{tag}.{cls}");
                if !candidates.contains(&cand) {
                    candidates.push(cand);
                }
            }
        }
    }

    let pick_field = |records: &[scraper::ElementRef<'_>],
                      opts: &[(&str, &str)],
                      abs_url: bool|
     -> Option<OutputField> {
        let min_hits = (records.len() * 4).div_ceil(5).max(2);
        for &(sel_str, attr_str) in opts {
            let parsed_sel = if sel_str.is_empty() {
                None
            } else {
                let Ok(s) = Selector::parse(sel_str) else {
                    continue;
                };
                Some(s)
            };
            let mut hits = 0usize;
            for rec in records {
                let target = if let Some(ref s) = parsed_sel {
                    let mut m = rec.select(s);
                    let first = m.next();
                    if m.next().is_some() { None } else { first }
                } else {
                    Some(*rec)
                };
                let val = target
                    .and_then(|el| {
                        if attr_str.is_empty() {
                            Some(element_text(el))
                        } else {
                            el.value().attr(attr_str).map(str::to_owned)
                        }
                    })
                    .filter(|v| !v.trim().is_empty() && v.len() <= 16_384);
                if val.is_some() {
                    hits += 1;
                }
            }
            if hits >= min_hits {
                return Some(OutputField {
                    selector: sel_str.to_string(),
                    attribute: attr_str.to_string(),
                    required: false,
                    absolute_url: abs_url,
                });
            }
        }
        None
    };

    for items_sel in candidates {
        let Ok(sel) = Selector::parse(&items_sel) else {
            continue;
        };
        let records: Vec<_> = document.select(&sel).take(40).collect();
        if records.len() < 3 {
            continue;
        }
        let mut fields = std::collections::BTreeMap::new();
        if let Some(f) = pick_field(
            &records,
            &[("", "data-topic-state-id"), ("", "data-id"), ("", "id")],
            false,
        ) {
            fields.insert("id".into(), f);
        }
        if let Some(f) = pick_field(
            &records,
            &[
                ("h1", ""),
                ("h2", ""),
                ("h3", ""),
                ("h4", ""),
                ("a.topic-title-link", ""),
                (".topictitle a", ""),
                (".titleline > a", ""),
                (".title a", ""),
                ("a[class*='title']", ""),
                ("a", ""),
            ],
            false,
        ) {
            fields.insert("title".into(), f);
        }
        if let Some(f) = pick_field(
            &records,
            &[
                ("a.topic-title-link", "href"),
                (".topictitle a", "href"),
                (".titleline > a", "href"),
                ("h1 a", "href"),
                ("h2 a", "href"),
                ("h3 a", "href"),
                (".title a", "href"),
                ("a[class*='title']", "href"),
                ("a[href]", "href"),
            ],
            true,
        ) {
            fields.insert("url".into(), f);
        }
        if !fields.contains_key("title") || !fields.contains_key("url") {
            continue;
        }
        if let Some(f) = pick_field(
            &records,
            &[
                (".topicdesc", ""),
                (".summary", ""),
                (".description", ""),
                (".excerpt", ""),
                (".snippet", ""),
                ("p", ""),
            ],
            false,
        ) {
            fields.insert("summary".into(), f);
        }
        if let Some(f) = pick_field(
            &records,
            &[
                (".topicinfo", ""),
                (".subtext", ""),
                (".meta", ""),
                (".byline", ""),
            ],
            false,
        ) {
            fields.insert("meta".into(), f);
        }
        if let Some(f) = pick_field(
            &records,
            &[("time[datetime]", "datetime"), ("time", "")],
            false,
        ) {
            fields.insert("published_at".into(), f);
        }
        let extraction = Extraction {
            items: items_sel,
            fields,
            page: std::collections::BTreeMap::new(),
            limit: 50,
        };
        if extraction.valid() && extraction.extract(raw, base).is_some() {
            return Some(extraction);
        }
    }
    None
}

/// Extract same-origin category/feed navigation paths from `<nav>` for server-rendered list pages.
pub fn nav_list_routes(raw: &str, base: &str) -> Vec<String> {
    let document = Html::parse_document(raw);
    let Ok(base_url) = url::Url::parse(base) else {
        return Vec::new();
    };
    let Ok(sel) = Selector::parse("header nav a[href], nav a[href]") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for el in document.select(&sel).take(40) {
        let Some(href) = el.value().attr("href") else {
            continue;
        };
        let Ok(u) = base_url.join(href) else {
            continue;
        };
        if u.origin() != base_url.origin() || u.query().is_some() {
            continue;
        }
        let p = u.path();
        if p == "/"
            || p.len() > 36
            || !p
                .trim_matches('/')
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            continue;
        }
        let lower = p.to_ascii_lowercase();
        if [
            "/login",
            "/signin",
            "/signup",
            "/register",
            "/logout",
            "/write",
            "/submit",
            "/search",
            "/settings",
            "/about",
            "/faq",
            "/terms",
            "/privacy",
            "/support",
            "/sponsor",
            "/guidelines",
            "/start",
            "/bookmarklet",
            "/geekbots",
            "/geekbadge",
            "/lists",
            "/threads",
            "/comments",
            "/past",
            "/weekly",
            "/articles",
        ]
        .contains(&lower.as_str())
        {
            continue;
        }
        if !out.contains(&p.to_string()) {
            out.push(p.to_string());
            if out.len() >= 6 {
                break;
            }
        }
    }
    out
}

/// Summarize SSR JSON payload shapes and CSRF/token selector metadata without exposing secret values.
pub fn ssr_payloads(html: &str) -> Value {
    let map = extract_ssr_map(html);
    let mut shapes = serde_json::Map::new();
    for (k, v) in map {
        shapes.insert(k, crate::session_recipe::shape(&v, 0));
    }
    let document = Html::parse_document(html);
    let mut csrf_selectors = Vec::new();
    if let Ok(sel) = Selector::parse("meta[name], input[type=\"hidden\"][name]") {
        for el in document.select(&sel).take(40) {
            let tag = el.value().name();
            if let Some(name) = el.value().attr("name").filter(|n| stable_name(n)) {
                let lower = name.to_ascii_lowercase();
                if ["csrf", "xsrf", "authenticity", "token", "nonce"]
                    .iter()
                    .any(|p| lower.contains(p))
                {
                    csrf_selectors.push(format!("{tag}[name=\"{name}\"]"));
                }
            }
        }
    }
    json!({
        "payloads": shapes,
        "csrf_selectors": csrf_selectors,
    })
}

/// Structural form descriptors (action, method, and named input/select/textarea controls) without field values.
pub fn forms(html: &str) -> Value {
    let document = Html::parse_document(html);
    let mut out = Vec::new();
    let Ok(form_sel) = Selector::parse("form") else {
        return json!(out);
    };
    let control_sel = Selector::parse("input[name], select[name], textarea[name]").unwrap();
    for form in document.select(&form_sel).take(40) {
        let action = form.value().attr("action").unwrap_or("").trim();
        let method = form
            .value()
            .attr("method")
            .unwrap_or("GET")
            .trim()
            .to_ascii_uppercase();
        let mut inputs = Vec::new();
        for ctrl in form.select(&control_sel).take(60) {
            let Some(name) = ctrl.value().attr("name").filter(|n| stable_name(n)) else {
                continue;
            };
            let tag = ctrl.value().name();
            let kind =
                ctrl.value()
                    .attr("type")
                    .unwrap_or(if tag == "input" { "text" } else { tag });
            let required = ctrl.value().attr("required").is_some();
            inputs.push(json!({
                "name": name,
                "type": kind,
                "required": required,
            }));
        }
        out.push(json!({
            "action": action,
            "method": method,
            "inputs": inputs,
        }));
    }
    json!(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extraction_refuses_missing_or_ambiguous_required_fields() {
        let extraction: Extraction = serde_json::from_value(json!({"items":"article","fields":{"title":{"selector":"h2","required":true},"url":{"selector":"a","attribute":"href","absolute_url":true}}})).unwrap();
        assert!(
            extraction
                .extract(
                    "<article><h2>One</h2><h2>Two</h2></article>",
                    "https://example.test/list"
                )
                .is_none()
        );
        let value = extraction
            .extract(
                "<article><h2>A\n title</h2><a href='javascript:alert(1)'>Link</a></article>",
                "https://example.test/list",
            )
            .unwrap();
        assert_eq!(value["items"][0]["title"], "A title");
        assert!(value["items"][0]["url"].is_null());
        let evidence = structure(
            "<article class='book'><a href='/item'>Read</a><input type='hidden' value='private-value'><script>secret_token='private-value'</script></article>",
        );
        assert!(evidence.to_string().contains("article.book"));
        assert!(!evidence.to_string().contains("private-value"));
        let evidence =
            structure("<article id='12:item' class='sm:wide book.item'><h2>Title</h2></article>");
        for node in evidence.as_array().unwrap() {
            assert!(
                node["matches"].as_u64().unwrap() > 0,
                "{}",
                node["selector"]
            );
        }
        let value = extraction.extract("<article><h2>A\n title<script>private-value</script><span hidden>hidden-value</span><style>private-style</style></h2></article>", "https://example.test/list").unwrap();
        assert_eq!(value["items"][0]["title"], "A title");
        let identity = Field {
            selector: "h2".into(),
            attribute: String::new(),
        };
        assert_eq!(
            identity.extract(&Html::parse_document("<h2>Mina\n Park</h2>")),
            Some("Mina Park".into())
        );
        let long_page = Html::parse_document(&format!("<h2>{}</h2>", "Book title ".repeat(100)));
        assert!(identity.extract(&long_page).is_none());
        assert!(identity.is_present(&long_page));
        assert!(!identity.is_present(&Html::parse_document("<h2>One</h2><h2>Two</h2>")));
    }
    #[test]
    fn fields_keep_values_private_and_refuse_ambiguous_identity() {
        let html = r#"<meta name="current-person" content="person-private"><meta name="csrf-token" content="secret-private"><input name="email" value="private@example.test"><span data-account-id="17"></span>"#;
        let evidence = fields(html).to_string();
        assert!(evidence.contains("current-person"));
        assert!(evidence.contains("data-account-id"));
        for value in [
            "person-private",
            "private@example.test",
            "secret-private",
            "csrf-token",
        ] {
            assert!(!evidence.contains(value));
        }
        let field = Field {
            selector: "meta[name=\"current-person\"]".into(),
            attribute: "content".into(),
        };
        assert_eq!(
            field.extract(&Html::parse_document(html)).as_deref(),
            Some("person-private")
        );
        assert!(
            field
                .extract(&Html::parse_document(&format!("{html}{html}")))
                .is_none()
        );
        assert!(
            field
                .extract(&Html::parse_document(
                    "<meta name='current-person' content=''>"
                ))
                .is_none()
        );
    }
    #[test]
    fn ssr_extraction_supports_remix_context_nextjs_flight_and_merged_ld_json() {
        let html = r#"
            <script type="application/ld+json">{"@context":"https://schema.org","@type":"WebSite","potentialAction":{"target":{"urlTemplate":"https://example.com/search?q={q}"}}}</script>
            <script type="application/ld+json">{"@context":"https://schema.org","@type":"ItemList","itemListElement":[{"name":"MacBook Pro"}]}</script>
            <script>window.__remixContext = {"state":{"loaderData":{"root":{"lang":"ko"},"routes/kr.search.buy-sell._index":{"buySellArticles":[{"title":"M3 Max; 16인치"}]}}}}; console.log("done");</script>
            <script>self.__next_f.push([1,"6:[\"$\",\"$L10\",null,{\"children\":[\"$\",\"$L13\",null,{\"data\":[{\"id\":\"p1\",\"title\":\"Hello Velog\"}]}]}]\n"])</script>
        "#;
        let map = extract_ssr_map(html);
        assert!(map.contains_key("__remixContext"));
        assert!(map.contains_key("__NEXT_F__"));
        assert!(map.contains_key("ld+json"));
        assert_eq!(
            extract_ssr_json(html, "__remixContext", "/routeData/buySellArticles/0/title"),
            Some(json!("M3 Max; 16인치"))
        );
        assert_eq!(
            extract_ssr_json(
                html,
                "__remixContext",
                "/state/loaderData/routes/kr.search.buy-sell._index/buySellArticles/0/title"
            ),
            Some(json!("M3 Max; 16인치"))
        );
        assert_eq!(
            extract_ssr_json(html, "__NEXT_F__", "/data/0/title"),
            Some(json!("Hello Velog"))
        );
        assert_eq!(
            extract_ssr_json(html, "ld+json", "/itemListElement/0/name"),
            Some(json!("MacBook Pro"))
        );
    }
    #[test]
    fn infer_html_extraction_and_apollo_state_work() {
        let html = r#"
            <header><nav><a href="/">Home</a><a href="/new">New</a><a href="/show">Show</a><a href="/login">Login</a></nav></header>
            <main>
                <div class='topic_row' data-topic-state-id='101'>
                    <div class='topictitle'><a href='https://example.org/a' class='topic-title-link'><h2>First Topic</h2></a></div>
                    <div class='topicdesc'><a href='topic?id=101'>Summary one</a></div>
                    <div class='topicinfo'><span>10 points</span><time datetime='2026-10-18T00:00:00Z'>1h</time></div>
                </div>
                <div class='topic_row' data-topic-state-id='102'>
                    <div class='topictitle'><a href='https://example.org/b' class='topic-title-link'><h2>Second Topic</h2></a></div>
                    <div class='topicdesc'><a href='topic?id=102'>Summary two</a></div>
                    <div class='topicinfo'><span>20 points</span><time datetime='2026-10-18T01:00:00Z'>2h</time></div>
                </div>
                <div class='topic_row' data-topic-state-id='103'>
                    <div class='topictitle'><a href='https://example.org/c' class='topic-title-link'><h2>Third Topic</h2></a></div>
                    <div class='topicdesc'><a href='topic?id=103'>Summary three</a></div>
                    <div class='topicinfo'><span>30 points</span><time datetime='2026-10-18T02:00:00Z'>3h</time></div>
                </div>
            </main>
            <script>window.__APOLLO_STATE__ = {"ROOT_QUERY":{"searchPosts":{"count":1}},"Post:abc-123":{"id":"abc-123","title":"Rust Guide","url_slug":"rust-guide"}};</script>
        "#;
        let ext = infer_html_extraction(html, "https://news.hada.io/").expect("inferred");
        assert_eq!(ext.items, "div.topic_row");
        let extracted = ext
            .extract(html, "https://news.hada.io/")
            .expect("extracted");
        assert_eq!(extracted["items"][0]["id"], "101");
        assert_eq!(extracted["items"][0]["title"], "First Topic");
        assert_eq!(extracted["items"][0]["url"], "https://example.org/a");
        assert_eq!(extracted["items"][0]["summary"], "Summary one");
        let nav = nav_list_routes(html, "https://news.hada.io/");
        assert_eq!(nav, vec!["/new".to_string(), "/show".to_string()]);
        assert_eq!(
            extract_ssr_json(html, "__ NEXT_F__", "/items/0/title"),
            Some(json!("Rust Guide"))
        );
    }
}
