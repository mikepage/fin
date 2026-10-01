mod backup;
mod camt;
pub mod cli;
mod locale;
mod store;

use std::sync::Mutex;

use fin_shared::{
    Account, BackupInfo, Category, CategoryKind, Contract, CopyResult, Dataset, ImportFile, ImportProgress, ImportResult, Lang,
    RuleKind, RuleResult, TransactionInput,
};
use store::Store;
use tauri::ipc::Channel;
use tauri::{Manager, State};

type AppStore = Mutex<Store>;
type CmdResult<T> = Result<T, String>;

/// Locks the store, first picking up changes the CLI wrote to the file.
fn lock<'a>(state: &'a State<AppStore>) -> CmdResult<std::sync::MutexGuard<'a, Store>> {
    let mut s = state.lock().map_err(|_| "Internal error (lock)")?;
    s.reload_if_changed()?;
    Ok(s)
}

fn mutate(state: &State<AppStore>, f: impl FnOnce(&mut Dataset) -> CmdResult<()>) -> CmdResult<Dataset> {
    let mut s = lock(state)?;
    s.mutate(f)?;
    Ok(s.data.clone())
}

#[tauri::command]
fn get_data(state: State<AppStore>) -> CmdResult<Dataset> {
    Ok(lock(&state)?.data.clone())
}

/// The app's language, stored in the data file; standard category names follow it.
#[tauri::command]
fn set_language(state: State<AppStore>, lang: Lang) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::set_language(ds, lang))
}

#[tauri::command]
fn add_account(state: State<AppStore>, name: String, iban: Option<String>) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::add_account(ds, &name, iban))
}

#[tauri::command]
fn update_account(state: State<AppStore>, account: Account) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::update_account(ds, account))
}

#[tauri::command]
fn delete_account(state: State<AppStore>, id: String) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::delete_account(ds, &id))
}

#[tauri::command]
fn add_category(state: State<AppStore>, name: String, group: String, kind: CategoryKind) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::add_category(ds, &name, &group, kind))
}

#[tauri::command]
fn update_category(state: State<AppStore>, category: Category) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::update_category(ds, category))
}

#[tauri::command]
fn delete_category(state: State<AppStore>, id: String) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::delete_category(ds, &id))
}

/// Adds an IBAN or text rule; fills in matching transactions still to be categorised.
#[tauri::command]
fn add_category_rule(state: State<AppStore>, category_id: String, kind: RuleKind, value: String) -> CmdResult<RuleResult> {
    let mut s = lock(&state)?;
    let applied = s.mutate(|ds| store::add_category_rule(ds, &category_id, kind, &value))?;
    Ok(RuleResult { applied, data: s.data.clone() })
}

/// Applies a rule to all matching transactions from `from` on, overriding categories.
#[tauri::command]
fn apply_category_rule(
    state: State<AppStore>,
    category_id: String,
    kind: RuleKind,
    value: String,
    from: String,
) -> CmdResult<RuleResult> {
    let mut s = lock(&state)?;
    let applied = s.mutate(|ds| store::apply_category_rule(ds, &category_id, kind, &value, &from))?;
    Ok(RuleResult { applied, data: s.data.clone() })
}

#[tauri::command]
fn remove_category_rule(state: State<AppStore>, category_id: String, kind: RuleKind, value: String) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::remove_category_rule(ds, &category_id, kind, &value))
}

/// All rules with whether each is a (locked) default.
#[tauri::command]
fn list_rules(state: State<AppStore>) -> CmdResult<Vec<fin_shared::RuleInfo>> {
    Ok(store::list_rules(&lock(&state)?.data))
}

#[tauri::command]
fn save_transaction(state: State<AppStore>, input: TransactionInput) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::save_transaction(ds, input))
}

/// An account's bank balance at the end of `date`; `None` removes it.
#[tauri::command]
fn set_account_balance(state: State<AppStore>, id: String, date: String, cents: Option<i64>) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::set_account_balance(ds, &id, &date, cents))
}

