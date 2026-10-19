//! Private identity proof and stale-verification publication boundary.
use super::Runtime;
use crate::{
    accounts::{self, AccountView, Credential, Identity},
    apperr::{AppError, AppResult},
    util,
};

impl Credential {
    pub(crate) fn identity_proof_for(
        &self,
        view: &AccountView,
        generation: &str,
    ) -> AppResult<String> {
        let mut snapshot = self.clone();
        snapshot.identity_proof.clear();
        Ok(util::hash(&serde_json::to_vec(&(
            generation,
            &view.id,
            &view.site_id,
            &view.site_url,
            &view.user_id,
            &view.email,
            &view.name,
            &view.browser,
            &view.profile,
            snapshot,
        ))?))
    }
    pub(crate) fn proves_identity(&self, view: &AccountView, generation: &str) -> AppResult<bool> {
        Ok(view.verified_identity
            && view.status == "connected"
            && !generation.is_empty()
            && !self.identity_proof.is_empty()
            && self.identity_proof == self.identity_proof_for(view, generation)?)
    }
}
impl Runtime {
    /// Commit only the identity observed using `expected`. A stale response may
    /// neither replace a newly connected credential nor relabel its account.
    pub(crate) fn publish_verified_identity(
        &self,
        account_id: &str,
        expected: &Credential,
        mut updated: Credential,
        identity: Identity,
    ) -> AppResult<AccountView> {
        let _guard = util::file_lock(&self.root.join("account-selection.lock"), false)?;
        let mut vault = self
            .vault
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        vault.reload()?;
        let current: Credential = serde_json::from_str(
            vault
                .get("accounts", account_id)
                .ok_or_else(|| AppError::api("auth_required", 401))?,
        )?;
        if serde_json::to_vec(&current)? != serde_json::to_vec(expected)? {
            return Err(AppError::api("conflict", 409));
        }
        let mut written = false;
        let result = self.state.update(|data| {
            let view = data
                .accounts
                .get_mut(account_id)
                .ok_or_else(|| AppError::api("auth_required", 401))?;
            if view.site_url != updated.origin || current.origin != updated.origin {
                return Err(AppError::api("unsafe_target", 400));
            }
            accounts::apply_identity(view, identity);
            let generation = util::id();
            updated.identity_proof = updated.identity_proof_for(view, &generation)?;
            data.identity_generations
                .insert(account_id.into(), generation);
            vault.set("accounts", account_id, &serde_json::to_string(&updated)?)?;
            written = true;
            let view = view.clone();
            let site = data.sites.entry(view.site_id.clone()).or_default();
            if let Some(recipe) = &updated.recipe {
                site.connection_recipe = Some(recipe.clone());
            }
            if !updated.identity_source.is_empty()
                && !site.identity_paths.contains(&updated.identity_source)
            {
                site.identity_paths.push(updated.identity_source.clone());
            }
            Ok(view)
        });
        if result.is_err() && written {
            vault.set("accounts", account_id, &serde_json::to_string(&current)?)?;
        }
        drop(vault);
        if result.is_ok() {
            self.changed();
        }
        result
    }

    /// An old failed refresh must not invalidate a newer successful sign-in.
    pub(crate) fn invalidate_identity_if_current(
        &self,
        account_id: &str,
        expected: &Credential,
        expired: bool,
    ) -> AppResult<()> {
        let _guard = util::file_lock(&self.root.join("account-selection.lock"), false)?;
        let mut vault = self
            .vault
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        vault.reload()?;
        let current: Credential = serde_json::from_str(
            vault
                .get("accounts", account_id)
                .ok_or_else(|| AppError::api("auth_required", 401))?,
        )?;
        if serde_json::to_vec(&current)? != serde_json::to_vec(expected)? {
            return Err(AppError::api("conflict", 409));
        }
        let mut updated = current.clone();
        updated.identity_proof.clear();
        let mut written = false;
        let result = self.state.update(|data| {
            let view = data
                .accounts
                .get_mut(account_id)
                .ok_or_else(|| AppError::api("auth_required", 401))?;
            data.identity_generations.remove(account_id);
            view.verified_identity = false;
            view.label.clear();
            view.name.clear();
            view.email.clear();
            view.user_id.clear();
            if expired {
                view.status = "expired".into();
            }
            view.checked_at = util::now();
            vault.set("accounts", account_id, &serde_json::to_string(&updated)?)?;
            written = true;
            Ok(())
        });
        if result.is_err() && written {
            vault.set("accounts", account_id, &serde_json::to_string(&current)?)?;
        }
        drop(vault);
        if result.is_ok() {
            self.changed();
        }
        result
    }
}
