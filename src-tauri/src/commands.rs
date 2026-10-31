//! Tauri layer over `Engine`: app paths, the background scheduler and file watcher, the
//! `scan-progress` / `data-updated` events, and the commands the popover calls.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, FixedOffset, Local, Utc};
use serde::Serialize;
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use tauri::async_runtime::{self, JoinHandle};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, WindowEvent};

use crate::aggregate::{HistoryView, ModelsView, Overview, Period, ProjectsView, SessionFilter, SessionPage};
use crate::engine::{self, is_due, thresholds, Engine, Roots, ScanStatus, Trigger};
use crate::i18n;
use crate::ingest::cursor as cursor_ingest;
use crate::pricing::{self, Pricing};
use crate::quota::claude::{fetch, KeychainSource, QuotaError};
use crate::quota::cursor as cursor_quota;
use crate::quota::openrouter as openrouter_quota;
use crate::rtk;
use crate::settings::{Lang, Settings};
use crate::share::{self, ShareRequest};
use crate::store::Store;

pub const SCAN_PROGRESS: &str = "scan-progress";
pub const DATA_UPDATED: &str = "data-updated";
/// Sent to the settings window right before it is shown, so the page resets its form.
pub const SETTINGS_OPENED: &str = "settings-opened";

const POPOVER: &str = "popover";
const SETTINGS_WINDOW: &str = "settings";
const POPOVER_WIDTH: f64 = 380.0;
const POPOVER_MIN_HEIGHT: f64 = 200.0;
const POPOVER_MAX_HEIGHT: f64 = 720.0;
/// How often the scheduler checks whether a scan or a quota poll is due.
const TICK: Duration = Duration::from_secs(5);
const WATCH_DEBOUNCE: Duration = Duration::from_secs(2);
/// Pause between history pages, so the first sync (about 50 pages) stays polite.
const CURSOR_PAGE_PAUSE: Duration = Duration::from_secs(1);
const RTK_EVERY: Duration = Duration::from_secs(60);
/// `scan-progress` is sent at most this often, plus the first and the last step.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

pub struct AppPaths {
    pub settings: PathBuf,
    pub prices: PathBuf,
    pub prices_version: PathBuf,
}

/// Shared by the scheduler thread and the commands.
struct Background {
    tx: Sender<Trigger>,
    http: reqwest::Client,
    quota_busy: AtomicBool,
    /// Created once for Cursor (no redirects, 15 s timeout); never shared with Claude's client.
    cursor_http: reqwest::Client,
    cursor_busy: AtomicBool,
    /// Held around check-then-save in connect and around clear-then-end in disconnect, so a
    /// Putuskan can never be undone by a Hubungkan that was still waiting for the server.
    cursor_keychain: Mutex<()>,
    /// OpenRouter's own client and busy flag; never shared with Cursor's or Claude's.
    openrouter_http: reqwest::Client,
    openrouter_busy: AtomicBool,
    openrouter_keychain: Mutex<()>,
}

/// Opens the store, loads settings and prices, registers the state, and starts the watcher and
/// the scheduler (which runs the first scan right away).
pub fn init(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data = app.path().app_data_dir()?;
    let home = app.path().home_dir()?;
    std::fs::create_dir_all(&data)?;
    let db = data.join("buku.db");
    let store = Store::open(&db).or_else(|e| {
        eprintln!("[buku-token] cannot open {}: {e:#}; using an in-memory store", db.display());
        Store::open_in_memory()
    })?;
    let paths = AppPaths {
        settings: app.path().app_config_dir()?.join("settings.json"),
        prices: data.join("model_prices.json"),
        prices_version: data.join("model_prices.VERSION"),
    };
    let engine = Engine::new(
        store,
        engine::load_pricing(&paths.prices, &paths.prices_version),
        crate::settings::load(&paths.settings),
        Roots::from_home(&home),
        home,
        app.path().app_cache_dir()?.join("snapshots"),
    );

    let (tx, rx) = mpsc::channel();
    let watcher = watch(engine.roots(), tx.clone());
    if let Some(win) = app.get_webview_window(POPOVER) {
        let tx = tx.clone();
        win.on_window_event(move |e| {
            if let WindowEvent::Focused(true) = e {
                let _ = tx.send(Trigger::PopoverOpened);
            }
        });
    }
    // Closing the settings window only hides it, so `open_settings` can show it again.
    if let Some(win) = app.get_webview_window(SETTINGS_WINDOW) {
        let w = win.clone();
        win.on_window_event(move |e| {
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
            }
        });
    }
    app.manage(engine);
    app.manage(paths);
    app.manage(Background {
        tx,
        http: reqwest::Client::new(),
        quota_busy: AtomicBool::new(false),
        cursor_http: cursor_quota::http_client(),
        cursor_busy: AtomicBool::new(false),
        cursor_keychain: Mutex::new(()),
        openrouter_http: openrouter_quota::http_client(),
        openrouter_busy: AtomicBool::new(false),
        openrouter_keychain: Mutex::new(()),
    });

    let handle = app.handle().clone();
    std::thread::Builder::new().name("buku-token-scan".into()).spawn(move || {
        let _watcher = watcher; // watching stops when it is dropped
        schedule(&handle, rx);
    })?;
    Ok(())
}

