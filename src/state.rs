use crate::{
    accounts::AccountView,
    apperr::{AppError, AppResult},
    util,
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

const MAX_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;
const MAX_JOBS_TOTAL: usize = 500;
const MAX_JOBS_FINISHED: usize = 400;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub locale: String,
    pub theme: String,
    pub reduce_motion: bool,
    pub notifications: bool,
    pub default_provider: String,
    pub engine_path: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            locale: "en".into(),
            theme: "light".into(),
            reduce_motion: false,
            notifications: false,
            default_provider: String::new(),
            engine_path: String::new(),
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProviderSettings {
    pub model: String,
    pub connected: bool,
    pub models: Vec<String>,
    pub checked_at: String,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct InputDescription {
    pub label: String,
    pub hint: String,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ActionDescription {
    pub title: String,
    pub description: String,
    pub output: String,
    pub inputs: BTreeMap<String, InputDescription>,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SiteDescription {
    pub title: String,
    pub summary: String,
    pub actions: BTreeMap<String, ActionDescription>,
    pub hash: String,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PreparationReport {
    pub os: String,
    pub architecture: String,
    pub sources: usize,
    pub tool_calls: usize,
    pub attempted_reads: usize,
    pub verified_reads: usize,
    pub account_verified: bool,
    pub workflows_total: usize,
    pub workflows_complete: usize,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct SelfQa {
    pub lens: String,
    pub question: String,
    pub evidence: String,
    pub answer: String,
    pub action_taken: String,
}
impl SelfQa {
    pub fn redact(&mut self) {
        self.lens = util::redact_text(&self.lens);
        self.question = util::redact_text(&self.question);
        self.evidence = util::redact_text(&self.evidence);
        self.answer = util::redact_text(&self.answer);
        self.action_taken = util::redact_text(&self.action_taken);
    }
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WebsiteUnderstanding {
    pub summary: String,
    pub workflows: Vec<WebsiteWorkflow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub self_qa: Vec<SelfQa>,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WebsiteWorkflow {
    pub id: String,
    pub title: String,
    pub benefit: String,
    pub steps: Vec<String>,
    pub success_criteria: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteMeta {
    pub workflows_complete: Option<bool>,
    pub pinned: bool,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_used: String,
    pub account_id: String,
    pub descriptions: BTreeMap<String, SiteDescription>,
    pub verified: BTreeMap<String, bool>,
    pub identity_paths: Vec<String>,
    pub observations: Vec<crate::accounts::Observation>,
    pub connection_recipe: Option<crate::session_recipe::ConnectionRecipe>,
    pub preparation: Option<PreparationReport>,
    pub icon_type: String,
    pub icon_source: String,
    /// Off until confirmed once for this CLI (`hycli writes <site> allow`, MCP `hycli_writes`, or dashboard toggle).
    pub writes: bool,
    /// Reads that passed during preparation, replayed by `hycli check` without AI.
    pub smoke: Vec<crate::smoke::ReadCheck>,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct JobAgent {
    pub id: String,
    pub status: String,
    pub found: usize,
    pub started_at: String,
    pub finished_at: String,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct JobNote {
    pub seq: u64,
    pub at: String,
    pub code: String,
    pub message: String,
    pub agent: String,
    pub count: usize,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub site_id: String,
    pub site_title: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub intent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub understanding: Option<WebsiteUnderstanding>,
    pub action_id: String,
    pub action_title: String,
    pub provider: String,
    pub locale: String,
    pub account_id: String,
    pub status: String,
    pub phase: String,
    pub error_code: String,
    pub found: Vec<String>,
    pub started_at: String,
    pub updated_at: String,
    pub finished_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    pub summary: String,
    #[serde(default)]
    pub approval_id: String,
    #[serde(default)]
    pub progress_done: usize,
    #[serde(default = "default_progress_total")]
    pub progress_total: usize,
    #[serde(default)]
    pub agents: Vec<JobAgent>,
    #[serde(default)]
    pub notes: Vec<JobNote>,
    #[serde(default)]
    pub parent_id: String,
}
fn default_progress_total() -> usize {
    5
}
impl Job {
    pub fn new(kind: &str, locale: &str) -> Self {
        Self {
            id: util::id(),
            kind: kind.into(),
            site_id: String::new(),
            site_title: String::new(),
            url: String::new(),
            intent: String::new(),
            understanding: None,
            action_id: String::new(),
            action_title: String::new(),
            provider: String::new(),
            locale: locale_or_default(locale).into(),
            account_id: String::new(),
            status: "queued".into(),
            phase: "queued".into(),
            error_code: String::new(),
            found: vec![],
            started_at: util::now(),
            updated_at: util::now(),
            finished_at: String::new(),
            result: None,
            summary: String::new(),
            approval_id: String::new(),
            progress_done: 0,
            progress_total: if kind == "prepare" { 5 } else { 2 },
            agents: vec![],
            notes: vec![],
            parent_id: String::new(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Data {
    pub settings: Settings,
    pub providers: BTreeMap<String, ProviderSettings>,
    pub sites: BTreeMap<String, SiteMeta>,
    pub accounts: BTreeMap<String, AccountView>,
    pub jobs: Vec<Job>,
}

#[derive(PartialEq, Eq)]
struct SiteRow {
    workflows_complete: Option<i64>,
    pinned: i64,
    title: String,
    created_at: String,
    updated_at: String,
    last_used: String,
    account_id: String,
    descriptions_json: String,
    verified_json: String,
    identity_paths_json: String,
    observations_json: String,
    connection_recipe_json: Option<String>,
    preparation_json: Option<String>,
    icon_type: String,
    icon_source: String,
    writes: i64,
    smoke_json: String,
}
impl SiteRow {
    fn from_row(r: &Row<'_>, offset: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            workflows_complete: r.get(offset)?,
            pinned: r.get(offset + 1)?,
            title: r.get(offset + 2)?,
            created_at: r.get(offset + 3)?,
            updated_at: r.get(offset + 4)?,
            last_used: r.get(offset + 5)?,
            account_id: r.get(offset + 6)?,
            descriptions_json: r.get(offset + 7)?,
            verified_json: r.get(offset + 8)?,
            identity_paths_json: r.get(offset + 9)?,
            observations_json: r.get(offset + 10)?,
            connection_recipe_json: r.get(offset + 11)?,
            preparation_json: r.get(offset + 12)?,
            icon_type: r.get(offset + 13)?,
            icon_source: r.get(offset + 14)?,
            writes: r.get(offset + 15)?,
            smoke_json: r.get(offset + 16)?,
        })
    }
    fn from_meta(meta: &SiteMeta) -> AppResult<Self> {
        Ok(Self {
            workflows_complete: meta.workflows_complete.map(i64::from),
            pinned: i64::from(meta.pinned),
            title: meta.title.clone(),
            created_at: meta.created_at.clone(),
            updated_at: meta.updated_at.clone(),
            last_used: meta.last_used.clone(),
            account_id: meta.account_id.clone(),
            descriptions_json: serde_json::to_string(&meta.descriptions)?,
            verified_json: serde_json::to_string(&meta.verified)?,
            identity_paths_json: serde_json::to_string(&meta.identity_paths)?,
            observations_json: serde_json::to_string(&meta.observations)?,
            connection_recipe_json: meta
                .connection_recipe
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            preparation_json: meta
                .preparation
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            icon_type: meta.icon_type.clone(),
            icon_source: meta.icon_source.clone(),
            writes: i64::from(meta.writes),
            smoke_json: serde_json::to_string(&meta.smoke)?,
        })
    }
    fn into_meta(self) -> AppResult<SiteMeta> {
        Ok(SiteMeta {
            workflows_complete: self.workflows_complete.map(|v| v != 0),
            pinned: self.pinned != 0,
            title: self.title,
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_used: self.last_used,
            account_id: self.account_id,
            descriptions: serde_json::from_str(&self.descriptions_json)?,
            verified: serde_json::from_str(&self.verified_json)?,
            identity_paths: serde_json::from_str(&self.identity_paths_json)?,
            observations: serde_json::from_str(&self.observations_json)?,
            connection_recipe: self
                .connection_recipe_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?,
            preparation: self
                .preparation_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?,
            icon_type: self.icon_type,
            icon_source: self.icon_source,
            writes: self.writes != 0,
            smoke: serde_json::from_str(&self.smoke_json)?,
        })
    }
}

struct JobRow {
    id: String,
    kind: String,
    site_id: String,
    site_title: String,
    url: String,
    intent: String,
    understanding_json: Option<String>,
    action_id: String,
    action_title: String,
    provider: String,
    locale: String,
    account_id: String,
    status: String,
    phase: String,
    error_code: String,
    found_json: String,
    started_at: String,
    updated_at: String,
    finished_at: String,
    summary: String,
    approval_id: String,
    progress_done: usize,
    progress_total: usize,
    parent_id: String,
}
impl JobRow {
    fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            kind: r.get(1)?,
            site_id: r.get(2)?,
            site_title: r.get(3)?,
            url: r.get(4)?,
            intent: r.get(5)?,
            understanding_json: r.get(6)?,
            action_id: r.get(7)?,
            action_title: r.get(8)?,
            provider: r.get(9)?,
            locale: r.get(10)?,
            account_id: r.get(11)?,
            status: r.get(12)?,
            phase: r.get(13)?,
            error_code: r.get(14)?,
            found_json: r.get(15)?,
            started_at: r.get(16)?,
            updated_at: r.get(17)?,
            finished_at: r.get(18)?,
            summary: r.get(19)?,
            approval_id: r.get(20)?,
            progress_done: r.get::<_, i64>(21)? as usize,
            progress_total: r.get::<_, i64>(22)? as usize,
            parent_id: r.get(23)?,
        })
    }
    fn into_job(
        self,
        agents: Vec<JobAgent>,
        notes: Vec<JobNote>,
        result: Option<Value>,
    ) -> AppResult<Job> {
        Ok(Job {
            id: self.id,
            kind: self.kind,
            site_id: self.site_id,
            site_title: self.site_title,
            url: self.url,
            intent: self.intent,
            understanding: self
                .understanding_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?,
            action_id: self.action_id,
            action_title: self.action_title,
            provider: self.provider,
            locale: self.locale,
            account_id: self.account_id,
            status: self.status,
            phase: self.phase,
            error_code: self.error_code,
            found: serde_json::from_str(&self.found_json)?,
            started_at: self.started_at,
            updated_at: self.updated_at,
            finished_at: self.finished_at,
            result,
            summary: self.summary,
            approval_id: self.approval_id,
            progress_done: self.progress_done,
            progress_total: self.progress_total,
            agents,
            notes,
            parent_id: self.parent_id,
        })
    }
}

pub struct StateStore {
    legacy_path: PathBuf,
    db_path: PathBuf,
    conn: Mutex<Connection>,
}

impl StateStore {
    pub fn open(path: &Path) -> AppResult<Self> {
        Self::open_with_recovery(path, true)
    }
    pub fn open_with_recovery(path: &Path, recover: bool) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                util::private_dir(parent)?;
            }
        }
        let db_path = path.with_extension("db");
        let legacy_path = if path.extension().is_some_and(|ext| ext == "json") {
            path.to_owned()
        } else {
            path.with_extension("json")
        };
        let conn = Connection::open(&db_path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        #[cfg(unix)]
        if db_path.exists() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600));
        }
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;

             CREATE TABLE IF NOT EXISTS state_meta (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS settings (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 locale TEXT NOT NULL DEFAULT 'en',
                 theme TEXT NOT NULL DEFAULT 'light',
                 reduce_motion INTEGER NOT NULL DEFAULT 0,
                 notifications INTEGER NOT NULL DEFAULT 0,
                 default_provider TEXT NOT NULL DEFAULT '',
                 engine_path TEXT NOT NULL DEFAULT ''
             );

             CREATE TABLE IF NOT EXISTS providers (
                 id TEXT PRIMARY KEY,
                 model TEXT NOT NULL DEFAULT '',
                 connected INTEGER NOT NULL DEFAULT 0,
                 models_json TEXT NOT NULL DEFAULT '[]',
                 checked_at TEXT NOT NULL DEFAULT ''
             );

             CREATE TABLE IF NOT EXISTS sites (
                 id TEXT PRIMARY KEY,
                 workflows_complete INTEGER,
                 pinned INTEGER NOT NULL DEFAULT 0,
                 title TEXT NOT NULL DEFAULT '',
                 created_at TEXT NOT NULL DEFAULT '',
                 updated_at TEXT NOT NULL DEFAULT '',
                 last_used TEXT NOT NULL DEFAULT '',
                 account_id TEXT NOT NULL DEFAULT '',
                 descriptions_json TEXT NOT NULL DEFAULT '{}',
                 verified_json TEXT NOT NULL DEFAULT '{}',
                 identity_paths_json TEXT NOT NULL DEFAULT '[]',
                 observations_json TEXT NOT NULL DEFAULT '[]',
                 connection_recipe_json TEXT,
                 preparation_json TEXT,
                 icon_type TEXT NOT NULL DEFAULT '',
                 icon_source TEXT NOT NULL DEFAULT '',
                 writes INTEGER NOT NULL DEFAULT 0,
                 smoke_json TEXT NOT NULL DEFAULT '[]'
             );

             CREATE TABLE IF NOT EXISTS accounts (
                 id TEXT PRIMARY KEY,
                 data_json TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS jobs (
                 ord INTEGER NOT NULL DEFAULT 0,
                 id TEXT PRIMARY KEY,
                 kind TEXT NOT NULL,
                 site_id TEXT NOT NULL DEFAULT '',
                 site_title TEXT NOT NULL DEFAULT '',
                 url TEXT NOT NULL DEFAULT '',
                 intent TEXT NOT NULL DEFAULT '',
                 understanding_json TEXT,
                 action_id TEXT NOT NULL DEFAULT '',
                 action_title TEXT NOT NULL DEFAULT '',
                 provider TEXT NOT NULL DEFAULT '',
                 locale TEXT NOT NULL DEFAULT 'en',
                 account_id TEXT NOT NULL DEFAULT '',
                 status TEXT NOT NULL DEFAULT 'queued',
                 phase TEXT NOT NULL DEFAULT 'queued',
                 error_code TEXT NOT NULL DEFAULT '',
                 found_json TEXT NOT NULL DEFAULT '[]',
                 started_at TEXT NOT NULL DEFAULT '',
                 updated_at TEXT NOT NULL DEFAULT '',
                 finished_at TEXT NOT NULL DEFAULT '',
                 summary TEXT NOT NULL DEFAULT '',
                 approval_id TEXT NOT NULL DEFAULT '',
                 progress_done INTEGER NOT NULL DEFAULT 0,
                 progress_total INTEGER NOT NULL DEFAULT 5,
                 parent_id TEXT NOT NULL DEFAULT ''
             );
             CREATE INDEX IF NOT EXISTS idx_jobs_ord ON jobs(ord ASC);
             CREATE INDEX IF NOT EXISTS idx_jobs_status_kind_site
                 ON jobs(status, kind, site_id, locale, parent_id);
             CREATE INDEX IF NOT EXISTS idx_jobs_site_status
                 ON jobs(site_id, status);

             CREATE TABLE IF NOT EXISTS job_agents (
                 job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
                 idx INTEGER NOT NULL,
                 id TEXT NOT NULL,
                 status TEXT NOT NULL DEFAULT '',
                 found INTEGER NOT NULL DEFAULT 0,
                 started_at TEXT NOT NULL DEFAULT '',
                 finished_at TEXT NOT NULL DEFAULT '',
                 PRIMARY KEY (job_id, idx)
             );

             CREATE TABLE IF NOT EXISTS job_notes (
                 job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
                 seq INTEGER NOT NULL,
                 at TEXT NOT NULL,
                 code TEXT NOT NULL,
                 message TEXT NOT NULL,
                 agent TEXT NOT NULL DEFAULT '',
                 count INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (job_id, seq)
             );

             CREATE TABLE IF NOT EXISTS job_results (
                 job_id TEXT PRIMARY KEY REFERENCES jobs(id) ON DELETE CASCADE,
                 result_json BLOB NOT NULL
             );",
        )?;
        let store = Self {
            legacy_path,
            db_path,
            conn: Mutex::new(conn),
        };
        store.migrate_legacy_json_if_needed()?;
        if recover {
            store.recover_interrupted_jobs()?;
        }
        Ok(store)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn has_job_result(&self, id: &str) -> AppResult<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM job_results WHERE job_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(exists.is_some())
    }

    fn migrate_legacy_json_if_needed(&self) -> AppResult<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        let already: Option<String> = conn
            .query_row(
                "SELECT value FROM state_meta WHERE key = 'migrated_from_json'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if already.as_deref() == Some("1") {
            if self.legacy_path.exists() {
                let migrated_backup = self.legacy_path.with_extension("json.migrated");
                let _ = std::fs::rename(&self.legacy_path, migrated_backup);
            }
            return Ok(());
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let already_in_tx: Option<String> = tx
            .query_row(
                "SELECT value FROM state_meta WHERE key = 'migrated_from_json'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if already_in_tx.as_deref() == Some("1") {
            if self.legacy_path.exists() {
                let migrated_backup = self.legacy_path.with_extension("json.migrated");
                let _ = std::fs::rename(&self.legacy_path, migrated_backup);
            }
            return Ok(());
        }
        let mut migrated_results = Vec::new();
        let migrated_json = self.legacy_path.exists();
        if migrated_json {
            let bytes = util::read_bounded(&self.legacy_path, MAX_PAYLOAD_BYTES)?;
            let mut legacy: Data = serde_json::from_slice(&bytes)?;
            for job in &mut legacy.jobs {
                if job.result.is_none() {
                    if let Ok(result_file) = Self::legacy_result_path(&self.legacy_path, &job.id) {
                        if result_file.exists() {
                            if let Ok(raw) = util::read_bounded(&result_file, MAX_PAYLOAD_BYTES) {
                                if let Ok(val) = serde_json::from_slice::<Value>(&raw) {
                                    job.result = Some(val);
                                    migrated_results.push(result_file);
                                }
                            }
                        }
                    }
                }
            }
            let current = Self::load_data(&tx)?;
            Self::save_diff(&tx, &current, &legacy)?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO state_meta (key, value) VALUES ('migrated_from_json', '1')",
            [],
        )?;
        tx.commit()?;
        for result_file in migrated_results {
            let _ = std::fs::remove_file(result_file);
        }
        if migrated_json {
            let migrated_backup = self.legacy_path.with_extension("json.migrated");
            let _ = std::fs::rename(&self.legacy_path, migrated_backup);
        }
        Ok(())
    }

    fn recover_interrupted_jobs(&self) -> AppResult<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut stmt = tx.prepare(
            "SELECT id FROM jobs WHERE status IN ('queued', 'running') ORDER BY ord ASC",
        )?;
        let ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        if ids.is_empty() {
            return Ok(());
        }
        for id in ids {
            if let Ok(mut job) = Self::load_single_job(&tx, &id, false) {
                let before_agents = job.agents.clone();
                let before_notes = job.notes.clone();
                let uncertain = job.kind == "action" && !job.approval_id.is_empty();
                job.status = if uncertain { "failed" } else { "cancelled" }.into();
                job.phase = job.status.clone();
                job.error_code = if uncertain {
                    "action_outcome_unknown"
                } else {
                    "cancelled"
                }
                .into();
                job.finished_at = util::now();
                job.updated_at = util::now();
                for agent in &mut job.agents {
                    if ["running", "queued"].contains(&agent.status.as_str()) {
                        agent.status = "cancelled".into();
                        agent.finished_at = util::now();
                    }
                }
                job.note("interrupted", "", 0, "");
                Self::update_job_row(&tx, &mut job, &before_agents, &before_notes)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn legacy_result_path(base_path: &Path, id: &str) -> AppResult<PathBuf> {
        if id.len() != 32
            || !id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(AppError::api("not_found", 404));
        }
        Ok(base_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("results")
            .join(format!("{id}.json")))
    }

    pub fn settings(&self) -> AppResult<Settings> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        Self::load_settings(&conn)
    }

    pub fn site(&self, id: &str) -> AppResult<Option<SiteMeta>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        let row: Option<SiteRow> = conn
            .query_row(
                "SELECT workflows_complete, pinned, title, created_at, updated_at, last_used,
                        account_id, descriptions_json, verified_json, identity_paths_json,
                        observations_json, connection_recipe_json, preparation_json,
                        icon_type, icon_source, writes, smoke_json
                 FROM sites WHERE id = ?1",
                params![id],
                |r| SiteRow::from_row(r, 0),
            )
            .optional()?;
        row.map(SiteRow::into_meta).transpose()
    }

    pub fn site_writes(&self, id: &str) -> AppResult<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        let writes: Option<i64> = conn
            .query_row("SELECT writes FROM sites WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(writes.unwrap_or(0) != 0)
    }

    pub fn account(&self, id: &str) -> AppResult<Option<AccountView>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        let data_json: Option<String> = conn
            .query_row(
                "SELECT data_json FROM accounts WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        data_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(Into::into)
    }

    pub fn accounts(&self) -> AppResult<BTreeMap<String, AccountView>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        Self::load_accounts(&conn)
    }

    pub fn latest_understanding(&self, site_id: &str) -> AppResult<Option<WebsiteUnderstanding>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT understanding_json FROM jobs
                 WHERE site_id = ?1 AND understanding_json IS NOT NULL
                 ORDER BY ord ASC, rowid DESC
                 LIMIT 1",
                params![site_id],
                |r| r.get(0),
            )
            .optional()?;
        raw.as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(Into::into)
    }

    pub fn read(&self) -> AppResult<Data> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        Self::load_data(&tx)
    }

    pub fn update<T>(&self, apply: impl FnOnce(&mut Data) -> AppResult<T>) -> AppResult<T> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = Self::load_data(&tx)?;
        let mut next = before.clone();
        let result = apply(&mut next)?;
        if next.jobs.len() > MAX_JOBS_TOTAL {
            let mut finished = 0;
            next.jobs.retain(|j| {
                if ["running", "queued"].contains(&j.status.as_str()) {
                    true
                } else {
                    finished += 1;
                    finished <= MAX_JOBS_FINISHED
                }
            });
        }
        Self::save_diff(&tx, &before, &next)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn add_job(&self, mut job: Job) -> AppResult<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let duplicate = match job.kind.as_str() {
            "prepare" => tx
                .query_row(
                    "SELECT 1 FROM jobs
                     WHERE status IN ('running', 'queued') AND kind = 'prepare' AND site_id = ?1
                     LIMIT 1",
                    params![job.site_id],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
                .is_some(),
            "describe" => tx
                .query_row(
                    "SELECT 1 FROM jobs
                     WHERE status IN ('running', 'queued') AND kind = 'describe'
                       AND site_id = ?1 AND locale = ?2
                     LIMIT 1",
                    params![job.site_id, job.locale],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
                .is_some(),
            "summary" => tx
                .query_row(
                    "SELECT 1 FROM jobs
                     WHERE status IN ('running', 'queued') AND kind = 'summary' AND parent_id = ?1
                     LIMIT 1",
                    params![job.parent_id],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
                .is_some(),
            _ => false,
        };
        if duplicate {
            return Err(AppError::api("job_running", 409));
        }
        let next_ord: i64 =
            tx.query_row("SELECT COALESCE(MIN(ord) - 1, 0) FROM jobs", [], |r| {
                r.get(0)
            })?;
        Self::insert_or_replace_job(&tx, next_ord, &mut job)?;
        Self::trim_excess_jobs(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub fn job(&self, id: &str) -> AppResult<Job> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        Self::load_single_job(&tx, id, true)
    }

    pub fn update_job(&self, id: &str, apply: impl FnOnce(&mut Job)) -> AppResult<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| AppError::api("internal", 500))?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut job = Self::load_single_job(&tx, id, false)?;
        let before_agents = job.agents.clone();
        let before_notes = job.notes.clone();
        apply(&mut job);
        job.updated_at = util::now();
        Self::update_job_row(&tx, &mut job, &before_agents, &before_notes)?;
        tx.commit()?;
        Ok(())
    }

    fn load_settings(conn: &Connection) -> AppResult<Settings> {
        let settings = conn
            .query_row(
                "SELECT locale, theme, reduce_motion, notifications, default_provider, engine_path
                 FROM settings WHERE id = 1",
                [],
                |r| {
                    Ok(Settings {
                        locale: r.get(0)?,
                        theme: r.get(1)?,
                        reduce_motion: r.get::<_, i64>(2)? != 0,
                        notifications: r.get::<_, i64>(3)? != 0,
                        default_provider: r.get(4)?,
                        engine_path: r.get(5)?,
                    })
                },
            )
            .optional()?
            .unwrap_or_default();
        Ok(settings)
    }

    fn load_accounts(conn: &Connection) -> AppResult<BTreeMap<String, AccountView>> {
        let mut accounts = BTreeMap::new();
        let mut stmt = conn.prepare("SELECT id, data_json FROM accounts ORDER BY id ASC")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (id, data_json) = row?;
            accounts.insert(id, serde_json::from_str(&data_json)?);
        }
        Ok(accounts)
    }

    fn load_data(tx: &Transaction<'_>) -> AppResult<Data> {
        let settings = Self::load_settings(tx)?;

        let mut providers = BTreeMap::new();
        {
            let mut stmt = tx.prepare(
                "SELECT id, model, connected, models_json, checked_at FROM providers ORDER BY id ASC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)? != 0,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?;
            for row in rows {
                let (id, model, connected, models_json, checked_at) = row?;
                providers.insert(
                    id,
                    ProviderSettings {
                        model,
                        connected,
                        models: serde_json::from_str(&models_json)?,
                        checked_at,
                    },
                );
            }
        }

        let mut sites = BTreeMap::new();
        {
            let mut stmt = tx.prepare(
                "SELECT id, workflows_complete, pinned, title, created_at, updated_at, last_used,
                        account_id, descriptions_json, verified_json, identity_paths_json,
                        observations_json, connection_recipe_json, preparation_json,
                        icon_type, icon_source, writes, smoke_json
                 FROM sites ORDER BY id ASC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, SiteRow::from_row(r, 1)?))
            })?;
            for row in rows {
                let (id, site_row) = row?;
                sites.insert(id, site_row.into_meta()?);
            }
        }

        let accounts = Self::load_accounts(tx)?;

        let mut agents_by_job: BTreeMap<String, Vec<JobAgent>> = BTreeMap::new();
        {
            let mut stmt = tx.prepare(
                "SELECT job_id, id, status, found, started_at, finished_at
                 FROM job_agents ORDER BY job_id ASC, idx ASC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    JobAgent {
                        id: r.get(1)?,
                        status: r.get(2)?,
                        found: r.get::<_, i64>(3)? as usize,
                        started_at: r.get(4)?,
                        finished_at: r.get(5)?,
                    },
                ))
            })?;
            for row in rows {
                let (job_id, agent) = row?;
                agents_by_job.entry(job_id).or_default().push(agent);
            }
        }

        let mut notes_by_job: BTreeMap<String, Vec<JobNote>> = BTreeMap::new();
        {
            let mut stmt = tx.prepare(
                "SELECT job_id, seq, at, code, message, agent, count
                 FROM job_notes ORDER BY job_id ASC, seq ASC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    JobNote {
                        seq: r.get::<_, i64>(1)? as u64,
                        at: r.get(2)?,
                        code: r.get(3)?,
                        message: r.get(4)?,
                        agent: r.get(5)?,
                        count: r.get::<_, i64>(6)? as usize,
                    },
                ))
            })?;
            for row in rows {
                let (job_id, note) = row?;
                notes_by_job.entry(job_id).or_default().push(note);
            }
        }

        let mut jobs = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT id, kind, site_id, site_title, url, intent, understanding_json,
                        action_id, action_title, provider, locale, account_id, status, phase,
                        error_code, found_json, started_at, updated_at, finished_at, summary,
                        approval_id, progress_done, progress_total, parent_id
                 FROM jobs ORDER BY ord ASC, rowid DESC",
            )?;
            let rows = stmt.query_map([], JobRow::from_row)?;
            for row in rows {
                let job_row = row?;
                let agents = agents_by_job.remove(&job_row.id).unwrap_or_default();
                let notes = notes_by_job.remove(&job_row.id).unwrap_or_default();
                jobs.push(job_row.into_job(agents, notes, None)?);
            }
        }

        Ok(Data {
            settings,
            providers,
            sites,
            accounts,
            jobs,
        })
    }

    fn load_single_job(tx: &Transaction<'_>, id: &str, with_result: bool) -> AppResult<Job> {
        let job_row = tx
            .query_row(
                "SELECT id, kind, site_id, site_title, url, intent, understanding_json,
                        action_id, action_title, provider, locale, account_id, status, phase,
                        error_code, found_json, started_at, updated_at, finished_at, summary,
                        approval_id, progress_done, progress_total, parent_id
                 FROM jobs WHERE id = ?1",
                params![id],
                JobRow::from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::api("not_found", 404))?;

        let mut agents = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT id, status, found, started_at, finished_at
                 FROM job_agents WHERE job_id = ?1 ORDER BY idx ASC",
            )?;
            let rows = stmt.query_map(params![id], |r| {
                Ok(JobAgent {
                    id: r.get(0)?,
                    status: r.get(1)?,
                    found: r.get::<_, i64>(2)? as usize,
                    started_at: r.get(3)?,
                    finished_at: r.get(4)?,
                })
            })?;
            for agent in rows {
                agents.push(agent?);
            }
        }

        let mut notes = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT seq, at, code, message, agent, count
                 FROM job_notes WHERE job_id = ?1 ORDER BY seq ASC",
            )?;
            let rows = stmt.query_map(params![id], |r| {
                Ok(JobNote {
                    seq: r.get::<_, i64>(0)? as u64,
                    at: r.get(1)?,
                    code: r.get(2)?,
                    message: r.get(3)?,
                    agent: r.get(4)?,
                    count: r.get::<_, i64>(5)? as usize,
                })
            })?;
            for note in rows {
                notes.push(note?);
            }
        }

        let result = if with_result {
            let raw: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT result_json FROM job_results WHERE job_id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?;
            match raw {
                Some(bytes) => Some(serde_json::from_slice(&bytes)?),
                None => None,
            }
        } else {
            None
        };

        job_row.into_job(agents, notes, result)
    }

    fn save_diff(tx: &Transaction<'_>, before: &Data, next: &Data) -> AppResult<()> {
        if next.settings != before.settings {
            tx.execute(
                "INSERT INTO settings (id, locale, theme, reduce_motion, notifications, default_provider, engine_path)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                     locale = excluded.locale,
                     theme = excluded.theme,
                     reduce_motion = excluded.reduce_motion,
                     notifications = excluded.notifications,
                     default_provider = excluded.default_provider,
                     engine_path = excluded.engine_path",
                params![
                    next.settings.locale,
                    next.settings.theme,
                    i64::from(next.settings.reduce_motion),
                    i64::from(next.settings.notifications),
                    next.settings.default_provider,
                    next.settings.engine_path,
                ],
            )?;
        }

        for id in before.providers.keys() {
            if !next.providers.contains_key(id) {
                tx.execute("DELETE FROM providers WHERE id = ?1", params![id])?;
            }
        }
        for (id, provider) in &next.providers {
            if before.providers.get(id) != Some(provider) {
                tx.execute(
                    "INSERT INTO providers (id, model, connected, models_json, checked_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(id) DO UPDATE SET
                         model = excluded.model,
                         connected = excluded.connected,
                         models_json = excluded.models_json,
                         checked_at = excluded.checked_at",
                    params![
                        id,
                        provider.model,
                        i64::from(provider.connected),
                        serde_json::to_string(&provider.models)?,
                        provider.checked_at,
                    ],
                )?;
            }
        }

        for id in before.sites.keys() {
            if !next.sites.contains_key(id) {
                tx.execute("DELETE FROM sites WHERE id = ?1", params![id])?;
            }
        }
        for (id, meta) in &next.sites {
            let new_row = SiteRow::from_meta(meta)?;
            let changed = match before.sites.get(id) {
                Some(old_meta) => SiteRow::from_meta(old_meta)? != new_row,
                None => true,
            };
            if changed {
                tx.execute(
                    "INSERT INTO sites (
                         id, workflows_complete, pinned, title, created_at, updated_at, last_used,
                         account_id, descriptions_json, verified_json, identity_paths_json,
                         observations_json, connection_recipe_json, preparation_json,
                         icon_type, icon_source, writes, smoke_json
                     ) VALUES (
                         ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
                     )
                     ON CONFLICT(id) DO UPDATE SET
                         workflows_complete = excluded.workflows_complete,
                         pinned = excluded.pinned,
                         title = excluded.title,
                         created_at = excluded.created_at,
                         updated_at = excluded.updated_at,
                         last_used = excluded.last_used,
                         account_id = excluded.account_id,
                         descriptions_json = excluded.descriptions_json,
                         verified_json = excluded.verified_json,
                         identity_paths_json = excluded.identity_paths_json,
                         observations_json = excluded.observations_json,
                         connection_recipe_json = excluded.connection_recipe_json,
                         preparation_json = excluded.preparation_json,
                         icon_type = excluded.icon_type,
                         icon_source = excluded.icon_source,
                         writes = excluded.writes,
                         smoke_json = excluded.smoke_json",
                    params![
                        id,
                        new_row.workflows_complete,
                        new_row.pinned,
                        new_row.title,
                        new_row.created_at,
                        new_row.updated_at,
                        new_row.last_used,
                        new_row.account_id,
                        new_row.descriptions_json,
                        new_row.verified_json,
                        new_row.identity_paths_json,
                        new_row.observations_json,
                        new_row.connection_recipe_json,
                        new_row.preparation_json,
                        new_row.icon_type,
                        new_row.icon_source,
                        new_row.writes,
                        new_row.smoke_json,
                    ],
                )?;
            }
        }

        for id in before.accounts.keys() {
            if !next.accounts.contains_key(id) {
                tx.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
            }
        }
        for (id, account) in &next.accounts {
            let new_json = serde_json::to_string(account)?;
            let changed = match before.accounts.get(id) {
                Some(old_acc) => serde_json::to_string(old_acc)? != new_json,
                None => true,
            };
            if changed {
                tx.execute(
                    "INSERT INTO accounts (id, data_json) VALUES (?1, ?2)
                     ON CONFLICT(id) DO UPDATE SET data_json = excluded.data_json",
                    params![id, new_json],
                )?;
            }
        }

        let next_ids: BTreeSet<&str> = next.jobs.iter().map(|j| j.id.as_str()).collect();
        for old_job in &before.jobs {
            if !next_ids.contains(old_job.id.as_str()) {
                tx.execute("DELETE FROM jobs WHERE id = ?1", params![old_job.id])?;
            }
        }

        let before_map: BTreeMap<&str, (usize, &Job)> = before
            .jobs
            .iter()
            .enumerate()
            .map(|(idx, j)| (j.id.as_str(), (idx, j)))
            .collect();

        for (idx, job) in next.jobs.iter().enumerate() {
            let ord = idx as i64;
            match before_map.get(job.id.as_str()) {
                Some((old_idx, old_job)) if *old_idx == idx && *old_job == job => {}
                Some((_, old_job)) if *old_job == job => {
                    tx.execute(
                        "UPDATE jobs SET ord = ?1 WHERE id = ?2",
                        params![ord, job.id],
                    )?;
                }
                Some((_, old_job)) => {
                    let mut cloned = job.clone();
                    Self::upsert_job_with_ord(
                        tx,
                        ord,
                        &mut cloned,
                        Some(&old_job.agents),
                        Some(&old_job.notes),
                    )?;
                }
                None => {
                    let mut cloned = job.clone();
                    Self::insert_or_replace_job(tx, ord, &mut cloned)?;
                }
            }
        }
        Ok(())
    }

    fn insert_or_replace_job(tx: &Transaction<'_>, ord: i64, job: &mut Job) -> AppResult<()> {
        Self::upsert_job_with_ord(tx, ord, job, None, None)
    }

    fn update_job_row(
        tx: &Transaction<'_>,
        job: &mut Job,
        before_agents: &[JobAgent],
        before_notes: &[JobNote],
    ) -> AppResult<()> {
        let ord: i64 =
            tx.query_row("SELECT ord FROM jobs WHERE id = ?1", params![job.id], |r| {
                r.get(0)
            })?;
        Self::upsert_job_with_ord(tx, ord, job, Some(before_agents), Some(before_notes))
    }

    fn upsert_job_with_ord(
        tx: &Transaction<'_>,
        ord: i64,
        job: &mut Job,
        before_agents: Option<&[JobAgent]>,
        before_notes: Option<&[JobNote]>,
    ) -> AppResult<()> {
        let pending_result = job.result.take();
        let understanding_json = job
            .understanding
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let found_json = serde_json::to_string(&job.found)?;
        let row_bytes = understanding_json.as_ref().map_or(0, String::len)
            + found_json.len()
            + job.summary.len()
            + job.intent.len();
        if row_bytes > MAX_PAYLOAD_BYTES {
            return Err(AppError::api("too_large", 413));
        }

        tx.execute(
            "INSERT INTO jobs (
                 ord, id, kind, site_id, site_title, url, intent, understanding_json,
                 action_id, action_title, provider, locale, account_id, status, phase,
                 error_code, found_json, started_at, updated_at, finished_at, summary,
                 approval_id, progress_done, progress_total, parent_id
             ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25
             )
             ON CONFLICT(id) DO UPDATE SET
                 ord = excluded.ord,
                 kind = excluded.kind,
                 site_id = excluded.site_id,
                 site_title = excluded.site_title,
                 url = excluded.url,
                 intent = excluded.intent,
                 understanding_json = excluded.understanding_json,
                 action_id = excluded.action_id,
                 action_title = excluded.action_title,
                 provider = excluded.provider,
                 locale = excluded.locale,
                 account_id = excluded.account_id,
                 status = excluded.status,
                 phase = excluded.phase,
                 error_code = excluded.error_code,
                 found_json = excluded.found_json,
                 started_at = excluded.started_at,
                 updated_at = excluded.updated_at,
                 finished_at = excluded.finished_at,
                 summary = excluded.summary,
                 approval_id = excluded.approval_id,
                 progress_done = excluded.progress_done,
                 progress_total = excluded.progress_total,
                 parent_id = excluded.parent_id",
            params![
                ord,
                job.id,
                job.kind,
                job.site_id,
                job.site_title,
                job.url,
                job.intent,
                understanding_json,
                job.action_id,
                job.action_title,
                job.provider,
                job.locale,
                job.account_id,
                job.status,
                job.phase,
                job.error_code,
                found_json,
                job.started_at,
                job.updated_at,
                job.finished_at,
                job.summary,
                job.approval_id,
                job.progress_done as i64,
                job.progress_total as i64,
                job.parent_id,
            ],
        )?;

        if let Some(value) = pending_result {
            let bytes = serde_json::to_vec(&value)?;
            if bytes.len() > MAX_PAYLOAD_BYTES {
                return Err(AppError::api("too_large", 413));
            }
            tx.execute(
                "INSERT INTO job_results (job_id, result_json) VALUES (?1, ?2)
                 ON CONFLICT(job_id) DO UPDATE SET result_json = excluded.result_json",
                params![job.id, bytes],
            )?;
        }

        if before_agents.is_none_or(|old| old != job.agents.as_slice()) {
            tx.execute("DELETE FROM job_agents WHERE job_id = ?1", params![job.id])?;
            for (idx, agent) in job.agents.iter().enumerate() {
                tx.execute(
                    "INSERT INTO job_agents (job_id, idx, id, status, found, started_at, finished_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        job.id,
                        idx as i64,
                        agent.id,
                        agent.status,
                        agent.found as i64,
                        agent.started_at,
                        agent.finished_at,
                    ],
                )?;
            }
        }

        match before_notes {
            Some(old) if old == job.notes.as_slice() => {}
            Some(old)
                if job.notes.len() > old.len()
                    && job.notes[..old.len()] == *old
                    && job.notes.windows(2).all(|w| w[0].seq < w[1].seq) =>
            {
                for note in &job.notes[old.len()..] {
                    tx.execute(
                        "INSERT INTO job_notes (job_id, seq, at, code, message, agent, count)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            job.id,
                            note.seq as i64,
                            note.at,
                            note.code,
                            note.message,
                            note.agent,
                            note.count as i64,
                        ],
                    )?;
                }
            }
            _ => {
                tx.execute("DELETE FROM job_notes WHERE job_id = ?1", params![job.id])?;
                let mut prev_seq = 0u64;
                for note in &job.notes {
                    let seq = if note.seq > prev_seq {
                        note.seq
                    } else {
                        prev_seq + 1
                    };
                    prev_seq = seq;
                    tx.execute(
                        "INSERT INTO job_notes (job_id, seq, at, code, message, agent, count)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            job.id,
                            seq as i64,
                            note.at,
                            note.code,
                            note.message,
                            note.agent,
                            note.count as i64,
                        ],
                    )?;
                }
            }
        }

        Ok(())
    }

    fn trim_excess_jobs(tx: &Transaction<'_>) -> AppResult<()> {
        let total: i64 = tx.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))?;
        if (total as usize) <= MAX_JOBS_TOTAL {
            return Ok(());
        }
        tx.execute(
            "DELETE FROM jobs WHERE id IN (
                 SELECT id FROM jobs
                 WHERE status NOT IN ('running', 'queued')
                 ORDER BY ord ASC, rowid DESC
                 LIMIT -1 OFFSET ?1
             )",
            params![MAX_JOBS_FINISHED as i64],
        )?;
        Ok(())
    }
}

