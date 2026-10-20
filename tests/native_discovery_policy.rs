use hycli::{runtime::Runtime, state::StateStore, util};
use serde_json::json;
use std::path::PathBuf;

fn root() -> PathBuf {
    std::env::var_os("HYCLI_TEST_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/tests"))
        .join(util::id())
}

fn set_discovery(store: &StateStore, enabled: bool) {
    store.update(|data| {
        let mut value = serde_json::to_value(&data.settings)?;
        value["native_browser_discovery"] = json!(enabled);
        data.settings = serde_json::from_value(value)?;
        Ok(())
    }).unwrap();
}

#[test]
fn native_discovery_policy_migrates_old_settings_without_enabling_macos() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("state.json");
    let connection = rusqlite::Connection::open(path.with_extension("db")).unwrap();
    connection.execute_batch("CREATE TABLE settings (id INTEGER PRIMARY KEY, locale TEXT NOT NULL, theme TEXT NOT NULL, reduce_motion INTEGER NOT NULL, notifications INTEGER NOT NULL, default_provider TEXT NOT NULL, engine_path TEXT NOT NULL); INSERT INTO settings VALUES (1, 'ko', 'dark', 0, 0, '', '');").unwrap();
    drop(connection);
    let store = StateStore::open(&path).unwrap();
    let value = serde_json::to_value(store.settings().unwrap()).unwrap();
    assert_eq!(value["native_browser_discovery"], json!(!cfg!(target_os = "macos")));
    assert_eq!(value["locale"], "ko");
    assert_eq!(value["theme"], "dark");
}

#[test]
fn native_discovery_policy_is_visible_to_already_open_process_connections() {
    let path = root().join("state.json");
    let first = StateStore::open(&path).unwrap();
    let second = StateStore::open(&path).unwrap();
    for enabled in [false, true, false] {
        set_discovery(&first, enabled);
        assert_eq!(serde_json::to_value(second.settings().unwrap()).unwrap()["native_browser_discovery"], json!(enabled));
    }
}

#[tokio::test]
async fn disabled_native_discovery_rejects_before_scan_and_preserves_accounts() {
    let core = Runtime::open(&root(), None, None, false, true, false).unwrap();
    set_discovery(&core.state, false);
    let before = serde_json::to_value(core.state.read().unwrap()).unwrap();
    let result = core.find_browser_accounts("", "http://127.0.0.1/", "").await;
    match result {
        Err(error) => assert_eq!(error.public_code, "native_discovery_disabled"),
        Ok(_) => panic!("Disabled native discovery must reject before native work"),
    }
    assert_eq!(before, serde_json::to_value(core.state.read().unwrap()).unwrap());
    set_discovery(&core.state, true);
    let result = core.find_browser_accounts("", "http://127.0.0.1/", "").await.unwrap();
    assert_eq!(result.profiles_checked, 0);
}