/// Debounced watch on the log roots that exist. A missing root is simply not watched; the
/// fallback scan still picks it up once the agent is installed.
fn watch(roots: &Roots, tx: Sender<Trigger>) -> Option<Debouncer<RecommendedWatcher>> {
    let filter = roots.clone();
    let mut debouncer = new_debouncer(WATCH_DEBOUNCE, move |res: DebounceEventResult| {
        if res.is_ok_and(|evs| evs.iter().any(|e| filter.is_relevant(&e.path))) {
            let _ = tx.send(Trigger::FilesChanged);
        }
    })
    .inspect_err(|e| eprintln!("[buku-token] file watcher unavailable: {e}"))
    .ok()?;
    for (path, recursive) in roots.watch_targets() {
        let mode = if recursive { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };
        if let Err(e) = debouncer.watcher().watch(&path, mode) {
            eprintln!("[buku-token] cannot watch {}: {e}", path.display());
        }
    }
    Some(debouncer)
}

/// Runs scans on this thread and starts quota polls whenever a trigger makes them due.
fn schedule(app: &AppHandle, rx: Receiver<Trigger>) {
    let mut want = thresholds(Trigger::Refresh);
    loop {
        let engine = app.state::<Engine>();
        if is_due(engine.quota_age(), want.1) {
            poll_claude_quota(app, false);
        }
        if is_due(engine.cursor_age(), want.1) {
            poll_cursor(app);
        }
        if is_due(engine.openrouter_age(), want.1) {
            poll_openrouter(app);
        }
        if is_due(engine.scan_age(), want.0) {
            run_scan(app);
        }
        want = match rx.recv_timeout(TICK) {
            Ok(t) => thresholds(t),
            Err(RecvTimeoutError::Timeout) => thresholds(Trigger::Tick),
            Err(RecvTimeoutError::Disconnected) => return,
        };
        // Merge whatever queued up meanwhile (e.g. watcher events during a long first scan).
        while let Ok(t) = rx.try_recv() {
            let (scan, quota) = thresholds(t);
            want = (want.0.min(scan), want.1.min(quota));
        }
    }
}

fn run_scan(app: &AppHandle) {
    let engine = app.state::<Engine>();
    let before = engine.version();
    let started = Instant::now();
    let last_sent = Cell::new(None::<Instant>);
    let res = engine.scan_all(|p| {
        if p.done == p.total || last_sent.get().is_none_or(|t| t.elapsed() >= PROGRESS_EVERY) {
            last_sent.set(Some(Instant::now()));
            let _ = app.emit(SCAN_PROGRESS, p);
        }
    });
    if is_due(engine.rtk_age(), RTK_EVERY) {
        engine.set_rtk(rtk::read_today(Local::now().date_naive()));
    }
    match res {
        Ok(s) if s.events > 0 || s.skipped > 0 => {
            let st = engine.status();
            eprintln!(
                "[buku-token] scan: {}/{} sources, {} events read, {} lines skipped, {} errors, {:.1?}",
                st.done,
                st.total,
                s.events,
                s.skipped,
                st.errors,
                started.elapsed()
            );
        }
        Ok(_) => {}
        Err(e) => eprintln!("[buku-token] scan failed: {e:#}"),
    }
    if cfg!(debug_assertions) {
        eprintln!("[buku-token] scan pass: {:.1?}", started.elapsed());
    }
    if engine.version() != before {
        let _ = app.emit(DATA_UPDATED, ());
    }
}

/// Starts a Claude quota poll on a blocking thread, unless one is running or it is not wanted.
fn poll_claude_quota(app: &AppHandle, force: bool) -> Option<JoinHandle<()>> {
    if !app.state::<Engine>().wants_claude_quota(force) || app.state::<Background>().quota_busy.swap(true, Ordering::SeqCst) {
        return None;
    }
    let app = app.clone();
    Some(async_runtime::spawn_blocking(move || {
        let bg = app.state::<Background>();
        // `fetch` reads the Keychain synchronously, and that read can wait on a permission
        // dialog, so the whole poll stays off the async workers.
        let res = async_runtime::block_on(fetch(&KeychainSource, &bg.http));
        if let Err(e) = &res {
            eprintln!("[buku-token] Claude quota unavailable: {e}");
        }
        if let Err(e) = app.state::<Engine>().set_claude_quota(res) {
            eprintln!("[buku-token] cannot store quota samples: {e:#}");
        }
        bg.quota_busy.store(false, Ordering::SeqCst);
        let _ = app.emit(DATA_UPDATED, ());
    }))
}