/// Adds a contract (empty id) or replaces it.
#[tauri::command]
fn save_contract(state: State<AppStore>, contract: Contract) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::save_contract(ds, contract).map(|_| ()))
}

#[tauri::command]
fn delete_contract(state: State<AppStore>, id: String) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::delete_contract(ds, &id))
}

/// Splits a transaction into parts per category (amounts without sign); empty removes it.
#[tauri::command]
fn set_splits(state: State<AppStore>, id: String, parts: Vec<(i64, String)>) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::set_splits(ds, &id, parts))
}

/// The day a transaction counts on (`None` = its bank date).
#[tauri::command]
fn set_counts_on(state: State<AppStore>, id: String, day: Option<String>) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::set_counts_on(ds, &id, day))
}

#[tauri::command]
fn delete_transaction(state: State<AppStore>, id: String) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::delete_transaction(ds, &id))
}

#[tauri::command]
fn set_budget(
    state: State<AppStore>,
    category_id: String,
    month: String,
    amount_cents: Option<i64>,
) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::set_budget(ds, &category_id, &month, amount_cents))
}

#[tauri::command]
fn set_group_budget(
    state: State<AppStore>,
    group: String,
    kind: CategoryKind,
    month: String,
    amount_cents: Option<i64>,
) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::set_group_budget(ds, &group, kind, &month, amount_cents))
}

#[tauri::command]
fn copy_budgets_from_previous_year(state: State<AppStore>, year: i32) -> CmdResult<CopyResult> {
    let mut s = lock(&state)?;
    let copied = s.mutate(|ds| store::copy_budgets_from_previous_year(ds, year))?;
    Ok(CopyResult { copied, data: s.data.clone() })
}

/// Fills a year's budgets from the average over the last `months` months up to `lastMonth`.
#[tauri::command]
fn budgets_from_average(
    state: State<AppStore>,
    year: i32,
    last_month: String,
    months: u32,
    overwrite: bool,
) -> CmdResult<CopyResult> {
    let mut s = lock(&state)?;
    let copied = s.mutate(|ds| store::budgets_from_average(ds, year, &last_month, months, overwrite))?;
    Ok(CopyResult { copied, data: s.data.clone() })
}

#[tauri::command]
fn copy_budget_month_to_next(state: State<AppStore>, month: String) -> CmdResult<Dataset> {
    mutate(&state, |ds| store::copy_budget_month_to_next(ds, &month))
}

/// Parses all files first, then applies and saves them in one write, so a bad file
/// imports nothing. Async so it runs off the main thread and progress can stream.
#[tauri::command]
async fn import_camt053(
    state: State<'_, AppStore>,
    account_id: String,
    files: Vec<ImportFile>,
    on_progress: Channel<ImportProgress>,
) -> CmdResult<ImportResult> {
    let file_count = files.len();
    let send = |file_index: usize, file_name: &str, fraction: f64| {
        let _ = on_progress.send(ImportProgress {
            file_index,
            file_count,
            file_name: file_name.to_string(),
            fraction,
        });
    };

    let mut parsed = Vec::new();
    for (i, file) in files.iter().enumerate() {
        send(i, &file.name, 0.0);
        let statements = camt::parse_with_progress(&file.xml, |f| send(i, &file.name, f))
            .map_err(|e| format!("{}: {e}", file.name))?;
        parsed.push(store::ParsedFile { name: file.name.clone(), statements });
    }

    send(file_count, "", 0.0);
    let mut s = lock(&state)?;
    s.backup(backup::BEFORE_IMPORT)?;
    let at = backup::rfc3339(backup::now_secs());
    let record = s.mutate(|ds| store::apply_import(ds, &account_id, parsed, &at))?;
    send(file_count, "", 1.0);
    let latest_date = s
        .data
        .transactions
        .iter()
        .filter(|t| t.import_id.as_deref() == Some(record.id.as_str()))
        .map(|t| t.date.clone())
        .max();
    Ok(ImportResult {
        imported: record.imported,
        skipped_duplicates: record.skipped_duplicates,
        classified: record.classified,
        latest_date,
        data: s.data.clone(),
    })
}