pub const LOCALES: [(&str, &str); 20] = [
    ("en", "English"),
    ("ko", "Korean"),
    ("ja", "Japanese"),
    ("zh-CN", "Simplified Chinese (Mainland China)"),
    ("zh-TW", "Traditional Chinese (Taiwan)"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("pt-BR", "Brazilian Portuguese"),
    ("id", "Indonesian"),
    ("it", "Italian"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("tr", "Turkish"),
    ("ru", "Russian"),
    ("uk", "Ukrainian"),
    ("vi", "Vietnamese"),
    ("th", "Thai"),
    ("ar", "Modern Standard Arabic"),
    ("hi", "Hindi"),
];
pub fn locale_or_default(locale: &str) -> &str {
    let mut choices: Vec<_> = locale
        .split(',')
        .filter_map(|entry| {
            let mut fields = entry.trim().split(';');
            let tag = fields.next()?.trim();
            let quality = fields
                .find_map(|part| {
                    part.trim()
                        .strip_prefix("q=")
                        .and_then(|v| v.parse::<f32>().ok())
                })
                .unwrap_or(1.0);
            (quality.is_finite() && quality > 0.0 && quality <= 1.0).then_some((tag, quality))
        })
        .collect();
    choices.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (tag, _) in choices {
        if let Some((id, _)) = LOCALES.iter().find(|(id, _)| id.eq_ignore_ascii_case(tag)) {
            return id;
        }
        let base = tag.split('-').next().unwrap_or(tag);
        if let Some((id, _)) = LOCALES.iter().find(|(id, _)| id.eq_ignore_ascii_case(base)) {
            return id;
        }
        if base.eq_ignore_ascii_case("pt") {
            return "pt-BR";
        }
    }
    "en"
}
pub fn language_name(locale: &str) -> &str {
    LOCALES
        .iter()
        .find(|(id, _)| *id == locale)
        .map(|(_, name)| *name)
        .unwrap_or("English")
}