/// Starts a Cursor round (quota, then history) on a blocking thread, unless one is running or
/// Cursor is not wanted (not onboarded or agent off: nothing is read or sent) or the Keychain
/// refused our item (only Hubungkan or Putuskan asks again).
/// It has its own busy flag, so a long first sync never delays Claude's poll or the scan.
fn poll_cursor(app: &AppHandle) -> Option<JoinHandle<()>> {
    let engine = app.state::<Engine>();
    if !engine.wants_cursor() || engine.cursor_keychain_denied() || app.state::<Background>().cursor_busy.swap(true, Ordering::SeqCst) {
        return None;
    }
    let app = app.clone();
    Some(async_runtime::spawn_blocking(move || {
        async_runtime::block_on(cursor_round(&app));
        app.state::<Background>().cursor_busy.store(false, Ordering::SeqCst);
        let _ = app.emit(DATA_UPDATED, ());
    }))
}

/// Starts an OpenRouter poll on a blocking thread, unless one is running, OpenRouter is not
/// wanted (not onboarded or switched off: nothing is read or sent), the Keychain refused our
/// item (only Hubungkan or Putuskan asks again) or a failed poll is still resting.
fn poll_openrouter(app: &AppHandle) -> Option<JoinHandle<()>> {
    let engine = app.state::<Engine>();
    if !engine.wants_openrouter()
        || engine.openrouter_keychain_denied()
        || !engine.openrouter_due()
        || app.state::<Background>().openrouter_busy.swap(true, Ordering::SeqCst)
    {
        return None;
    }
    let app = app.clone();
    Some(async_runtime::spawn_blocking(move || {
        async_runtime::block_on(openrouter_round(&app));
        app.state::<Background>().openrouter_busy.store(false, Ordering::SeqCst);
        let _ = app.emit(DATA_UPDATED, ());
    }))
}

/// One OpenRouter poll. Only fixed reason texts are logged, never the key or a response body.
/// Without a stored key nothing is sent and there is no OpenRouter section.
async fn openrouter_round(app: &AppHandle) {
    let (engine, bg) = (app.state::<Engine>(), app.state::<Background>());
    // A disconnect or a new key after this point drops the answer (see `set_openrouter_at`).
    let epoch = engine.openrouter_epoch();
    let Some(res) = openrouter_quota::round(&openrouter_quota::OPENROUTER_KEY, &bg.openrouter_http, openrouter_quota::BASE_URL).await else {
        return engine.clear_openrouter_at(epoch);
    };
    let failure = res.as_ref().err().copied();
    if let Some(e) = &failure {
        eprintln!("[buku-token] OpenRouter unavailable: {}", openrouter_quota::reason(e, Lang::default()));
    }
    if engine.set_openrouter_at(epoch, res) {
        match failure {
            Some(e) => engine.mark_openrouter_failed(cursor_ingest::history_backoff(&e)),
            None => engine.clear_openrouter_backoff(),
        }
    }
}