#[tauri::command]
fn undo_import(state: State<AppStore>, id: String) -> CmdResult<Dataset> {
    let at = backup::rfc3339(backup::now_secs());
    mutate(&state, |ds| store::undo_import(ds, &id, &at).map(|_| ()))
}

#[tauri::command]
fn list_backups(state: State<AppStore>) -> CmdResult<Vec<BackupInfo>> {
    lock(&state)?.list_backups()
}

#[tauri::command]
fn create_backup(state: State<AppStore>) -> CmdResult<Vec<BackupInfo>> {
    let s = lock(&state)?;
    s.backup(backup::MANUAL)?.ok_or("Nothing to back up yet")?;
    s.list_backups()
}

#[tauri::command]
fn restore_backup(state: State<AppStore>, name: String) -> CmdResult<Dataset> {
    let mut s = lock(&state)?;
    s.restore_backup(&name)?;
    Ok(s.data.clone())
}

/// Restores from a backup file the user picked (its contents, read by the frontend).
#[tauri::command]
fn restore_backup_file(state: State<AppStore>, json: String) -> CmdResult<Dataset> {
    let mut s = lock(&state)?;
    s.restore_bytes(json.as_bytes())?;
    Ok(s.data.clone())
}

/// Writes a copy of the current data to the Downloads folder; returns the path.
#[tauri::command]
fn export_backup<R: tauri::Runtime>(app: tauri::AppHandle<R>, state: State<AppStore>) -> CmdResult<String> {
    let bytes = lock(&state)?.export_json()?;
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    let stamp = backup::rfc3339(backup::now_secs()).replace([':', '-'], "");
    let path = dir.join(format!("fin-backup-{stamp}.json"));
    std::fs::write(&path, bytes).map_err(|e| format!("Save failed: {e}"))?;
    Ok(path.display().to_string())
}

/// Saves a report as CSV in the Downloads folder: `name` (letters, digits, `-`, `_` and
/// spaces; anything else becomes `-`) plus `.csv`. Returns the path.
#[tauri::command]
fn export_csv<R: tauri::Runtime>(app: tauri::AppHandle<R>, name: String, content: String) -> CmdResult<String> {
    let name: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ') { c } else { '-' }).collect();
    let name = name.trim();
    if name.is_empty() {
        return Err("Invalid file name".into());
    }
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    let path = dir.join(format!("{name}.csv"));
    std::fs::write(&path, content).map_err(|e| format!("Save failed: {e}"))?;
    Ok(path.display().to_string())
}

fn with_commands<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
            get_data,
            set_language,
            add_account,
            update_account,
            delete_account,
            add_category,
            update_category,
            delete_category,
            add_category_rule,
            apply_category_rule,
            remove_category_rule,
            list_rules,
            save_transaction,
            delete_transaction,
            set_counts_on,
            set_splits,
            set_account_balance,
            save_contract,
            delete_contract,
            import_camt053,
            undo_import,
            list_backups,
            create_backup,
            restore_backup,
            restore_backup_file,
            export_backup,
            export_csv,
            set_budget,
            set_group_budget,
            copy_budgets_from_previous_year,
            copy_budget_month_to_next,
            budgets_from_average,
        ])
}

// generate_context! may only expand once per crate on macOS (it embeds Info.plist).
fn context<R: tauri::Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    with_commands(tauri::Builder::default())
        .setup(|app| {
            // Same file the CLI uses (cli::data_file), unless FIN_DATA_FILE points elsewhere.
            let path = match std::env::var_os("FIN_DATA_FILE") {
                Some(p) => p.into(),
                None => app.path().app_data_dir()?.join("fin.json"),
            };
            app.manage(Mutex::new(Store::load(path)?));
            Ok(())
        })
        .run(context())
        .expect("error while running tauri application");
}

