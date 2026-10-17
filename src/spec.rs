//! SiteSpec v1 — 웹사이트를 CLI로 기술하는 선언형 포맷. 스펙은 데이터다.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::apperr::{self, AppResult};

pub const SPEC_VERSION: i64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    #[serde(default)]
    pub spec_version: i64,
    pub site: SiteInfo,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub auth: Vec<AuthStrategy>,
    #[serde(default)]
    pub operations: Vec<Operation>,
    #[serde(default)]
    pub models: std::collections::BTreeMap<String, Model>,
    #[serde(default)]
    pub health: Option<Health>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteInfo {
    pub name: String,
    #[serde(default)]
    pub title: String,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_url: String,
}
impl SiteInfo {
    pub fn source_url(&self) -> &str {
        if self.source_url.is_empty() {
            &self.base_url
        } else {
            &self.source_url
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub timeout: String,
    #[serde(default)]
    pub tls_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthStrategy {
    pub name: String,
    pub kind: String, // none | header | browser | derived
    #[serde(default)]
    pub header: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default, rename = "value_from")]
    pub value_from: String, // store:<site>/<key> | cookie:<name> | storage:<key>[#<pointer>]
    /// Exact credential destination; legacy definitions use the site's base origin.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub origin: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PreflightStep {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub op: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub method: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub base_url: String,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub args: std::collections::BTreeMap<String, String>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub extract: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Operation {
    pub name: String,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub method: String,
    pub path: String,
    /// Optional evidenced server for sites whose web UI and API use different origins.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub transport: String,
    #[serde(default)]
    pub auth: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preflight: Vec<PreflightStep>,
    #[serde(default)]
    pub params: std::collections::BTreeMap<String, Param>,
    #[serde(default)]
    pub query: std::collections::BTreeMap<String, Param>,
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<Body>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub effect: crate::policy::Effect,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<ResponseExpectation>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ResponseExpectation {
    pub format: String,
    pub required_pointers: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub required_html_fields: Vec<crate::html::Field>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<crate::html::Extraction>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub strip_prefix: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ssr_json: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub extract_pointer: String,
}

impl Operation {
    pub fn graphql_query(&self, args: &std::collections::BTreeMap<String, String>) -> bool {
        let query = args.get("query").map(String::as_str).or_else(|| {
            self.query
                .get("query")
                .or_else(|| {
                    self.body
                        .as_ref()
                        .and_then(|body| body.json.get("query").or_else(|| body.form.get("query")))
                })?
                .default
                .as_ref()?
                .as_str()
        });
        query.is_some_and(|query| graphql_parser::parse_query::<&str>(query).is_ok())
    }
    pub fn parse_json_response(&self, response: &crate::net::Response) -> Option<Value> {
        let expected = self.response.as_ref();
        if let Some(exp) = expected {
            if !exp.ssr_json.is_empty() {
                let html = String::from_utf8_lossy(&response.bytes);
                return crate::html::extract_ssr_json(&html, &exp.ssr_json, &exp.extract_pointer);
            }
        }
        let mut parsed = response.json().or_else(|| {
            let text = std::str::from_utf8(&response.bytes).ok()?.trim_start();
            let stripped = if let Some(exp) = expected {
                if !exp.strip_prefix.is_empty() {
                    text.strip_prefix(exp.strip_prefix.as_str())
                        .map(str::trim_start)
                } else {
                    None
                }
            } else {
                None
            }
            .or_else(|| {
                for prefix in [")]}',", ")]}'", "while(1);", "for(;;);"] {
                    if let Some(rest) = text.strip_prefix(prefix) {
                        return Some(rest.trim_start());
                    }
                }
                None
            })?;
            serde_json::from_str(stripped).ok()
        })?;
        if let Some(exp) = expected {
            if !exp.extract_pointer.is_empty() {
                parsed = parsed.pointer(&exp.extract_pointer)?.clone();
            }
        }
        Some(parsed)
    }
    pub fn matches_response(&self, response: &crate::net::Response) -> bool {
        if !response.successful() {
            return false;
        }
        let Some(expected) = &self.response else {
            return true;
        };
        match expected.format.as_str() {
            "json" => self.parse_json_response(response).is_some_and(|body| {
                expected
                    .required_pointers
                    .iter()
                    .all(|p| body.pointer(p).is_some())
            }),
            "html" => {
                if !expected.ssr_json.is_empty() {
                    return response.header("content-type").contains("text/html")
                        && self.parse_json_response(response).is_some_and(|body| {
                            expected
                                .required_pointers
                                .iter()
                                .all(|p| body.pointer(p).is_some())
                        });
                }
                !expected.required_html_fields.is_empty()
                    && response.header("content-type").contains("text/html")
                    && !response.bytes.is_empty()
                    && {
                        let document = scraper::Html::parse_document(&String::from_utf8_lossy(
                            &response.bytes,
                        ));
                        expected
                            .required_html_fields
                            .iter()
                            .all(|field| field.is_present(&document))
                            && expected.html.as_ref().is_none_or(|extraction| {
                                extraction
                                    .extract(
                                        &String::from_utf8_lossy(&response.bytes),
                                        &response.url,
                                    )
                                    .is_some()
                            })
                    }
            }
            "text" => {
                response.header("content-type").starts_with("text/") && !response.bytes.is_empty()
            }
            "xml" => response.header("content-type").contains("xml") && !response.bytes.is_empty(),
            "empty" => response.bytes.is_empty(),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    #[serde(default = "default_param_type", rename = "type")]
    pub kind: String, // string|int|float|bool
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
}

fn default_param_type() -> String {
    "string".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    #[serde(default, rename = "json")]
    pub json: std::collections::BTreeMap<String, Param>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_template: Option<Value>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub form: std::collections::BTreeMap<String, Param>,
}
impl Body {
    pub fn inputs(&self) -> impl Iterator<Item = (&String, &Param)> {
        self.json.iter().chain(self.form.iter())
    }
    pub fn content_type(&self) -> &'static str {
        if self.form.is_empty() {
            "application/json"
        } else {
            "application/x-www-form-urlencoded"
        }
    }
}

pub type Model = std::collections::BTreeMap<String, Field>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    #[serde(default)]
    pub path: String,
    #[serde(default, rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub op: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub field: String,
    pub detail: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub remedy: String,
}

pub fn parse(data: &[u8]) -> AppResult<Spec> {
    let mut s: Spec = serde_yaml::from_slice(data)
        .map_err(|e| apperr::spec(format!("스펙 YAML 파싱 실패 ({e})"), "yaml 문법 확인"))?;
    if s.spec_version == 0 {
        s.spec_version = SPEC_VERSION;
    }
    if s.defaults.timeout.is_empty() {
        s.defaults.timeout = "15s".into();
    }
    if s.defaults.tls_profile.is_empty() {
        s.defaults.tls_profile = "go".into();
    }
    if s.auth.is_empty() {
        s.auth.push(AuthStrategy {
            name: "public".into(),
            kind: "none".into(),
            header: String::new(),
            prefix: String::new(),
            value_from: String::new(),
            origin: String::new(),
        });
    }
    Ok(s)
}

impl Spec {
    pub fn operation_base<'a>(&'a self, op: &'a Operation) -> &'a str {
        if op.base_url.is_empty() {
            &self.site.base_url
        } else {
            &op.base_url
        }
    }

    pub fn validate(&self) -> Vec<Issue> {
        let mut out: Vec<Issue> = Vec::new();
        {
            fn add(out: &mut Vec<Issue>, field: &str, detail: String, remedy: &str) {
                out.push(Issue {
                    field: field.into(),
                    detail,
                    remedy: remedy.into(),
                });
            }

            if self.spec_version != SPEC_VERSION && self.spec_version != 2 {
                add(
                    &mut out,
                    "spec_version",
                    format!("지원: 1|2, 실제: {}", self.spec_version),
                    "spec_version: 1 또는 2",
                );
            }
            if self.site.name.is_empty() {
                add(
                    &mut out,
                    "site.name",
                    "비어 있다".into(),
                    "사이트 식별자 필수",
                );
            }
            if self.site.base_url.is_empty() {
                add(
                    &mut out,
                    "site.base_url",
                    "비어 있다".into(),
                    "https:// 절대 URL",
                );
            } else if !self.site.base_url.starts_with("http") {
                add(&mut out, "site.base_url", "http(s) 접두사 없음".into(), "");
            }

            let mut auths = std::collections::HashSet::new();
            for a in &self.auth {
                if !auths.insert(a.name.clone()) {
                    add(
                        &mut out,
                        &format!("auth.{}", a.name),
                        "이름 중복".into(),
                        "",
                    );
                }
                match a.kind.as_str() {
                    "none" | "browser" => {}
                    "header" => {
                        if a.header.is_empty() {
                            add(
                                &mut out,
                                &format!("auth.{}.header", a.name),
                                "kind=header인데 헤더명 없음".into(),
                                "",
                            );
                        }
                        if !a.value_from.starts_with("store:") {
                            add(
                                &mut out,
                                &format!("auth.{}.value_from", a.name),
                                "store:<site>/<key> 참조 필요".into(),
                                "시크릿은 스펙에 넣지 않는다",
                            );
                        }
                    }
                    "derived" => {
                        if a.header.is_empty() {
                            add(
                                &mut out,
                                &format!("auth.{}.header", a.name),
                                "kind=derived인데 헤더명 없음".into(),
                                "",
                            );
                        }
                        if !["cookie:", "storage:", "header:"]
                            .iter()
                            .any(|p| a.value_from.starts_with(p))
                        {
                            add(
                                &mut out,
                                &format!("auth.{}.value_from", a.name),
                                "cookie:<name> 또는 storage:<key>[#<pointer>] 참조 필요".into(),
                                "예: cookie:note_gql_auth_token",
                            );
                        }
                    }
                    other => add(
                        &mut out,
                        &format!("auth.{}.kind", a.name),
                        format!("미지원 kind: {other}"),
                        "none|header|browser|derived",
                    ),
                }
            }
            let models: std::collections::HashSet<&String> = self.models.keys().collect();
            let op_names: std::collections::HashSet<&String> =
                self.operations.iter().map(|o| &o.name).collect();

            for op in &self.operations {
                let id = format!("operations.{}", op.name);
                if op.method.is_empty() {
                    add(
                        &mut out,
                        &format!("{id}.method"),
                        "비어 있다".into(),
                        "GET|POST|PUT|PATCH|DELETE",
                    );
                }
                if !op.path.starts_with('/') {
                    add(
                        &mut out,
                        &format!("{id}.path"),
                        format!("/로 시작해야 한다: {}", op.path),
                        "",
                    );
                }
                if !op.auth.is_empty() && !auths.contains(&op.auth) {
                    add(
                        &mut out,
                        &format!("{id}.auth"),
                        format!("미정의 auth: {}", op.auth),
                        "auth 블록에 정의",
                    );
                }
                if !op.model.is_empty() && !models.contains(&op.model) {
                    add(
                        &mut out,
                        &format!("{id}.model"),
                        format!("미정의 model: {}", op.model),
                        "models 블록에 정의",
                    );
                }
                for step in &op.preflight {
                    let valid_target = (!step.op.is_empty() && op_names.contains(&step.op))
                        || (!step.path.is_empty() && step.path.starts_with('/'));
                    if step.name.is_empty() || !valid_target {
                        add(
                            &mut out,
                            &format!("{id}.preflight"),
                            format!("유효하지 않은 preflight step: {}", step.name),
                            "정의된 operation 참조 또는 '/'로 시작하는 path 지정 필요",
                        );
                    }
                }
                let path_names = path_vars(&op.path);
                for n in &path_names {
                    if !op.params.contains_key(n) {
                        add(
                            &mut out,
                            &format!("{id}.params"),
                            format!("경로 변수 {{{n}}} 미선언"),
                            "params에 선언",
                        );
                    }
                }
                for n in op.params.keys() {
                    if !path_names.contains(n) {
                        add(
                            &mut out,
                            &format!("{id}.params.{n}"),
                            "경로 템플릿에 없는 파라미터".into(),
                            &format!("path에 {{{n}}} 사용 또는 query로 이동"),
                        );
                    }
                }
                let mut counts: std::collections::BTreeMap<&String, usize> = Default::default();
                for k in op.params.keys() {
                    *counts.entry(k).or_default() += 1;
                }
                for k in op.query.keys() {
                    *counts.entry(k).or_default() += 1;
                }
                if let Some(b) = &op.body {
                    if (!b.json.is_empty() || b.json_template.is_some()) && !b.form.is_empty() {
                        add(
                            &mut out,
                            &format!("{id}.body"),
                            "Choose one body encoding".into(),
                            "Use body.json/json_template or body.form, not both.",
                        );
                    }
                    for (k, _) in b.inputs() {
                        *counts.entry(k).or_default() += 1;
                    }
                }
                for (n, c) in &counts {
                    if *c > 1 {
                        add(
                            &mut out,
                            id.as_str(),
                            format!("인자 이름 충돌: {n}"),
                            "params/query/body에서 고유하게",
                        );
                    }
                }
                let check = |where_: &str,
                             ps: &std::collections::BTreeMap<String, Param>,
                             out: &mut Vec<Issue>| {
                    for (n, p) in ps {
                        match p.kind.as_str() {
                            "string" | "int" | "float" | "bool" | "json" => {}
                            other => out.push(Issue {
                                field: format!("{where_}.{n}.type"),
                                detail: format!("미지원 type: {other}"),
                                remedy: "string|int|float|bool|json".into(),
                            }),
                        }
                        if p.required && p.default.is_some() {
                            out.push(Issue {
                                field: format!("{where_}.{n}"),
                                detail: "required에 default 병기".into(),
                                remedy: "하나만".into(),
                            });
                        }
                    }
                };
                check(&format!("{id}.params"), &op.params, &mut out);
                check(&format!("{id}.query"), &op.query, &mut out);
                if let Some(b) = &op.body {
                    check(&format!("{id}.body.json"), &b.json, &mut out);
                    check(&format!("{id}.body.form"), &b.form, &mut out);
                }
            }
        }
        out
    }

    pub fn find_auth(&self, name: &str) -> Option<&AuthStrategy> {
        self.auth.iter().find(|a| a.name == name)
    }

    pub fn op(&self, name: &str) -> AppResult<&Operation> {
        self.operations
            .iter()
            .find(|o| o.name == name)
            .ok_or_else(|| {
                let names: Vec<&str> = self.operations.iter().map(|o| o.name.as_str()).collect();
                apperr::spec(
                    format!("연산 없음: {name}"),
                    format!("가능: {}", names.join(", ")),
                )
            })
    }

    /// 요청 URL·바디 구성. args는 플래그에서 모은 문자열 맵.
    pub fn build_request(
        &self,
        op: &Operation,
        args: &std::collections::BTreeMap<String, String>,
    ) -> AppResult<(String, Option<Vec<u8>>)> {
        let mut path = op.path.clone();
        for n in path_vars(&op.path) {
            let p = op
                .params
                .get(&n)
                .ok_or_else(|| apperr::spec(format!("경로 변수 {n} 미선언"), ""))?;
            let v = coerce_str(p, &n, args, false)?;
            path = path.replace(&format!("{{{n}}}"), &urlencode_path(&v));
        }
        let mut url_s = format!(
            "{}/{}",
            self.operation_base(op).trim_end_matches('/'),
            path.trim_start_matches('/')
        );

        let mut q = FormEncoder::default();
        for (name, p) in &op.query {
            let present = args.contains_key(name);
            if !present && p.default.is_none() {
                if p.required {
                    return Err(apperr::usage(
                        format!("Missing required input: {name}"),
                        "Provide the input shown by describe.",
                    ));
                }
                continue;
            }
            let v = coerce_str(p, name, args, true)?;
            q.append_pair(name, &v);
        }
        let qs = q.finish();
        if !qs.is_empty() {
            url_s.push('?');
            url_s.push_str(&qs);
        }

        let mut body = None;
        if let Some(b) = &op.body {
            if !b.form.is_empty() {
                let mut form = FormEncoder::default();
                for (name, p) in &b.form {
                    if !args.contains_key(name) && p.default.is_none() {
                        if p.required {
                            return Err(apperr::usage(
                                format!("Missing required input: {name}"),
                                "Use the action's --help for its inputs.",
                            ));
                        }
                        continue;
                    }
                    form.append_pair(name, &coerce_str(p, name, args, true)?);
                }
                body = Some(form.finish().into_bytes());
            } else if let Some(template) = &b.json_template {
                let mut resolved_values = std::collections::BTreeMap::new();
                for (name, p) in &b.json {
                    let present = args.contains_key(name);
                    if !present && p.default.is_none() {
                        if p.required {
                            return Err(apperr::spec(
                                format!("필수 인자 누락: --{name}"),
                                format!("예: --{name} <{}>", p.kind),
                            ));
                        }
                        continue;
                    }
                    resolved_values.insert(name.clone(), coerce_value(p, name, args)?);
                }
                let rendered = render_json_template(template, &resolved_values, args);
                body = Some(serde_json::to_vec(&rendered).unwrap_or_default());
            } else if !b.json.is_empty() {
                let mut obj = Map::new();
                for (name, p) in &b.json {
                    let present = args.contains_key(name);
                    if !present && p.default.is_none() {
                        if p.required {
                            return Err(apperr::spec(
                                format!("필수 인자 누락: --{name}"),
                                format!("예: --{name} <{}>", p.kind),
                            ));
                        }
                        continue;
                    }
                    obj.insert(name.clone(), coerce_value(p, name, args)?);
                }
                body = Some(serde_json::to_vec(&Value::Object(obj)).unwrap_or_default());
            }
        }
        Ok((url_s, body))
    }

    /// 점경로 추출+타입 강제로 원시 JSON을 모델 모양으로.
    pub fn apply_model(&self, name: &str, raw: &[u8]) -> AppResult<Value> {
        let model = match self.models.get(name) {
            Some(m) => m,
            None => return Ok(serde_json::from_slice(raw).unwrap_or(Value::Null)),
        };
        let doc: Value = serde_json::from_slice(raw)
            .map_err(|e| apperr::spec(format!("모델 적용 중 JSON 파싱 실패 ({e})"), ""))?;
        let mut out = Map::new();
        for (field, f) in model {
            let path = if f.path.is_empty() {
                field.clone()
            } else {
                f.path.clone()
            };
            if let Some(v) = walk(&doc, path.split('.')) {
                out.insert(field.clone(), convert_type(v, &f.kind));
            }
        }
        Ok(Value::Object(out))
    }
}

fn walk<'a>(doc: &'a Value, path: impl Iterator<Item = &'a str>) -> Option<&'a Value> {
    let mut cur = doc;
    for seg in path {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

fn convert_type(v: &Value, t: &str) -> Value {
    match t {
        "string" => Value::String(match v {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            other => other.to_string(),
        }),
        "int" => v.as_i64().map(Value::from).unwrap_or(Value::Null),
        "float" => v.as_f64().map(Value::from).unwrap_or(Value::Null),
        _ => v.clone(),
    }
}

fn path_vars(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_var = false;
    for c in path.chars() {
        match c {
            '{' => {
                in_var = true;
                cur.clear();
            }
            '}' => {
                in_var = false;
                out.push(cur.clone());
            }
            _ if in_var => cur.push(c),
            _ => {}
        }
    }
    out
}

fn coerce_str(
    p: &Param,
    name: &str,
    args: &std::collections::BTreeMap<String, String>,
    allow_default: bool,
) -> AppResult<String> {
    match args.get(name) {
        Some(v) => {
            check_type(p, name, v)?;
            Ok(v.clone())
        }
        None => {
            if let Some(d) = &p.default {
                if allow_default {
                    return Ok(value_to_plain_string(d));
                }
            }
            if p.required {
                return Err(apperr::spec(
                    format!("필수 인자 누락: --{name}"),
                    format!("예: --{name} <{}>", p.kind),
                ));
            }
            Ok(String::new())
        }
    }
}

fn coerce_value(
    p: &Param,
    name: &str,
    args: &std::collections::BTreeMap<String, String>,
) -> AppResult<Value> {
    match args.get(name) {
        Some(v) => {
            check_type(p, name, v)?;
            Ok(match p.kind.as_str() {
                "int" => v
                    .parse::<i64>()
                    .map(Value::from)
                    .map_err(|e| apperr::spec(format!("--{name} 파싱 실패: {e}"), ""))?,
                "float" => v
                    .parse::<f64>()
                    .map(Value::from)
                    .map_err(|e| apperr::spec(format!("--{name} 파싱 실패: {e}"), ""))?,
                "bool" => v
                    .parse::<bool>()
                    .map(Value::from)
                    .map_err(|e| apperr::spec(format!("--{name} 파싱 실패: {e}"), ""))?,
                "json" => serde_json::from_str(v).map_err(|_| {
                    apperr::usage(
                        format!("Invalid structured input: {name}"),
                        "Provide valid JSON.",
                    )
                })?,
                _ => Value::String(v.clone()),
            })
        }
        None => p
            .default
            .clone()
            .ok_or_else(|| apperr::spec(format!("필수 인자 누락: --{name}"), "")),
    }
}

fn check_type(p: &Param, name: &str, v: &str) -> AppResult<()> {
    let ok = match p.kind.as_str() {
        "int" => v.parse::<i64>().is_ok(),
        "float" => v.parse::<f64>().is_ok_and(|n| n.is_finite()),
        "bool" => v.parse::<bool>().is_ok(),
        "json" => serde_json::from_str::<Value>(v).is_ok(),
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(apperr::spec(
            format!("--{name}은(는) {}여야 한다: {v}", p.kind),
            "",
        ))
    }
}

fn value_to_plain_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn urlencode_path(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// url 크레이트 의존 없는 최소 form encode.
#[derive(Default)]
pub struct FormEncoder(String);

impl FormEncoder {
    pub fn append_pair(&mut self, k: &str, v: &str) {
        if !self.0.is_empty() {
            self.0.push('&');
        }
        self.0.push_str(&form_encode(k));
        self.0.push('=');
        self.0.push_str(&form_encode(v));
    }
    pub fn finish(self) -> String {
        self.0
    }
}

fn form_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'*' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn render_json_template(
    template: &Value,
    resolved_values: &std::collections::BTreeMap<String, Value>,
    raw_args: &std::collections::BTreeMap<String, String>,
) -> Value {
    match template {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), render_json_template(v, resolved_values, raw_args)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| render_json_template(v, resolved_values, raw_args))
                .collect(),
        ),
        Value::String(s) => {
            if let Some(var) = s.strip_prefix("{{").and_then(|rest| rest.strip_suffix("}}")) {
                let var = var.trim();
                if !var.contains("{{") {
                    if let Some(val) = resolved_values.get(var) {
                        return val.clone();
                    }
                    if let Some(raw) = raw_args.get(var) {
                        return Value::String(raw.clone());
                    }
                }
            }
            Value::String(interpolate_template_str(s, resolved_values, raw_args))
        }
        other => other.clone(),
    }
}

pub fn interpolate_template_str(
    template: &str,
    resolved_values: &std::collections::BTreeMap<String, Value>,
    raw_args: &std::collections::BTreeMap<String, String>,
) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        if let Some(end) = after_start.find("}}") {
            let var = after_start[..end].trim();
            if let Some(raw) = raw_args.get(var) {
                out.push_str(raw);
            } else if let Some(val) = resolved_values.get(var) {
                out.push_str(&value_to_plain_string(val));
            }
            rest = &after_start[end + 2..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