/// One Cursor round. Only fixed reason texts are logged, never the cookie or a response body.
/// Without a stored cookie nothing is sent and Cursor has no quota row.
async fn cursor_round(app: &AppHandle) {
    let (engine, bg) = (app.state::<Engine>(), app.state::<Background>());
    let now = Utc::now();
    // A disconnect or a new cookie after this point ends the round (see `go` below).
    let epoch = engine.cursor_epoch();
    let summary = match cursor_quota::load_auth(&cursor_quota::CURSOR_SESSION) {
        Ok(Some(auth)) => cursor_quota::fetch_summary(&bg.cursor_http, &auth, now).await.map(|q| (auth, q)),
        Ok(None) => {
            engine.clear_cursor_quota();
            return;
        }
        Err(e) => Err(e),
    };
    if matches!(summary, Err(QuotaError::KeychainDenied)) {
        engine.set_cursor_keychain_denied(true);
    }
    let auth = match summary {
        Ok((auth, q)) => {
            if !engine.set_cursor_quota_at(epoch, Ok(q)) {
                return;
            }
            auth
        }
        Err(e) => {
            // No history call either: `fetch_page` has no expiry check, so a dead cookie stays home.
            eprintln!("[buku-token] Cursor quota unavailable: {}", cursor_quota::reason(&e, Lang::default()));
            engine.set_cursor_quota_at(epoch, Err(e));
            return;
        }
    };
    let _ = app.emit(DATA_UPDATED, ());

    // The quota above keeps its own cadence; only history rests after a failure.
    if !engine.cursor_history_due() {
        return;
    }
    let mut watermark = match engine.cursor_watermark() {
        Ok(w) => w,
        Err(e) => return eprintln!("[buku-token] cannot read the Cursor watermark: {e:#}"),
    };
    let now_ms = now.timestamp_millis();
    let projects = engine.cursor_project_map();
    let http = &bg.cursor_http;
    let auth = &auth;
    // `sink` cannot fail, so the first write error is kept; it stops further fetching.
    let write_error = RefCell::new(None::<String>);
    let stopped = Cell::new(false);
    // Checked before every request, retries included: switching Cursor off, a failed write, or a
    // disconnect / replaced cookie (epoch) ends the sync without sending this cookie again.
    let go = || write_error.borrow().is_none() && engine.wants_cursor() && engine.cursor_epoch() == epoch;
    let go = &go;
    let stopped = &stopped;
    // History goes in 30-day slices; the watermark is saved after each, so a failure late in a
    // long first sync costs one slice, not everything.
    loop {
        let (start, end, is_final) = cursor_ingest::next_slice(watermark, now_ms);
        let res = cursor_ingest::sync(
            |page| async move {
                cursor_ingest::retrying(
                    || {
                        let ok = go();
                        if !ok {
                            stopped.set(true);
                        }
                        async move {
                            if !ok {
                                return Err(QuotaError::Offline); // never shown: `stopped` is checked first
                            }
                            cursor_ingest::fetch_page(http, auth, cursor_quota::BASE_URL, start, end, page).await
                        }
                    },
                    tokio::time::sleep, // no lock is held while waiting
                    || {
                        let halt = !go();
                        if halt {
                            stopped.set(true);
                        }
                        halt
                    },
                )
                .await
            },
            &projects,
            |events| {
                if let Err(e) = engine.store_cursor_events(events) {
                    write_error.borrow_mut().get_or_insert_with(|| format!("{e:#}"));
                }
            },
            CURSOR_PAGE_PAUSE,
        )
        .await;
        if let Some(e) = write_error.borrow().as_deref() {
            eprintln!("[buku-token] cannot store Cursor history, will retry in an hour: {e}");
            engine.mark_cursor_history_failed(cursor_ingest::HISTORY_BACKOFF);
            return;
        }
        if stopped.get() {
            return eprintln!("[buku-token] Cursor history sync stopped; watermark not advanced");
        }
        let stats = match res {
            Ok(s) => s,
            Err(e) => {
                let rest = cursor_ingest::history_backoff(&e);
                engine.mark_cursor_history_failed(rest);
                return eprintln!(
                    "[buku-token] Cursor history sync stopped, will retry in {} minutes: {}",
                    rest.as_secs() / 60,
                    cursor_quota::reason(&e, Lang::default())
                );
            }
        };
        if stats.truncated {
            eprintln!("[buku-token] Cursor history hit the 200-page cap; older events are not fetched");
        }
        if let Some(next) = cursor_ingest::slice_watermark(watermark, end, is_final, &stats) {
            if let Err(e) = engine.set_cursor_watermark(next) {
                return eprintln!("[buku-token] cannot save the Cursor watermark: {e:#}");
            }
            watermark = next;
        }
        if is_final {
            engine.clear_cursor_history_backoff();
            return;
        }
    }
}

fn local_tz() -> FixedOffset {
    *Local::now().offset()
}

