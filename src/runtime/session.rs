//! Bounded session diagnostics shared by command surfaces; response values stay local.
use super::Runtime;
use crate::{
    accounts::{AccountView, Observation},
    apperr::{AppError, AppResult},
    session_recipe::ConnectionRecipe,
};
use serde_json::{Value, json};

impl Runtime {
    pub(crate) fn inspect_account_session(&self, id: &str) -> AppResult<Value> {
        let account = self.session_account(id)?;
        self.session_output(
            id,
            self.inspect_session_key(id, &super::origin(&account.site_url)?, "", 0)?,
        )
    }

    pub(crate) async fn observe_account_session(
        &self,
        id: &str,
        url: &str,
        browser: bool,
    ) -> AppResult<Value> {
        let account = self.session_account(id)?;
        let target = session_get_target(&account, url)?;
        let (observations, result, changed_headers) = if browser {
            let capture = crate::browser_session::capture_session_diagnostic(
                &self.credential(id)?,
                target.as_str(),
                self.network.allow_local,
            )
            .await?;
            let names: Vec<_> = capture.headers.keys().cloned().collect();
            let (_, changed_headers) = self.update_credential(id, |credential| {
                let changed = capture
                    .headers
                    .iter()
                    .any(|(name, value)| credential.headers.get(name) != Some(value));
                credential.headers.extend(capture.headers);
                Ok(changed)
            })?;
            (
                capture.observations,
                json!({"status":capture.status,"authenticated_requests":capture.authenticated_requests,"captured_headers":names,"values_exposed":false}),
                changed_headers,
            )
        } else {
            self.policy
                .resume_auth(&format!("{}|{id}", target.origin().ascii_serialization()))?;
            // A diagnostic is one request: every next URL needs its own validation.
            let response = self
                .network
                .fetch(crate::net::Request {
                    url: target.to_string(),
                    method: "GET".into(),
                    headers: self.credential(id)?.request_headers(target.as_str())?,
                    body: None,
                    account_id: id.into(),
                    limit: 2 * 1024 * 1024,
                    timeout: std::time::Duration::from_secs(25),
                    read_only: true,
                    follow_redirects: false,
                    tls_profile: "standard".into(),
                })
                .await?;
            let observation = Observation {
                method: "GET".into(),
                path: target.path().into(),
                query: target
                    .query_pairs()
                    .map(|(key, _)| (key.into_owned(), "string".into()))
                    .collect(),
                status: response.status,
                observed_at: crate::util::now(),
                ..Default::default()
            };
            (
                vec![observation],
                json!({"status":response.status,"successful":response.successful(),"response_shape":response.json().as_ref().map(|body|crate::session_recipe::shape(body,0)),"values_exposed":false}),
                false,
            )
        };
        self.state.update(|data| {
            if changed_headers {
                if let Some(view) = data.accounts.get_mut(id) {
                    view.verified_identity = false;
                    view.user_id.clear();
                    view.email.clear();
                    view.name.clear();
                    view.label.clear();
                    view.checked_at.clear();
                }
            }
            let meta = data.sites.entry(account.site_id.clone()).or_default();
            for observation in &observations {
                if let Some(old) = meta.observations.iter_mut().find(|old| {
                    old.method == observation.method
                        && old.path == observation.path
                        && old.query.keys().eq(observation.query.keys())
                }) {
                    *old = observation.clone();
                } else {
                    meta.observations.push(observation.clone());
                }
            }
            Ok(())
        })?;
        self.changed();
        self.session_output(
            id,
            json!({"account_id":id,"observation":result,"requests":observations}),
        )
    }

    pub(crate) async fn verify_observed_session(
        &self,
        id: &str,
        recipe: &ConnectionRecipe,
    ) -> AppResult<Value> {
        let account = self.session_account(id)?;
        let target = session_get_target(&account, &recipe.identity_url)?;
        let query_keys: std::collections::BTreeSet<_> = target
            .query_pairs()
            .map(|(key, _)| key.into_owned())
            .collect();
        let observed = self
            .state
            .read()?
            .sites
            .get(&account.site_id)
            .is_some_and(|meta| {
                meta.observations.iter().any(|observation| {
                    observation.method == "GET"
                        && observation.path == target.path()
                        && observation.query.keys().eq(query_keys.iter())
                })
            });
        if !observed || recipe.evidence != "observed_browser_request" {
            return Err(AppError::api("unsupported_evidence", 422));
        }
        let view = self.verify_session_recipe(id, recipe).await?;
        self.session_output(id, serde_json::to_value(view)?)
    }

    pub(crate) async fn refresh_account_session(&self, id: &str) -> AppResult<Value> {
        self.session_account(id)?;
        let recipe = self
            .credential(id)?
            .recipe
            .ok_or_else(|| AppError::api("identity_unavailable", 422))?;
        // Reuse verification's single-hop transport, strict target/evidence checks and baseline.
        let result = self.verify_observed_session(id, &recipe).await;
        if let Err(error) = &result {
            if matches!(
                error.public_code.as_str(),
                "auth_required" | "identity_unavailable"
            ) {
                self.state.update(|data| {
                    if let Some(view) = data.accounts.get_mut(id) {
                        view.verified_identity = false;
                        view.checked_at = crate::util::now();
                        if error.public_code == "auth_required" {
                            view.status = "expired".into();
                        } else {
                            view.user_id.clear();
                            view.email.clear();
                            view.name.clear();
                            view.label.clear();
                        }
                    }
                    Ok(())
                })?;
                self.changed();
            }
        }
        result
    }

    fn session_output(&self, id: &str, mut value: Value) -> AppResult<Value> {
        let secrets = self.credential(id)?.session_redaction_values();
        fn redact_keys(value: &mut Value, secrets: &[String]) {
            match value {
                Value::Object(fields) => {
                    for (key, mut child) in std::mem::take(fields) {
                        let clean = super::sanitized(Value::String(key), secrets);
                        redact_keys(&mut child, secrets);
                        fields.insert(clean.as_str().unwrap_or("[private]").into(), child);
                    }
                }
                Value::Array(items) => items.iter_mut().for_each(|v| redact_keys(v, secrets)),
                _ => {}
            }
        }
        redact_keys(&mut value, &secrets);
        Ok(super::sanitized(value, &secrets))
    }

    fn session_account(&self, id: &str) -> AppResult<AccountView> {
        self.state
            .account(id)?
            .ok_or_else(|| AppError::api("not_found", 404))
    }
}

fn session_get_target(account: &AccountView, url: &str) -> AppResult<url::Url> {
    let target = crate::net::validate_url(url)?;
    if !crate::net::same_site_origin(&target, &crate::net::validate_url(&account.site_url)?)
        || !crate::policy::safe_read_url(&target)
    {
        return Err(AppError::api("unsafe_target", 400));
    }
    if !crate::policy::safe_session_read_url(&target) {
        return Err(AppError::api("unsafe_target", 400));
    }
    Ok(target)
}