/// Drives the real commands through Tauri's IPC layer, so argument names, the
/// capability file and the command registration are checked, not just the logic.
#[cfg(test)]
mod ipc_tests {
    use super::*;
    use serde_json::{json, Value};
    use tauri::test::{get_ipc_response, mock_builder, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tauri::WebviewWindowBuilder;

    fn call(w: &tauri::WebviewWindow<tauri::test::MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
        get_ipc_response(
            w,
            InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|b| b.deserialize::<Value>().unwrap())
    }

    #[test]
    fn full_flow_over_ipc() {
        let dir = std::env::temp_dir().join(format!("fin-ipc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("fin.json");

        let store = Store::load(file.clone()).unwrap();
        let app = with_commands(mock_builder()).manage(Mutex::new(store)).build(context()).unwrap();
        let w = WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();

        let ds = call(&w, "get_data", json!({})).unwrap();
        assert_eq!(ds["categories"].as_array().unwrap().len(), fin_shared::catalog::CATALOG.len());
        assert_eq!(ds["categories"][0]["kind"], "Income");

        let ds = call(&w, "add_account", json!({"name": "Betaal", "iban": null})).unwrap();
        let acc = ds["accounts"][0]["id"].as_str().unwrap().to_string();
        let cat = ds["categories"][0]["id"].as_str().unwrap().to_string();

        call(&w, "save_transaction", json!({"input": {
            "id": null, "date": "2026-09-10", "description": "Koffie",
            "amount_cents": -350, "category_id": cat, "account_id": acc,
        }}))
        .unwrap();

        let files = json!([{"name": "sept.xml", "xml": camt::SAMPLE_V02}]);
        let r = call(&w, "import_camt053", json!({
            "accountId": acc, "files": files, "onProgress": "__CHANNEL__:7",
        }))
        .unwrap();
        assert_eq!(r["imported"], 2);
        let r = call(&w, "import_camt053", json!({
            "accountId": acc, "files": files, "onProgress": "__CHANNEL__:8",
        }))
        .unwrap();
        assert_eq!((r["imported"].as_u64(), r["skipped_duplicates"].as_u64()), (Some(0), Some(2)));

        let bad = call(&w, "import_camt053", json!({
            "accountId": acc, "files": [{"name": "x.xml", "xml": "<nope/>"}], "onProgress": "__CHANNEL__:9",
        }));
        assert!(bad.is_err());

        let ds = call(&w, "set_budget", json!({"categoryId": cat, "month": "2025-12", "amountCents": 45000})).unwrap();
        assert_eq!(ds["budgets"][0]["amount_cents"], 45000);
        assert!(call(&w, "set_budget", json!({"categoryId": cat, "month": "2025-13", "amountCents": 1})).is_err());
        let groceries = json!({"categoryId": fin_shared::catalog::ids::GROCERIES, "month": "2025-12", "amountCents": 100});
        assert!(call(&w, "set_budget", groceries).is_err(), "variable costs are budgeted per group");
        let r = call(&w, "copy_budgets_from_previous_year", json!({"year": 2026})).unwrap();
        assert_eq!(r["copied"], 1);
        assert_eq!(r["data"]["budgets"][1]["month"], "2026-12");
        let ds = call(&w, "copy_budget_month_to_next", json!({"month": "2026-12"})).unwrap();
        assert_eq!(ds["budgets"].as_array().unwrap().len(), 3);
        let ds = call(&w, "set_budget", json!({"categoryId": cat, "month": "2025-12", "amountCents": null})).unwrap();
        assert_eq!(ds["budgets"].as_array().unwrap().len(), 2);
        let g = json!({"group": "Vervoer", "kind": "Variable", "month": "2026-05", "amountCents": 11500});
        let ds = call(&w, "set_group_budget", g).unwrap();
        assert_eq!(ds["group_budgets"][0]["amount_cents"], 11500);
        assert!(call(&w, "set_group_budget", json!({"group": "Vervoer", "kind": "Income", "month": "2026-05", "amountCents": 100})).is_err());
        assert!(call(&w, "set_group_budget", json!({"group": "Financiën", "kind": "Fixed", "month": "2026-05", "amountCents": 100})).is_err());

        // Persisted to disk: a fresh store sees all three transactions.
        let reloaded = Store::load(file).unwrap();
        assert_eq!(reloaded.data.transactions.len(), 3);
        assert_eq!(reloaded.data.budget_for(&cat, "2027-01"), Some(45000));
        assert_eq!(reloaded.data.accounts[0].iban.as_deref(), Some("NL91ABNA0417164300"));

        // Rules: the imported salary (counterparty NL20INGB…) waits uncategorised until a rule.
        let salary = fin_shared::catalog::ids::SALARY;
        let food = fin_shared::catalog::ids::GROCERIES;
        let iban = json!({"categoryId": salary, "kind": "Iban", "value": "NL20INGB0001234567"});
        assert_eq!(call(&w, "add_category_rule", iban.clone()).unwrap()["applied"], 1);
        let mut year = iban.clone();
        year["from"] = json!("2026-01-01");
        assert_eq!(call(&w, "apply_category_rule", year).unwrap()["applied"], 0, "already in the category");
        call(&w, "remove_category_rule", iban).unwrap();
        // Albert Heijn was already sorted on import by the default supermarket rule.
        let ds = call(&w, "get_data", json!({})).unwrap();
        let ah = ds["transactions"].as_array().unwrap().iter().find(|t| t["description"].as_str().unwrap().starts_with("Albert")).unwrap();
        assert_eq!(ah["category_id"], food);
        let text = json!({"categoryId": food, "kind": "Text", "value": "Albert Heijn"});
        assert_eq!(call(&w, "add_category_rule", text).unwrap()["applied"], 0, "nothing left to sort");
        assert!(call(&w, "delete_category", json!({"id": salary})).is_err(), "system category");

        // Budgets from the average of the imported September.
        let r = call(&w, "budgets_from_average", json!({"year": 2027, "lastMonth": "2026-09", "months": 1, "overwrite": false}))
            .unwrap();
        // Salary gets February–December (January 2027 already had a budget, copied above);
        // Huishouden (Groceries, filled in by the text rule) gets all twelve months.
        assert_eq!(r["copied"], 23);

        // Import log, undo and backups.
        let ds = call(&w, "get_data", json!({})).unwrap();
        let first_import = ds["imports"].as_array().unwrap().last().unwrap()["id"].as_str().unwrap().to_string();
        let before = ds["transactions"].as_array().unwrap().len();
        let ds = call(&w, "undo_import", json!({"id": first_import})).unwrap();
        assert_eq!(ds["transactions"].as_array().unwrap().len(), before - 2);
        let backups = call(&w, "create_backup", json!({})).unwrap();
        let names: Vec<&str> = backups.as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap()).collect();
        assert!(names.iter().any(|n| n.ends_with("-handmatig.json")));
        assert!(names.iter().any(|n| n.ends_with("-voor-import.json")));
        let ds = call(&w, "restore_backup", json!({"name": names.last().unwrap()})).unwrap();
        assert!(ds["categories"].as_array().unwrap().len() >= fin_shared::catalog::CATALOG.len());
        assert_eq!(call(&w, "list_backups", json!({})).unwrap().as_array().unwrap().len(), names.len() + 1);
        assert!(call(&w, "restore_backup_file", json!({"json": "nope"})).is_err());

        // The language is kept in the data file; standard names follow it.
        let ds = call(&w, "set_language", json!({"lang": "en"})).unwrap();
        assert_eq!(ds["language"], "en");
        let name = |ds: &Value, id: &str| ds["categories"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap()["name"].clone();
        assert_eq!(name(&ds, food), "Groceries");
        assert!(call(&w, "set_language", json!({"lang": "fr"})).is_err());
        let ds = call(&w, "set_language", json!({"lang": "nl-NL"})).unwrap();
        assert_eq!(name(&ds, food), "Boodschappen");

        // A command outside the app's list is rejected.
        assert!(call(&w, "plugin:opener|open_url", json!({"url": "https://example.com"})).is_err());
    }
}