/// Runs `f` on a blocking thread: queries and scans touch SQLite and the file system.
async fn on_engine<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&Engine) -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    async_runtime::spawn_blocking(move || f(&app.state::<Engine>()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn get_overview(app: AppHandle) -> Result<Overview, String> {
    on_engine(app, |e| e.overview(local_tz(), Utc::now())).await
}

#[tauri::command]
pub async fn get_sessions(app: AppHandle, filter: SessionFilter) -> Result<SessionPage, String> {
    on_engine(app, move |e| e.sessions(filter, local_tz(), Utc::now())).await
}

#[tauri::command]
pub async fn get_models(app: AppHandle, period: Period) -> Result<ModelsView, String> {
    on_engine(app, move |e| e.models(period, local_tz(), Utc::now())).await
}

#[tauri::command]
pub async fn get_projects(app: AppHandle, period: Period) -> Result<ProjectsView, String> {
    on_engine(app, move |e| e.projects(period, local_tz(), Utc::now())).await
}

/// The language of the texts the user sees right now.
fn lang(app: &AppHandle) -> Lang {
    app.state::<Engine>().settings().language
}

/// The share card as PNG bytes, drawn on a blocking thread from the stored events. Errors are fixed
/// texts in the current language and never carry a file path.
async fn share_png(app: &AppHandle, request: ShareRequest) -> Result<Vec<u8>, String> {
    let app = app.clone();
    let language = lang(&app);
    let render_error = i18n::share_render_error(language);
    async_runtime::spawn_blocking(move || {
        let data = app.state::<Engine>().share_data(&request, local_tz(), Utc::now()).map_err(|e| {
            eprintln!("[buku-token] share: cannot read usage: {e:#}");
            render_error
        })?;
        share::render_png(&share::share_svg(&data, &request, language)).map_err(|e| {
            eprintln!("[buku-token] share: cannot render: {e}");
            render_error
        })
    })
    .await
    .map_err(|_| render_error.to_string())?
    .map_err(str::to_string)
}

/// The card as a `data:image/png;base64,...` URL for the preview. Nothing leaves the machine.
#[tauri::command]
pub async fn share_preview(app: AppHandle, request: ShareRequest) -> Result<String, String> {
    use base64::Engine as _;
    let png = share_png(&app, request).await?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
}

/// Puts the card on the clipboard as an image.
#[tauri::command]
pub async fn share_copy(app: AppHandle, request: ShareRequest) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let png = share_png(&app, request).await?;
    let copy_error = i18n::share_copy_error(lang(&app));
    // Decoding the PNG and the pasteboard write stay off the async workers.
    async_runtime::spawn_blocking(move || {
        let image = tauri::image::Image::from_bytes(&png).map_err(|_| copy_error.to_string())?;
        app.clipboard().write_image(&image).map_err(|e| {
            eprintln!("[buku-token] share: clipboard: {e}");
            copy_error.to_string()
        })
    })
    .await
    .map_err(|_| copy_error.to_string())?
}

/// Saves the card as `~/Downloads/buku-token-YYYY-MM-DD.png` (`-2`, `-3` when taken) and returns the file name.
#[tauri::command]
pub async fn share_save(app: AppHandle, request: ShareRequest) -> Result<String, String> {
    let png = share_png(&app, request).await?;
    let save_error = i18n::share_save_error(lang(&app));
    let dir = app.path().download_dir().map_err(|_| save_error.to_string())?;
    async_runtime::spawn_blocking(move || share::save_png(&dir, Local::now().date_naive(), &png))
        .await
        .map_err(|_| save_error.to_string())?
        .map_err(|e| {
            eprintln!("[buku-token] share: cannot save: {:?}", e.kind());
            save_error.to_string()
        })
}

#[tauri::command]
pub async fn get_history(app: AppHandle, days: u32) -> Result<HistoryView, String> {
    on_engine(app, move |e| e.history(days, local_tz(), Utc::now())).await
}

/// Scans now and re-polls the Claude quota (even after a refused Keychain prompt); returns
/// when both are done. Cursor's round starts too but runs on its own.
#[tauri::command]
pub async fn refresh(app: AppHandle) -> Result<(), String> {
    let quota = poll_claude_quota(&app, true);
    // An explicit refresh also ends a Cursor history backoff (a refused Keychain stays refused).
    app.state::<Engine>().clear_cursor_history_backoff();
    app.state::<Engine>().clear_openrouter_backoff();
    poll_openrouter(&app);
    // Not awaited: the first history sync takes a while, and it announces itself with `data-updated`.
    poll_cursor(&app);
    let scan_app = app.clone();
    async_runtime::spawn_blocking(move || run_scan(&scan_app)).await.map_err(|e| e.to_string())?;
    if let Some(q) = quota {
        q.await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<Settings, String> {
    Ok(app.state::<Engine>().settings())
}

/// Merges the given top-level fields (a partial `Settings` is fine) into the current settings,
/// saves them, and returns them as stored (sanitized the same way as at launch).
#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    settings: serde_json::Map<String, serde_json::Value>,
) -> Result<Settings, String> {
    let path = app.state::<AppPaths>().settings.clone();
    let (old, saved) = on_engine(app.clone(), move |e| {
        // Every writer goes through `update_settings`, which holds one lock across read, merge and write.
        e.update_settings(&path, |old| Ok(Some(crate::settings::merge(old, settings)?)))?.ok_or_else(|| anyhow::anyhow!("no settings"))
    })
    .await?;
    if old.enabled_agents != saved.enabled_agents
        || old.onboarded != saved.onboarded
        || old.openrouter_enabled != saved.openrouter_enabled
        || old.antigravity_enabled != saved.antigravity_enabled
    {
        // A newly enabled agent needs a scan; finishing onboarding allows the first quota poll.
        let _ = app.state::<Background>().tx.send(Trigger::Refresh);
    }
    if old.hide_on_screen_share != saved.hide_on_screen_share {
        crate::screen::sync(&app);
    }
    if (Settings { last_tab: saved.last_tab.clone(), ..old }) != saved {
        let _ = app.emit(DATA_UPDATED, ());
    }
    Ok(saved)
}

/// Sends one sample notification so the user can check that macOS shows it.
#[tauri::command]
pub async fn send_test_notification(app: AppHandle) {
    crate::notify::send_test(&app);
}

/// Downloads the latest LiteLLM price table, keeps it in the app data folder, re-prices stored
/// events, and returns the new version (the download date).
#[tauri::command]
pub async fn update_pricing(app: AppHandle) -> Result<String, String> {
    let json = pricing::download_latest().await.map_err(|e| format!("{e:#}"))?;
    let version = Local::now().format("%Y-%m-%d").to_string();
    let (json_path, version_path) = {
        let p = app.state::<AppPaths>();
        (p.prices.clone(), p.prices_version.clone())
    };
    let version = on_engine(app.clone(), move |e| {
        let table = Pricing::from_json_str(&json)?.with_version(&version);
        engine::save_pricing(&json_path, &version_path, &json, &version)?;
        e.set_pricing(table)?;
        Ok(version)
    })
    .await?;
    let _ = app.emit(DATA_UPDATED, ());
    Ok(version)
}

#[tauri::command]
pub async fn resize_popover(app: AppHandle, height: f64) -> Result<(), String> {
    if height.is_nan() {
        return Err("height is not a number".into());
    }
    let win = app.get_webview_window(POPOVER).ok_or("popover window not found")?;
    let size = LogicalSize::new(POPOVER_WIDTH, height.clamp(POPOVER_MIN_HEIGHT, POPOVER_MAX_HEIGHT));
    win.set_size(size).map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorSessionStatus {
    pub connected: bool,
    pub expires_at: Option<DateTime<Utc>>,
    /// The Keychain refused our item; nothing reads it again until Hubungkan or Putuskan.
    pub keychain_denied: bool,
    /// Why the last quota poll failed (a fixed text), shown next to "Terhubung".
    pub last_reason: Option<String>,
    pub history_failed: bool,
}

impl CursorSessionStatus {
    fn disconnected() -> Self {
        CursorSessionStatus { connected: false, expires_at: None, keychain_denied: false, last_reason: None, history_failed: false }
    }
}

/// What the engine remembers about Cursor, passed in so the status can be built without it.
struct CursorMemory {
    keychain_denied: bool,
    last_reason: Option<String>,
    history_failed: bool,
}

/// Does not read the Keychain once it was refused. Carries no cookie, only fixed texts.
fn session_status(store: &dyn cursor_quota::SessionStore, mem: CursorMemory) -> CursorSessionStatus {
    if mem.keychain_denied {
        return CursorSessionStatus { keychain_denied: true, ..CursorSessionStatus::disconnected() };
    }
    match cursor_quota::load_auth(store) {
        Ok(Some(auth)) => CursorSessionStatus {
            connected: true,
            expires_at: auth.expires_at,
            keychain_denied: false,
            last_reason: mem.last_reason,
            history_failed: mem.history_failed,
        },
        Ok(None) => CursorSessionStatus::disconnected(),
        Err(e) => {
            eprintln!("[buku-token] Cursor session unreadable: {}", cursor_quota::reason(&e, Lang::default()));
            CursorSessionStatus { keychain_denied: matches!(e, QuotaError::KeychainDenied), ..CursorSessionStatus::disconnected() }
        }
    }
}

/// Never returns the cookie, only whether one is stored and when it expires (when readable).
#[tauri::command]
pub async fn cursor_session_status(app: AppHandle) -> Result<CursorSessionStatus, String> {
    async_runtime::spawn_blocking(move || {
        let engine = app.state::<Engine>();
        let mem = CursorMemory {
            keychain_denied: engine.cursor_keychain_denied(),
            last_reason: engine.cursor_last_reason(),
            history_failed: engine.cursor_history_failed(),
        };
        let status = session_status(&cursor_quota::CURSOR_SESSION, mem);
        if status.keychain_denied {
            engine.set_cursor_keychain_denied(true);
        }
        status
    })
    .await
    .map_err(|e| e.to_string())
}

/// The reason of the last Cursor quota failure in the current language, from memory only: the
/// Keychain is not touched, so a language change can reword the line without a prompt.
#[tauri::command]
pub async fn cursor_last_reason(app: AppHandle) -> Option<String> {
    app.state::<Engine>().cursor_last_reason()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterSessionStatus {
    pub connected: bool,
    /// The Keychain refused our item; nothing reads it again until Hubungkan or Putuskan.
    pub keychain_denied: bool,
    /// Why the last poll failed (a fixed text), shown next to "Akun Terhubung".
    pub last_reason: Option<String>,
}

impl OpenRouterSessionStatus {
    fn disconnected() -> Self {
        OpenRouterSessionStatus { connected: false, keychain_denied: false, last_reason: None }
    }
}

/// Does not read the Keychain once it was refused. Carries no key, only fixed texts.
fn openrouter_status(store: &dyn cursor_quota::SessionStore, keychain_denied: bool, last_reason: Option<String>) -> OpenRouterSessionStatus {
    if keychain_denied {
        return OpenRouterSessionStatus { keychain_denied: true, ..OpenRouterSessionStatus::disconnected() };
    }
    match openrouter_quota::load_auth(store) {
        Ok(Some(_)) => OpenRouterSessionStatus { connected: true, keychain_denied: false, last_reason },
        Ok(None) => OpenRouterSessionStatus::disconnected(),
        Err(e) => {
            eprintln!("[buku-token] OpenRouter key unreadable: {}", openrouter_quota::reason(&e, Lang::default()));
            OpenRouterSessionStatus { keychain_denied: e == QuotaError::KeychainDenied, ..OpenRouterSessionStatus::disconnected() }
        }
    }
}

/// Never returns the key, only whether one is stored.
#[tauri::command]
pub async fn openrouter_session_status(app: AppHandle) -> Result<OpenRouterSessionStatus, String> {
    async_runtime::spawn_blocking(move || {
        let engine = app.state::<Engine>();
        // A Putuskan or a new key while the Keychain is read makes this read stale.
        let epoch = engine.openrouter_epoch();
        let status = openrouter_status(&openrouter_quota::OPENROUTER_KEY, engine.openrouter_keychain_denied(), engine.openrouter_last_reason());
        if status.keychain_denied {
            engine.deny_openrouter_keychain_at(epoch);
        }
        status
    })
    .await
    .map_err(|e| e.to_string())
}

/// Checks the pasted key against openrouter.ai and stores it in the Keychain only if accepted.
/// The argument is never logged and the key is kept nowhere else.
#[tauri::command]
pub async fn connect_openrouter(app: AppHandle, key: String) -> Result<OpenRouterSessionStatus, String> {
    let http = app.state::<Background>().openrouter_http.clone();
    let auth = openrouter_quota::prepare_connect(&key, lang(&app))?;
    // A Putuskan or a newer Hubungkan while the server answers bumps this, and the key is not saved.
    let epoch = app.state::<Engine>().openrouter_epoch();
    let fetched = openrouter_quota::fetch(&http, &auth).await;
    let (saver, language) = (app.clone(), lang(&app));
    async_runtime::spawn_blocking(move || {
        let bg = saver.state::<Background>();
        let current = || saver.state::<Engine>().openrouter_epoch() == epoch;
        cursor_quota::connect_locked(&bg.openrouter_keychain, || {
            openrouter_quota::finish_connect(&openrouter_quota::OPENROUTER_KEY, &auth, fetched, &current, language)
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    // Running rounds stop, and the next scheduler tick is due even if a round is still busy.
    app.state::<Engine>().begin_openrouter_session();
    poll_openrouter(&app);
    Ok(OpenRouterSessionStatus { connected: true, ..OpenRouterSessionStatus::disconnected() })
}

/// Deletes the Keychain item and the section.
#[tauri::command]
pub async fn disconnect_openrouter(app: AppHandle) -> Result<(), String> {
    let worker = app.clone();
    async_runtime::spawn_blocking(move || {
        let bg = worker.state::<Background>();
        cursor_quota::disconnect_locked(&bg.openrouter_keychain, &openrouter_quota::OPENROUTER_KEY, || {
            worker.state::<Engine>().end_openrouter_session()
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|_| i18n::disconnect_failed(i18n::Service::OpenRouter, lang(&app)))?;
    let _ = app.emit(DATA_UPDATED, ());
    Ok(())
}

/// The day of the month a billing cycle ends on, in `tz`: the new plan's renewal day.
fn renewal_day(billing_end: DateTime<Utc>, tz: FixedOffset) -> u8 {
    billing_end.with_timezone(&tz).day() as u8
}

/// Checks the pasted cookie against cursor.com and stores it in the Keychain only if accepted.
/// The argument is never logged and the cookie is kept nowhere else.
#[tauri::command]
pub async fn connect_cursor(app: AppHandle, cookie: String) -> Result<CursorSessionStatus, String> {
    let http = app.state::<Background>().cursor_http.clone();
    let auth = cursor_quota::prepare_connect(&cookie, lang(&app))?;
    // A Putuskan or a newer Hubungkan while the server answers bumps this, and the cookie is not saved.
    let epoch = app.state::<Engine>().cursor_epoch();
    let fetched = cursor_quota::fetch_summary(&http, &auth, Utc::now()).await;
    let billing_end = fetched.as_ref().ok().and_then(|q| q.windows.first()).map(|w| w.resets_at);
    let (saver, language) = (app.clone(), lang(&app));
    let expires_at = async_runtime::spawn_blocking(move || {
        let bg = saver.state::<Background>();
        let current = || saver.state::<Engine>().cursor_epoch() == epoch;
        cursor_quota::connect_locked(&bg.cursor_keychain, || {
            cursor_quota::finish_connect(&cursor_quota::CURSOR_SESSION, &auth, fetched, &current, language)
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    // Running rounds stop, and the next scheduler tick is due even if a round is still busy.
    app.state::<Engine>().begin_cursor_session();
    add_cursor_plan(&app, billing_end.map_or(1, |end| renewal_day(end, local_tz()))).await;
    poll_cursor(&app);
    Ok(CursorSessionStatus { connected: true, expires_at, ..CursorSessionStatus::disconnected() })
}

/// First successful connect only: adds the Cursor Pro plan (or fixes the renewal day of the one
/// that exists). A failure is logged; connecting itself already succeeded.
async fn add_cursor_plan(app: &AppHandle, day: u8) {
    let path = app.state::<AppPaths>().settings.clone();
    let res = on_engine(app.clone(), move |e| {
        Ok(e.update_settings(&path, |cur| Ok(crate::settings::with_cursor_plan(cur, day)))?.is_some())
    })
    .await;
    match res {
        Ok(true) => {
            let _ = app.emit(DATA_UPDATED, ());
        }
        Ok(false) => {}
        Err(e) => eprintln!("[buku-token] cannot add the Cursor plan: {e}"),
    }
}

/// Deletes the Keychain item and the quota row. Synced history and the watermark stay.
#[tauri::command]
pub async fn disconnect_cursor(app: AppHandle) -> Result<(), String> {
    let worker = app.clone();
    async_runtime::spawn_blocking(move || {
        let bg = worker.state::<Background>();
        cursor_quota::disconnect_locked(&bg.cursor_keychain, &cursor_quota::CURSOR_SESSION, || {
            worker.state::<Engine>().end_cursor_session()
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|_| i18n::disconnect_failed(i18n::Service::Cursor, lang(&app)))?;
    let _ = app.emit(DATA_UPDATED, ());
    Ok(())
}

#[tauri::command]
pub async fn open_settings(app: AppHandle) -> Result<(), String> {
    let win = app.get_webview_window(SETTINGS_WINDOW).ok_or("settings window not found")?;
    let _ = app.emit_to(SETTINGS_WINDOW, SETTINGS_OPENED, ());
    win.show().and_then(|()| win.set_focus()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scan_status(app: AppHandle) -> Result<ScanStatus, String> {
    Ok(app.state::<Engine>().status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::cell::{Cell, RefCell};

    /// Counts loads, so tests can prove the Keychain is not read.
    #[derive(Default)]
    struct FakeStore {
        cookie: RefCell<Option<String>>,
        deny: bool,
        loads: Cell<u32>,
    }

    impl cursor_quota::SessionStore for FakeStore {
        fn load(&self) -> Result<Option<String>, QuotaError> {
            self.loads.set(self.loads.get() + 1);
            if self.deny { Err(QuotaError::KeychainDenied) } else { Ok(self.cookie.borrow().clone()) }
        }
        fn save(&self, _: &str) -> Result<(), QuotaError> {
            Ok(())
        }
        fn clear(&self) -> Result<(), QuotaError> {
            Ok(())
        }
    }

    fn mem() -> CursorMemory {
        CursorMemory { keychain_denied: false, last_reason: None, history_failed: false }
    }

    #[test]
    fn status_connected_carries_reason_and_history_flag_but_no_cookie() {
        let store = FakeStore { cookie: RefCell::new(Some("SECRET-COOKIE".into())), ..Default::default() };
        let st = session_status(&store, CursorMemory { keychain_denied: false, last_reason: Some("Sedang Offline".into()), history_failed: true });
        assert!(st.connected && !st.keychain_denied && st.history_failed);
        assert_eq!(st.last_reason.as_deref(), Some("Sedang Offline"));
        let json = serde_json::to_string(&st).unwrap();
        assert!(!json.contains("SECRET-COOKIE"), "{json}");
        assert!(json.contains("keychainDenied") && json.contains("lastReason") && json.contains("historyFailed"));
    }

    #[test]
    fn status_without_a_cookie_is_plain_disconnected() {
        let st = session_status(&FakeStore::default(), CursorMemory { last_reason: Some("x".into()), history_failed: true, ..mem() });
        assert!(!st.connected && !st.keychain_denied && st.last_reason.is_none() && !st.history_failed);
    }

    #[test]
    fn status_after_a_refusal_does_not_read_the_keychain_again() {
        let store = FakeStore { deny: true, ..Default::default() };
        let first = session_status(&store, mem());
        assert!(first.keychain_denied && !first.connected);
        assert_eq!(store.loads.get(), 1);
        let again = session_status(&store, CursorMemory { keychain_denied: true, ..mem() });
        assert!(again.keychain_denied);
        assert_eq!(store.loads.get(), 1, "the remembered refusal short-circuits");
    }

    #[test]
    fn openrouter_status_carries_no_key_and_does_not_reread_after_a_refusal() {
        let store = FakeStore { cookie: RefCell::new(Some("sk-or-v1-test".into())), ..Default::default() };
        let st = openrouter_status(&store, false, Some("Sedang Offline".into()));
        assert!(st.connected && !st.keychain_denied);
        let json = serde_json::to_string(&st).unwrap();
        assert!(!json.contains("sk-or"), "{json}");
        assert!(json.contains("keychainDenied") && json.contains("lastReason") && json.contains("Sedang Offline"));

        assert!(!openrouter_status(&FakeStore::default(), false, Some("x".into())).connected);
        let denied = FakeStore { deny: true, ..Default::default() };
        assert!(openrouter_status(&denied, false, None).keychain_denied);
        assert_eq!(denied.loads.get(), 1);
        assert!(openrouter_status(&denied, true, None).keychain_denied);
        assert_eq!(denied.loads.get(), 1, "the remembered refusal short-circuits");
    }

    #[test]
    fn renewal_day_is_the_billing_end_day_in_the_local_zone() {
        let end = Utc.with_ymd_and_hms(2026, 11, 7, 1, 48, 53).unwrap();
        assert_eq!(renewal_day(end, FixedOffset::east_opt(7 * 3600).unwrap()), 7);
        assert_eq!(renewal_day(end, FixedOffset::west_opt(5 * 3600).unwrap()), 6, "the local calendar day");
    }
}
