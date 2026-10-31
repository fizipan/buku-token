//! Scan orchestration and the read side behind the Tauri commands. Holds the store, the price
//! table, settings and the latest quota state. No Tauri types here, so it runs headless in
//! tests; `commands.rs` adds the scheduler, watcher, events and commands on top.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use chrono::{DateTime, FixedOffset, NaiveTime, Utc};
use serde::Serialize;

use crate::aggregate::{
    self, is_enabled, HistoryView, ModelsView, Overview, Period, ProjectsView, SessionFilter, SessionPage,
};
use crate::ingest::codex::{CodexCtx, CodexRateLimits};
use crate::ingest::antigravity;
use crate::ingest::cursor as cursor_ingest;
use crate::ingest::{self, claude, codex, hermes, opencode, DbStamp, ReadBatch, ScanStats};
use crate::model::{Agent, UsageEvent};
use crate::pricing::Pricing;
use crate::project::ProjectResolver;
use crate::quota::claude::QuotaError;
use crate::quota::openrouter::{OpenRouterData, OpenRouterView};
use crate::quota::{self, AgentQuota, Burn};
use crate::rtk::{self, RtkDay, RtkSummary};
use crate::i18n::{self, Service};
use crate::settings::{Lang, Settings};
use crate::share::{ShareData, ShareRequest};
use crate::store::Store;

/// Words a stored quota in the language current now. A quota keeps window ids and the error, never a
/// text, so a language change shows at once on the next read without a new poll.
fn localize(q: &mut AgentQuota, lang: Lang) {
    for w in q.windows.iter_mut().chain(q.binding.as_mut()) {
        w.label = i18n::window_label(&w.id, lang);
    }
    if let Some(e) = &q.failure {
        let service = if q.agent == Agent::Cursor { Service::Cursor } else { Service::Claude };
        q.unavailable_reason = Some(i18n::quota_reason(service, e, lang));
    }
}

/// Fallback scan when the watcher saw nothing.
const SCAN_EVERY: Duration = Duration::from_secs(60);
const QUOTA_EVERY: Duration = Duration::from_secs(5 * 60);
/// When the popover is shown, data older than this is refreshed first.
const POPOVER_SCAN_AGE: Duration = Duration::from_secs(30);
const POPOVER_QUOTA_AGE: Duration = Duration::from_secs(60);
/// `days` accepted by the history and session views.
const MAX_DAYS: u32 = 60;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ScanProgress {
    pub done: usize,
    pub total: usize,
}

/// One log location and whether it exists on this Mac.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub agent: Agent,
    pub path: String,
    pub found: bool,
}

/// What the last scan saw of Antigravity.
#[derive(Default, PartialEq)]
struct AntigravityScan {
    files: Vec<antigravity::Stamped>,
    summaries: Vec<antigravity::Summary>,
    /// Modification time and size of the summaries database when `summaries` was read. The database
    /// is opened again only when this changes.
    stamp: Option<(SystemTime, u64)>,
}

type ProjectMap = HashMap<String, String>;

/// Two consumers of one update (the menu bar and the notifications) share one overview this long.
const SHARED_OVERVIEW_FOR: Duration = Duration::from_secs(2);

/// A folder that is read for activity only (not an `Agent`), so it has a name, not an agent.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySource {
    pub name: String,
    pub path: String,
    pub found: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    /// Malformed lines/rows skipped since the app started (each one is skipped once).
    pub skipped: u64,
    /// Files or databases that could not be read in the last pass.
    pub errors: u64,
    pub last_scan_at: Option<DateTime<Utc>>,
    pub pricing_version: String,
    pub sources: Vec<SourceStatus>,
    pub activity_sources: Vec<ActivitySource>,
}

/// Where each agent keeps its logs.
#[derive(Clone, Debug)]
pub struct Roots {
    pub claude: PathBuf,
    pub codex: Vec<PathBuf>,
    pub hermes: PathBuf,
    pub opencode: PathBuf,
    /// `~/.cursor/projects` (entry names only). Cursor's own database and Keychain are never opened.
    pub cursor_projects: PathBuf,
    /// Antigravity's conversation folders. Only names and modification dates are read.
    pub antigravity: Vec<PathBuf>,
    /// The CLI's summaries database (id, step count and date are read, nothing else).
    pub antigravity_summaries: PathBuf,
}

impl Roots {
    pub fn from_home(home: &Path) -> Roots {
        Roots {
            claude: home.join(".claude/projects"),
            codex: vec![home.join(".codex/sessions"), home.join(".codex/archived_sessions")],
            hermes: home.join(".hermes/state.db"),
            opencode: home.join(".local/share/opencode/opencode.db"),
            cursor_projects: home.join(".cursor/projects"),
            antigravity: antigravity::folders(home),
            antigravity_summaries: home.join(antigravity::SUMMARIES),
        }
    }

    fn databases(&self) -> [(Agent, &Path); 2] {
        [(Agent::Hermes, &self.hermes), (Agent::OpenCode, &self.opencode)]
    }

    /// Existing directories to watch, with whether to recurse. A database is watched through
    /// its folder, because SQLite in WAL mode writes to `<db>-wal`, not to the file itself.
    pub fn watch_targets(&self) -> Vec<(PathBuf, bool)> {
        let logs = std::iter::once(&self.claude).chain(&self.codex).map(|p| (p.clone(), true));
        let dbs = self.databases().into_iter().filter_map(|(_, db)| db.parent().map(|d| (d.to_path_buf(), false)));
        logs.chain(dbs).filter(|(p, _)| p.is_dir()).collect()
    }

    /// True for a change that can carry new usage: a `.jsonl` under a log root, or exactly
    /// `<db>` / `<db>-wal`, the only files a database's owner writes rows to. `-shm`, `-journal`
    /// and lock files also change on plain reads or housekeeping, so they never trigger a scan.
    pub fn is_relevant(&self, path: &Path) -> bool {
        if path.extension().is_some_and(|x| x == "jsonl") {
            return std::iter::once(&self.claude).chain(&self.codex).any(|root| path.starts_with(root));
        }
        self.databases().into_iter().any(|(_, db)| {
            let (Some(name), Some(db_name)) = (path.file_name(), db.file_name()) else { return false };
            let mut wal = db_name.to_os_string();
            wal.push("-wal");
            path.parent() == db.parent() && (name == db_name || name == wal)
        })
    }

    fn sources(&self) -> Vec<SourceStatus> {
        let src = |agent, p: &Path| SourceStatus { agent, path: p.to_string_lossy().into_owned(), found: p.exists() };
        let mut v = vec![src(Agent::ClaudeCode, &self.claude)];
        v.extend(self.codex.iter().map(|p| src(Agent::Codex, p)));
        v.extend(self.databases().into_iter().map(|(a, p)| src(a, p)));
        v.push(src(Agent::Cursor, &self.cursor_projects));
        v
    }

    fn activity_sources(&self) -> Vec<ActivitySource> {
        let src = |p: &PathBuf| ActivitySource { name: "Antigravity".into(), path: p.to_string_lossy().into_owned(), found: p.exists() };
        self.antigravity.iter().chain(std::iter::once(&self.antigravity_summaries)).map(src).collect()
    }
}

/// Why the scheduler woke up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Tick,
    FilesChanged,
    PopoverOpened,
    Refresh,
}

/// Minimum age of (scanned data, Claude quota) at which `t` starts a scan / a quota poll.
pub fn thresholds(t: Trigger) -> (Duration, Duration) {
    match t {
        Trigger::Tick => (SCAN_EVERY, QUOTA_EVERY),
        Trigger::FilesChanged => (Duration::ZERO, QUOTA_EVERY),
        Trigger::PopoverOpened => (POPOVER_SCAN_AGE, POPOVER_QUOTA_AGE),
        Trigger::Refresh => (Duration::ZERO, Duration::ZERO),
    }
}

/// Never done counts as due.
pub fn is_due(age: Option<Duration>, threshold: Duration) -> bool {
    age.is_none_or(|a| a >= threshold)
}

/// The downloaded table when present and valid, else the bundled snapshot.
pub fn load_pricing(json: &Path, version: &Path) -> Pricing {
    let Some(p) = std::fs::read_to_string(json).ok().and_then(|raw| Pricing::from_json_str(&raw).ok()) else {
        return Pricing::bundled();
    };
    match std::fs::read_to_string(version) {
        Ok(v) if !v.trim().is_empty() => p.with_version(v.trim()),
        _ => p,
    }
}

pub fn save_pricing(json_path: &Path, version_path: &Path, json: &str, version: &str) -> Result<()> {
    write_atomic(json_path, json)?;
    write_atomic(version_path, version)
}

/// Temp file in the same folder, then rename.
fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!("{name}.tmp"));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(())
}

#[derive(Default)]
struct Progress {
    running: bool,
    done: usize,
    total: usize,
    skipped: u64,
    errors: u64,
    last_scan_at: Option<DateTime<Utc>>,
    scanned: Option<Instant>,
    quota_polled: Option<Instant>,
    cursor_polled: Option<Instant>,
    openrouter_polled: Option<Instant>,
    rtk_read: Option<Instant>,
}

pub struct Engine {
    store: Mutex<Store>,
    pricing: RwLock<Arc<Pricing>>,
    settings: RwLock<Settings>,
    /// Held across a whole read-merge-write of the settings file, so two writers never lose each other's fields.
    settings_write: Mutex<()>,
    roots: Roots,
    projects: Mutex<ProjectResolver>,
    /// Per-file Codex parsing state. Held for a whole pass, so passes never overlap.
    codex_ctx: Mutex<HashMap<PathBuf, CodexCtx>>,
    codex_limits: Mutex<Option<CodexRateLimits>>,
    /// Where agent databases are copied just for the time of a read:
    /// `<snapshots>/{hermes,opencode}/`. Empty between reads.
    snapshots: PathBuf,
    /// Stamp of the originals at the last fully processed snapshot, per database.
    db_stamps: Mutex<HashMap<Agent, DbStamp>>,
    claude_quota: Mutex<Option<AgentQuota>>,
    cursor_quota: Mutex<Option<AgentQuota>>,
    /// The user refused the Keychain prompt: only an explicit refresh asks again.
    keychain_denied: AtomicBool,
    /// Bumped when the Cursor cookie is replaced or removed; a round started before that stops.
    cursor_epoch: AtomicU64,
    /// The Keychain refused our own Cursor item: nothing reads it again until the user connects
    /// or disconnects (Claude's `keychain_denied` pattern).
    cursor_keychain_denied: AtomicBool,
    /// When the last Cursor history sync failed; cleared by a success, a connect, a disconnect or Refresh.
    cursor_history_failed: Mutex<Option<(Instant, Duration)>>,
    /// OpenRouter has its own copy of the Cursor machinery: epoch, refused Keychain, backoff.
    openrouter: Mutex<Option<OpenRouterView>>,
    openrouter_epoch: AtomicU64,
    openrouter_keychain_denied: AtomicBool,
    openrouter_failed: Mutex<Option<(Instant, Duration)>>,
    /// Names, dates and summary rows of Antigravity's conversations, refreshed by each scan.
    antigravity_scan: Mutex<AntigravityScan>,
    /// Cursor's conversation-to-project map and when it was built.
    cursor_slugs: Mutex<HashMap<String, String>>,
    /// The overview built for the current `version`, for the menu bar and the notifications.
    shared_overview: Mutex<Option<(u64, Instant, Arc<Overview>)>>,
    rtk_today: Mutex<Option<RtkDay>>,
    progress: Mutex<Progress>,
    /// Bumped whenever something shown in the UI may have changed.
    version: AtomicU64,
}

impl Engine {
    pub fn new(store: Store, pricing: Pricing, settings: Settings, roots: Roots, home: PathBuf, snapshots: PathBuf) -> Engine {
        // A run that was killed mid-read can leave a copy behind.
        if let Err(e) = ingest::remove_snapshot(&snapshots) {
            eprintln!("[buku-token] {e:#}");
        }
        Engine {
            store: Mutex::new(store),
            pricing: RwLock::new(Arc::new(pricing)),
            settings: RwLock::new(settings),
            settings_write: Mutex::new(()),
            roots,
            projects: Mutex::new(ProjectResolver::new(home)),
            codex_ctx: Mutex::new(HashMap::new()),
            codex_limits: Mutex::new(None),
            snapshots,
            db_stamps: Mutex::new(HashMap::new()),
            claude_quota: Mutex::new(None),
            cursor_quota: Mutex::new(None),
            keychain_denied: AtomicBool::new(false),
            cursor_epoch: AtomicU64::new(0),
            cursor_keychain_denied: AtomicBool::new(false),
            cursor_history_failed: Mutex::new(None),
            openrouter: Mutex::new(None),
            openrouter_epoch: AtomicU64::new(0),
            openrouter_keychain_denied: AtomicBool::new(false),
            openrouter_failed: Mutex::new(None),
            antigravity_scan: Mutex::new(AntigravityScan::default()),
            cursor_slugs: Mutex::new(HashMap::new()),
            shared_overview: Mutex::new(None),
            rtk_today: Mutex::new(None),
            progress: Mutex::new(Progress::default()),
            version: AtomicU64::new(0),
        }
    }

    pub fn roots(&self) -> &Roots {
        &self.roots
    }

    /// Changes whenever data shown in the UI may have changed.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }

    fn bump(&self) {
        self.version.fetch_add(1, Ordering::SeqCst);
    }

    /// One incremental pass over every enabled agent. A failing file or database is counted in
    /// `ScanStatus::errors` and the pass goes on.
    pub fn scan_all(&self, progress: impl Fn(ScanProgress)) -> Result<ScanStats> {
        let mut ctxs = lock(&self.codex_ctx);
        let settings = self.settings();
        let on = |a: Agent| is_enabled(&settings, a);
        let claude_files = if on(Agent::ClaudeCode) { claude::log_files(&self.roots.claude) } else { Vec::new() };
        let mut codex_files = Vec::new();
        if on(Agent::Codex) {
            codex_files.extend(self.roots.codex.iter().flat_map(|r| codex::log_files(r)));
            codex_files.sort();
            codex_files.dedup();
        }
        let dbs: Vec<Agent> = self.roots.databases().into_iter().map(|(a, _)| a).filter(|a| on(*a)).collect();
        let total = claude_files.len() + codex_files.len() + dbs.len();
        {
            let mut p = lock(&self.progress);
            (p.running, p.done, p.total) = (true, 0, total);
        }
        progress(ScanProgress { done: 0, total });

        let (mut stats, mut errors, mut done) = (ScanStats::default(), 0, 0);
        let mut step = |what: &dyn std::fmt::Display, r: Result<ScanStats>| {
            match r {
                Ok(s) => {
                    stats.lines += s.lines;
                    stats.skipped += s.skipped;
                    stats.events += s.events;
                }
                Err(e) => {
                    errors += 1;
                    eprintln!("[buku-token] cannot read {what}: {e:#}");
                }
            }
            done += 1;
            lock(&self.progress).done = done;
            progress(ScanProgress { done, total });
        };
        for path in &claude_files {
            step(&path.display(), self.scan_claude(path));
        }
        for path in &codex_files {
            step(&path.display(), self.scan_codex(path, &mut ctxs));
        }
        for agent in dbs {
            step(&format!("{agent:?} database"), self.scan_db(agent));
        }

        self.scan_antigravity(settings.antigravity_enabled);
        {
            let mut p = lock(&self.progress);
            p.running = false;
            p.skipped += stats.skipped;
            p.errors = errors;
            p.last_scan_at = Some(Utc::now());
            p.scanned = Some(Instant::now());
        }
        if stats.events > 0 {
            self.bump();
        }
        Ok(stats)
    }

    /// Only `read_dir` and file dates; a switched-off Antigravity keeps nothing.
    fn scan_antigravity(&self, enabled: bool) {
        let mut scan = if enabled {
            let db = &self.roots.antigravity_summaries;
            let stamp = std::fs::metadata(db).ok().and_then(|m| Some((m.modified().ok()?, m.len())));
            let kept = {
                let old = lock(&self.antigravity_scan);
                (stamp.is_some() && old.stamp == stamp).then(|| old.summaries.clone())
            };
            AntigravityScan {
                files: antigravity::list(&self.roots.antigravity),
                summaries: kept.unwrap_or_else(|| antigravity::read_summaries(db)),
                stamp,
            }
        } else {
            AntigravityScan::default()
        };
        scan.files.sort();
        scan.summaries.sort();
        let mut slot = lock(&self.antigravity_scan);
        if *slot != scan {
            *slot = scan;
            drop(slot);
            self.bump();
        }
    }

    fn persist(&self, store: &Store, events: &[UsageEvent], upsert: bool) -> Result<()> {
        // Read under the store lock, so a price swap (which holds it) is never half applied.
        let pricing = self.pricing();
        let cost = |e: &UsageEvent| pricing.cost(e.agent, &e.model, &e.tokens);
        if upsert { store.upsert_events(events, &cost)? } else { store.insert_events(events, &cost)? };
        Ok(())
    }

    fn scan_claude(&self, path: &Path) -> Result<ScanStats> {
        let store = lock(&self.store);
        let (events, cursor, stats) = claude::scan_file(&store, path)?;
        self.persist(&store, &events, false)?;
        store.set_cursor(&cursor)?;
        Ok(stats)
    }

    fn scan_codex(&self, path: &Path, ctxs: &mut HashMap<PathBuf, CodexCtx>) -> Result<ScanStats> {
        let ctx = ctxs.entry(path.to_path_buf()).or_default();
        let before = ctx.clone();
        let res = self.scan_codex_with(path, ctx);
        if res.is_err() {
            // The cursor did not move: next pass re-reads the same lines from the same state.
            *ctx = before;
        }
        let (stats, limits) = res?;
        if let Some(l) = limits {
            let mut cur = lock(&self.codex_limits);
            if cur.as_ref().is_none_or(|c| l.observed_at > c.observed_at) {
                *cur = Some(l);
                self.bump();
            }
        }
        Ok(stats)
    }

    fn scan_codex_with(&self, path: &Path, ctx: &mut CodexCtx) -> Result<(ScanStats, Option<CodexRateLimits>)> {
        let store = lock(&self.store);
        let (events, cursor, stats, limits) = codex::scan_file(&store, path, ctx)?;
        self.persist(&store, &events, false)?;
        store.set_cursor(&cursor)?;
        Ok((stats, limits))
    }

    /// Hermes/OpenCode rows grow in place, so they are upserted past a timestamp watermark.
    /// They are read from a private copy, taken only when the originals changed and deleted right
    /// after: SQLite must never open a file in the agent's folder, and no copy stays on disk. The
    /// stamp is kept (in memory) only after a successful pass, so a failed copy or read (a torn
    /// snapshot, say) is retried by the next one.
    fn scan_db(&self, agent: Agent) -> Result<ScanStats> {
        type Reader = fn(&Path, i64) -> Result<ReadBatch>;
        let (db, key, read): (&Path, &str, Reader) = match agent {
            Agent::Hermes => (&self.roots.hermes, "hm", hermes::read_since),
            _ => (&self.roots.opencode, "oc", opencode::read_since),
        };
        let Some(stamp) = DbStamp::of(db)? else { return Ok(ScanStats::default()) };
        if lock(&self.db_stamps).get(&agent) == Some(&stamp) {
            return Ok(ScanStats::default());
        }
        let watermark = lock(&self.store).get_watermark(key)?;
        // Without holding our store.
        let Some((batch, stamp)) = ingest::read_snapshot(db, &self.snapshot_dir(agent), |copy| read(copy, watermark))? else {
            return Ok(ScanStats::default());
        };
        let store = lock(&self.store);
        self.persist(&store, &batch.events, true)?;
        store.set_watermark(key, batch.watermark)?;
        drop(store);
        lock(&self.db_stamps).insert(agent, stamp);
        Ok(ScanStats { lines: 0, skipped: batch.skipped, events: batch.events.len() as u64 })
    }

    pub fn status(&self) -> ScanStatus {
        let p = lock(&self.progress);
        ScanStatus {
            running: p.running,
            done: p.done,
            total: p.total,
            skipped: p.skipped,
            errors: p.errors,
            last_scan_at: p.last_scan_at,
            pricing_version: self.pricing().version(),
            sources: self.roots.sources(),
            activity_sources: self.roots.activity_sources(),
        }
    }

    pub fn scan_age(&self) -> Option<Duration> {
        lock(&self.progress).scanned.map(|t| t.elapsed())
    }

    pub fn quota_age(&self) -> Option<Duration> {
        lock(&self.progress).quota_polled.map(|t| t.elapsed())
    }

    pub fn cursor_age(&self) -> Option<Duration> {
        lock(&self.progress).cursor_polled.map(|t| t.elapsed())
    }

    pub fn openrouter_age(&self) -> Option<Duration> {
        lock(&self.progress).openrouter_polled.map(|t| t.elapsed())
    }

    pub fn rtk_age(&self) -> Option<Duration> {
        lock(&self.progress).rtk_read.map(|t| t.elapsed())
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The one way to change the stored settings: `change` gets the current ones and returns the next
    /// (or `None` for no change). The lock covers read, merge, write and publish. Returns old and new.
    pub fn update_settings(&self, path: &Path, change: impl FnOnce(&Settings) -> Result<Option<Settings>>) -> Result<Option<(Settings, Settings)>> {
        let _guard = lock(&self.settings_write);
        let old = self.settings();
        let Some(next) = change(&old)? else { return Ok(None) };
        crate::settings::save(path, &next)?;
        let saved = crate::settings::load(path);
        self.set_settings(saved.clone());
        Ok(Some((old, saved)))
    }

    fn snapshot_dir(&self, agent: Agent) -> PathBuf {
        self.snapshots.join(if agent == Agent::Hermes { "hermes" } else { "opencode" })
    }

    pub fn set_settings(&self, s: Settings) {
        for (agent, _) in self.roots.databases() {
            if !is_enabled(&s, agent)
                && let Err(e) = ingest::remove_snapshot(&self.snapshot_dir(agent))
            {
                eprintln!("[buku-token] {e:#}");
            }
        }
        *self.settings.write().unwrap_or_else(PoisonError::into_inner) = s;
        self.bump();
    }

    fn pricing(&self) -> Arc<Pricing> {
        self.pricing.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The Claude quota is read only after onboarding (the welcome screen explains the Keychain
    /// prompt) and, once the prompt was refused, only when the user asks (`force`).
    pub fn wants_claude_quota(&self, force: bool) -> bool {
        let s = self.settings();
        s.onboarded && is_enabled(&s, Agent::ClaudeCode) && (force || !self.keychain_denied.load(Ordering::SeqCst))
    }

    /// Stores a poll result. A success also records `five_hour`/`seven_day` samples for the burn
    /// chart; a failure keeps only the reason, so the UI never shows stale windows.
    pub fn set_claude_quota(&self, res: Result<AgentQuota, QuotaError>) -> Result<()> {
        self.keychain_denied.store(matches!(res, Err(QuotaError::KeychainDenied)), Ordering::SeqCst);
        let mut sampled = Ok(());
        let q = match res {
            Ok(q) => {
                let store = lock(&self.store);
                for w in q.windows.iter().filter(|w| w.id == "five_hour" || w.id == "seven_day") {
                    if let Err(e) = store.add_quota_sample(Agent::ClaudeCode, &w.id, w.observed_at, w.used_pct) {
                        sampled = Err(e);
                    }
                }
                q
            }
            Err(e) => AgentQuota::failed(Agent::ClaudeCode, e),
        };
        *lock(&self.claude_quota) = Some(q);
        lock(&self.progress).quota_polled = Some(Instant::now());
        self.bump();
        sampled
    }

    /// Cursor is polled only after onboarding and for an enabled agent. Whether a cookie is
    /// stored is checked by the round itself: without one nothing is sent.
    pub fn wants_cursor(&self) -> bool {
        let s = self.settings();
        s.onboarded && is_enabled(&s, Agent::Cursor)
    }

    pub fn cursor_epoch(&self) -> u64 {
        self.cursor_epoch.load(Ordering::SeqCst)
    }

    /// Like [`set_cursor_quota`], but dropped (false) when the cookie changed since `epoch`:
    /// an answer fetched with a removed or replaced cookie must not bring the row back.
    pub fn set_cursor_quota_at(&self, epoch: u64, res: Result<AgentQuota, QuotaError>) -> bool {
        let q = res.unwrap_or_else(|e| AgentQuota::failed(Agent::Cursor, e));
        let mut slot = lock(&self.cursor_quota);
        if self.cursor_epoch() != epoch {
            return false;
        }
        *slot = Some(q);
        drop(slot);
        lock(&self.progress).cursor_polled = Some(Instant::now());
        self.bump();
        true
    }

    pub fn cursor_keychain_denied(&self) -> bool {
        self.cursor_keychain_denied.load(Ordering::SeqCst)
    }

    pub fn set_cursor_keychain_denied(&self, denied: bool) {
        self.cursor_keychain_denied.store(denied, Ordering::SeqCst);
    }

    /// The current Cursor unavailable reason, if the last quota result was a failure.
    pub fn cursor_last_reason(&self) -> Option<String> {
        let lang = self.settings().language;
        lock(&self.cursor_quota).as_ref().and_then(|q| q.failure).map(|e| quota::cursor::reason(&e, lang))
    }

    /// True while a failed history sync is remembered (until a success or an explicit reset).
    pub fn cursor_history_failed(&self) -> bool {
        lock(&self.cursor_history_failed).is_some()
    }

    /// False while the rest earned by the last failed history sync (10 minutes or an hour) runs.
    pub fn cursor_history_due(&self) -> bool {
        match *lock(&self.cursor_history_failed) {
            Some((at, backoff)) => cursor_ingest::history_due(Some(at.elapsed()), backoff),
            None => true,
        }
    }

    pub fn mark_cursor_history_failed(&self, backoff: Duration) {
        *lock(&self.cursor_history_failed) = Some((Instant::now(), backoff));
    }

    pub fn clear_cursor_history_backoff(&self) {
        *lock(&self.cursor_history_failed) = None;
    }

    /// The cookie was deleted: invalidates running rounds and removes the quota row.
    pub fn end_cursor_session(&self) {
        self.set_cursor_keychain_denied(false);
        self.clear_cursor_history_backoff();
        let mut slot = lock(&self.cursor_quota);
        self.cursor_epoch.fetch_add(1, Ordering::SeqCst);
        *slot = None;
        drop(slot);
        lock(&self.progress).cursor_polled = Some(Instant::now());
        self.bump();
    }

    /// A new cookie was stored: invalidates rounds using the old one and makes the next
    /// scheduler tick start a fresh round.
    pub fn begin_cursor_session(&self) {
        self.set_cursor_keychain_denied(false);
        self.clear_cursor_history_backoff();
        let mut slot = lock(&self.cursor_quota);
        self.cursor_epoch.fetch_add(1, Ordering::SeqCst);
        *slot = None; // the old cookie's figures must not outlive it
        drop(slot);
        lock(&self.progress).cursor_polled = None;
        self.bump();
    }

    /// No stored cookie (never connected, or just disconnected): no Cursor quota row at all. The
    /// poll is still dated, so Buku Token's own Keychain item is checked at the normal cadence
    /// and not on every tick.
    pub fn clear_cursor_quota(&self) {
        *lock(&self.cursor_quota) = None;
        lock(&self.progress).cursor_polled = Some(Instant::now());
        self.bump();
    }

    /// Stores a Cursor quota result. A failure keeps only the reason, and is dated like a
    /// success so it is not retried every tick.
    pub fn set_cursor_quota(&self, res: Result<AgentQuota, QuotaError>) {
        self.set_cursor_quota_at(self.cursor_epoch(), res);
    }

    /// OpenRouter is polled only after onboarding and with its own switch on. Whether a key is
    /// stored is checked by the round itself: without one nothing is sent.
    pub fn wants_openrouter(&self) -> bool {
        let s = self.settings();
        s.onboarded && s.openrouter_enabled
    }

    pub fn openrouter_epoch(&self) -> u64 {
        self.openrouter_epoch.load(Ordering::SeqCst)
    }

    /// Stores an OpenRouter result, dropped (false) when the key changed since `epoch`. A failure
    /// keeps only the reason and is dated like a success, so it is not retried every tick.
    pub fn set_openrouter_at(&self, epoch: u64, res: Result<OpenRouterData, QuotaError>) -> bool {
        let res_denied = res.as_ref().is_err_and(|e| *e == QuotaError::KeychainDenied);
        let view = match res {
            Ok(data) => OpenRouterView { data: Some(data), unavailable_reason: None, failure: None },
            Err(e) => OpenRouterView { data: None, unavailable_reason: None, failure: Some(e) },
        };
        let mut slot = lock(&self.openrouter);
        if self.openrouter_epoch() != epoch {
            return false;
        }
        if res_denied {
            self.set_openrouter_keychain_denied(true);
        }
        *slot = Some(view);
        drop(slot);
        lock(&self.progress).openrouter_polled = Some(Instant::now());
        self.bump();
        true
    }

    pub fn openrouter_keychain_denied(&self) -> bool {
        self.openrouter_keychain_denied.load(Ordering::SeqCst)
    }

    pub fn set_openrouter_keychain_denied(&self, denied: bool) {
        self.openrouter_keychain_denied.store(denied, Ordering::SeqCst);
    }

    /// The Keychain refused our item (seen by the status command): no old figures may stay on show.
    /// Ignored (false) when the key changed since `epoch`, the point the Keychain read began.
    pub fn deny_openrouter_keychain_at(&self, epoch: u64) -> bool {
        let mut slot = lock(&self.openrouter);
        if self.openrouter_epoch() != epoch {
            return false;
        }
        self.set_openrouter_keychain_denied(true);
        *slot = None;
        drop(slot);
        self.bump();
        true
    }

    /// The current OpenRouter unavailable reason, if the last result was a failure.
    pub fn openrouter_last_reason(&self) -> Option<String> {
        let lang = self.settings().language;
        lock(&self.openrouter).as_ref().and_then(|v| v.failure).map(|e| quota::openrouter::reason(&e, lang))
    }

    /// False while the rest earned by the last failed poll runs.
    pub fn openrouter_due(&self) -> bool {
        match *lock(&self.openrouter_failed) {
            Some((at, backoff)) => cursor_ingest::history_due(Some(at.elapsed()), backoff),
            None => true,
        }
    }

    pub fn mark_openrouter_failed(&self, backoff: Duration) {
        *lock(&self.openrouter_failed) = Some((Instant::now(), backoff));
    }

    pub fn clear_openrouter_backoff(&self) {
        *lock(&self.openrouter_failed) = None;
    }

    /// The key was deleted: invalidates running rounds and removes the section.
    pub fn end_openrouter_session(&self) {
        self.set_openrouter_keychain_denied(false);
        self.clear_openrouter_backoff();
        let mut slot = lock(&self.openrouter);
        self.openrouter_epoch.fetch_add(1, Ordering::SeqCst);
        *slot = None;
        drop(slot);
        lock(&self.progress).openrouter_polled = Some(Instant::now());
        self.bump();
    }

    /// A new key was stored: invalidates rounds using the old one and makes the next tick poll.
    pub fn begin_openrouter_session(&self) {
        self.set_openrouter_keychain_denied(false);
        self.clear_openrouter_backoff();
        let mut slot = lock(&self.openrouter);
        self.openrouter_epoch.fetch_add(1, Ordering::SeqCst);
        *slot = None; // the old key's figures must not outlive it
        drop(slot);
        lock(&self.progress).openrouter_polled = None;
        self.bump();
    }

    /// No stored key: no OpenRouter section. The poll is still dated, so our Keychain item is
    /// checked at the normal cadence and not on every tick.
    pub fn clear_openrouter_at(&self, epoch: u64) {
        let mut slot = lock(&self.openrouter);
        if self.openrouter_epoch() != epoch {
            return;
        }
        *slot = None;
        drop(slot);
        lock(&self.progress).openrouter_polled = Some(Instant::now());
        self.bump();
    }

    /// Notifications still to send, for the quotas as they are now. Old sent keys are dropped first.
    pub fn pending_notices(&self, tz: FixedOffset, now: DateTime<Utc>) -> Result<Vec<crate::notify::Notice>> {
        let settings = self.settings();
        if !settings.notify_low_quota && !settings.notify_projected_exhaustion {
            return Ok(vec![]);
        }
        let overview = self.shared_overview(tz, now)?;
        let store = lock(&self.store);
        store.prune_sent(now - chrono::Duration::days(1))?;
        Ok(crate::notify::decide(&overview.quotas, overview.burn.as_ref(), &settings, &store.sent_keys()?, now, tz))
    }

    /// Cursor's conversation-to-project map, listed fresh each round so new conversations get their
    /// project: their events are stored once and never corrected. Decoded slugs are remembered.
    pub fn cursor_project_map(&self) -> Arc<ProjectMap> {
        Arc::new(cursor_ingest::project_map(&self.roots.cursor_projects, &mut lock(&self.cursor_slugs)))
    }

    /// Like [`Engine::overview`], but the menu bar and the notifications of one update get the same
    /// one: it is reused while nothing shown has changed and it is a couple of seconds old at most.
    pub fn shared_overview(&self, tz: FixedOffset, now: DateTime<Utc>) -> Result<Arc<Overview>> {
        let mut slot = lock(&self.shared_overview);
        let version = self.version();
        if let Some((v, at, o)) = slot.as_ref()
            && *v == version
            && at.elapsed() < SHARED_OVERVIEW_FOR
        {
            return Ok(o.clone());
        }
        let o = Arc::new(self.overview(tz, now)?);
        *slot = Some((version, Instant::now(), o.clone()));
        Ok(o)
    }

    pub fn mark_notified(&self, notice: &crate::notify::Notice) -> Result<()> {
        lock(&self.store).mark_sent(&notice.key, notice.expires)
    }

    pub fn cursor_watermark(&self) -> Result<i64> {
        lock(&self.store).get_watermark("cr")
    }

    pub fn set_cursor_watermark(&self, ts_ms: i64) -> Result<()> {
        lock(&self.store).set_watermark("cr", ts_ms)
    }

    /// Upserts one page of synced Cursor events. The store lock is held only for the write.
    pub fn store_cursor_events(&self, events: &[UsageEvent]) -> Result<()> {
        let store = lock(&self.store);
        self.persist(&store, events, true)?;
        drop(store);
        if !events.is_empty() {
            self.bump();
        }
        Ok(())
    }

    pub fn set_rtk(&self, day: Option<RtkDay>) {
        lock(&self.progress).rtk_read = Some(Instant::now());
        let mut cur = lock(&self.rtk_today);
        if *cur != day {
            *cur = day;
            self.bump();
        }
    }

    /// Swaps the price table and re-prices every stored event that has no reported cost.
    pub fn set_pricing(&self, p: Pricing) -> Result<()> {
        let store = lock(&self.store);
        store.recompute_costs(&|e: &UsageEvent| p.cost(e.agent, &e.model, &e.tokens))?;
        *self.pricing.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(p);
        drop(store);
        self.bump();
        Ok(())
    }

    pub fn overview(&self, tz: FixedOffset, now: DateTime<Utc>) -> Result<Overview> {
        let settings = self.settings();
        let store = lock(&self.store);
        let lang = settings.language;
        // A Mac without Claude Code has no Claude folder: "login not found" there is no problem to show.
        let mut claude = lock(&self.claude_quota)
            .clone()
            .filter(|q| !(q.failure == Some(QuotaError::NotFound) && !self.roots.claude.exists()));
        let mut burn = None;
        if let Some(q) = claude.as_mut() {
            localize(q, lang);
            // A window can reset between polls.
            for w in &mut q.windows {
                w.expired = w.resets_at < now;
            }
            q.binding = quota::binding_window(&q.windows);
            burn = claude_burn(&store, &q.windows)?;
            q.near_limit = quota::is_near_limit(&q.windows, burn.as_ref());
        }
        let codex = lock(&self.codex_limits).as_ref().map(|rl| quota::codex::from_rate_limits(rl, now, lang));
        let cursor = lock(&self.cursor_quota).clone().map(|mut q| {
            localize(&mut q, lang);
            for w in &mut q.windows {
                w.expired = w.resets_at < now;
            }
            quota::cursor::build_quota_from(q)
        });
        let quotas: Vec<AgentQuota> = claude.into_iter().chain(codex).chain(cursor).collect();
        let rtk = self.rtk_summary(&store, &settings, tz, now)?;
        let mut overview = aggregate::overview(&store, &quotas, burn, rtk, &settings, tz, now)?;
        // Beside the agents, never in them: no total, chart, tray item or notification reads these.
        overview.openrouter = lock(&self.openrouter).clone().filter(|_| settings.openrouter_enabled).map(|v| v.localized(lang));
        overview.antigravity = Some({
            let scan = lock(&self.antigravity_scan);
            antigravity::activity(&scan.files, &scan.summaries, now, tz)
        })
        .filter(|a| settings.antigravity_enabled && a.total_sessions > 0);
        Ok(overview)
    }

    /// RTK's saving today, priced at the input price of the model that read the most fresh
    /// (non-cached) input today.
    fn rtk_summary(&self, store: &Store, settings: &Settings, tz: FixedOffset, now: DateTime<Utc>) -> Result<Option<RtkSummary>> {
        let Some(day) = lock(&self.rtk_today).clone() else { return Ok(None) };
        let today = now.with_timezone(&tz).date_naive();
        let saved_tokens = if day.date == today { day.saved_tokens } else { 0 };
        let midnight = today.and_time(NaiveTime::MIN).and_local_timezone(tz).single().map_or(now, |t| t.to_utc());
        let mut fresh_input: HashMap<String, u64> = HashMap::new();
        for e in store.events_between(midnight, now)? {
            // RTK does not run in Cursor, so its input says nothing about what RTK saved.
            if !is_enabled(settings, e.agent) || e.agent == Agent::Cursor {
                continue;
            }
            // Codex input includes the cached part.
            let n = if e.agent == Agent::Codex { e.tokens.input.saturating_sub(e.tokens.cache_read) } else { e.tokens.input };
            *fresh_input.entry(e.model).or_default() += n;
        }
        let model = fresh_input
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
            .map(|(m, _)| m);
        let approx_usd = rtk::approx_usd(saved_tokens, model.as_deref().and_then(|m| self.pricing().input_price(m)));
        Ok(Some(RtkSummary { saved_tokens, approx_usd, model }))
    }

    pub fn sessions(&self, mut filter: SessionFilter, tz: FixedOffset, now: DateTime<Utc>) -> Result<SessionPage> {
        filter.days = filter.days.clamp(1, MAX_DAYS);
        let settings = self.settings();
        let store = lock(&self.store);
        aggregate::sessions(&store, &mut lock(&self.projects), &settings, &filter, tz, now)
    }

    pub fn models(&self, period: Period, tz: FixedOffset, now: DateTime<Utc>) -> Result<ModelsView> {
        let (settings, pricing) = (self.settings(), self.pricing());
        aggregate::models(&lock(&self.store), &pricing, &settings, period, tz, now)
    }

    pub fn share_data(&self, request: &ShareRequest, tz: FixedOffset, now: DateTime<Utc>) -> Result<ShareData> {
        let settings = self.settings();
        crate::share::share_data(&lock(&self.store), &settings, request, tz, now)
    }

    pub fn projects(&self, period: Period, tz: FixedOffset, now: DateTime<Utc>) -> Result<ProjectsView> {
        let settings = self.settings();
        let store = lock(&self.store);
        aggregate::projects(&store, &mut lock(&self.projects), &settings, period, tz, now)
    }

    pub fn history(&self, days: u32, tz: FixedOffset, now: DateTime<Utc>) -> Result<HistoryView> {
        let settings = self.settings();
        aggregate::history(&lock(&self.store), &settings, days.clamp(1, MAX_DAYS), tz, now)
    }
}

/// Linear burn of the live 5-hour window from its stored samples.
fn claude_burn(store: &Store, windows: &[quota::QuotaWindow]) -> Result<Option<Burn>> {
    let Some(w) = windows.iter().find(|w| w.id == "five_hour" && !w.expired) else { return Ok(None) };
    let start = w.resets_at - chrono::Duration::hours(5);
    let points: Vec<(i64, f64)> = store
        .quota_samples(Agent::ClaudeCode, "five_hour", start)?
        .into_iter()
        .map(|(ts, used)| ((ts - start).num_minutes(), used))
        .collect();
    Ok(Some(quota::project(&points, start, w.resets_at)))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use chrono::{Duration as Span, NaiveDate};
    use rusqlite::Connection;

    use super::*;
    use crate::aggregate::testkit::{at, close, ev, now, store_priced, tok, tz};
    use crate::ingest::tests::{folder_state, wal_only_db};
    use crate::model::STACK_ORDER;
    use crate::quota::{binding_window, QuotaWindow};
    use crate::store::StoredEvent;

    const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    /// The fixture logs plus one-row Hermes and OpenCode databases created in `dir`.
    /// Codex gets both roots, like the real `sessions` + `archived_sessions`; here the second
    /// sits inside the first, so its file is found twice.
    fn fixture_roots(dir: &Path) -> Roots {
        let hermes = dir.join("state.db");
        Connection::open(&hermes)
            .unwrap()
            .execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, cwd TEXT, git_repo_root TEXT, started_at REAL);
                 CREATE TABLE session_model_usage (
                   session_id TEXT, model TEXT, billing_provider TEXT, billing_base_url TEXT, billing_mode TEXT,
                   task TEXT, api_call_count INTEGER, input_tokens INTEGER, output_tokens INTEGER,
                   cache_read_tokens INTEGER, cache_write_tokens INTEGER, reasoning_tokens INTEGER,
                   estimated_cost_usd REAL, actual_cost_usd REAL, cost_status TEXT, cost_source TEXT,
                   first_seen REAL, last_seen REAL);
                 INSERT INTO sessions VALUES ('hs1', '/tmp/hermes-proj', NULL, 1.0);
                 INSERT INTO session_model_usage VALUES
                   ('hs1','gpt-x','p','u','m',NULL,3,100,50,20,7,9,0.01,NULL,'s','src',1782120256.0,1782120256.0);",
            )
            .unwrap();
        let opencode = dir.join("opencode.db");
        Connection::open(&opencode)
            .unwrap()
            .execute_batch(
                r#"CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER,
                     time_updated INTEGER, data TEXT);
                   INSERT INTO message VALUES ('msg_a', 'ses_1', 1782120256885, 1782120256885,
                     '{"role":"assistant","modelID":"big-pickle","tokens":{"input":10,"output":5,"reasoning":0,
                       "cache":{"write":0,"read":0}},"path":{"cwd":"/tmp/oc-proj"}}');"#,
            )
            .unwrap();
        Roots {
            claude: format!("{FIX}/claude").into(),
            codex: vec![format!("{FIX}/codex").into(), format!("{FIX}/codex/archived_sessions").into()],
            hermes,
            opencode,
            cursor_projects: dir.join("no-cursor-projects"),
            antigravity: vec![],
            antigravity_summaries: PathBuf::new(),
        }
    }

    fn engine(store: Store, roots: Roots, settings: Settings) -> Engine {
        let snapshots = roots.hermes.with_file_name("buku-snapshots"); // created only for a real database
        Engine::new(store, Pricing::bundled(), settings, roots, PathBuf::from("/zz-buku-token-test-home"), snapshots)
    }

    #[test]
    fn one_update_builds_one_overview_for_the_menu_bar_and_the_notifications() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        let first = e.shared_overview(tz(), now()).unwrap();
        let second = e.shared_overview(tz(), now()).unwrap();
        assert!(Arc::ptr_eq(&first, &second), "same version, same overview");
        e.set_settings(Settings { usd_to_idr: 17000.0, ..Settings::default() });
        let third = e.shared_overview(tz(), now()).unwrap();
        assert!(!Arc::ptr_eq(&first, &third), "a change builds a new one");
    }

    #[test]
    fn the_antigravity_summaries_database_is_read_again_only_when_its_stamp_changed() {
        let home = tempfile::tempdir().unwrap();
        let e = engine(Store::open_in_memory().unwrap(), Roots::from_home(home.path()), Settings::default());
        let cli = home.path().join(crate::ingest::antigravity::SUMMARIES);
        std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
        let conn = Connection::open(&cli).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (conversation_id TEXT, title TEXT, preview TEXT, workspace_uris TEXT, step_count INTEGER, last_modified_time TEXT);
             INSERT INTO conversation_summaries VALUES ('a', 't', 'p', 'w', 40, '0001-01-01 00:00:00+00:00');",
        )
        .unwrap();
        drop(conn);
        let stamp = std::fs::metadata(&cli).unwrap();
        e.scan_all(|_| {}).unwrap();
        assert_eq!(lock(&e.antigravity_scan).summaries.len(), 1);

        // A second row lands in the same page, so the size stays; put the date back as it was.
        let conn = Connection::open(&cli).unwrap();
        conn.execute_batch("INSERT INTO conversation_summaries VALUES ('b', 't', 'p', 'w', 60, '0001-01-01 00:00:00+00:00');").unwrap();
        drop(conn);
        std::fs::OpenOptions::new().write(true).open(&cli).unwrap().set_modified(stamp.modified().unwrap()).unwrap();
        assert_eq!(std::fs::metadata(&cli).unwrap().len(), stamp.len(), "the size did not move");
        e.scan_all(|_| {}).unwrap();
        assert_eq!(lock(&e.antigravity_scan).summaries.len(), 1, "the same stamp is not opened again");

        std::fs::OpenOptions::new().write(true).open(&cli).unwrap().set_modified(stamp.modified().unwrap() + Duration::from_secs(5)).unwrap();
        e.scan_all(|_| {}).unwrap();
        assert_eq!(lock(&e.antigravity_scan).summaries.len(), 2, "a new stamp is read");
    }

    #[test]
    fn concurrent_settings_writers_keep_every_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let e = std::sync::Arc::new(engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default()));
        let patches: Vec<serde_json::Value> = vec![
            serde_json::json!({ "lastTab": "model" }),
            serde_json::json!({ "usdToIdr": 17000.0 }),
            serde_json::json!({ "theme": "dark" }),
            serde_json::json!({ "language": "en" }),
            serde_json::json!({ "notifyLowQuota": true }),
            serde_json::json!({ "hideOnScreenShare": true }),
        ];
        let threads: Vec<_> = patches
            .into_iter()
            .map(|patch| {
                let (e, path) = (e.clone(), path.clone());
                std::thread::spawn(move || {
                    e.update_settings(&path, |old| {
                        let fields = patch.as_object().unwrap().clone();
                        let merged = crate::settings::merge(old, fields)?;
                        // Widen the window between the read and the write.
                        std::thread::sleep(Duration::from_millis(20));
                        Ok(Some(merged))
                    })
                    .unwrap();
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let s = crate::settings::load(&path);
        assert_eq!(s.last_tab, "model");
        assert_eq!(s.usd_to_idr, 17000.0);
        assert_eq!(s.theme, crate::settings::Theme::Dark);
        assert_eq!(s.language, Lang::En);
        assert!(s.notify_low_quota && s.hide_on_screen_share);
        assert_eq!(e.settings(), s, "memory and file agree");
    }

    #[test]
    fn update_settings_with_no_change_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        assert!(e.update_settings(&path, |_| Ok(None)).unwrap().is_none());
        assert!(!path.exists());
    }

    #[test]
    fn a_new_cursor_session_clears_the_old_quota_row() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        e.set_cursor_quota(Err(QuotaError::Expired));
        assert!(e.cursor_last_reason().is_some());
        e.begin_cursor_session();
        assert!(e.cursor_last_reason().is_none(), "the old failure must not outlive the old cookie");
    }

    fn all_events(e: &Engine) -> Vec<StoredEvent> {
        lock(&e.store).events_between(DateTime::<Utc>::MIN_UTC, DateTime::<Utc>::MAX_UTC).unwrap()
    }

    fn count(e: &Engine, agent: Agent) -> usize {
        all_events(e).iter().filter(|x| x.agent == agent).count()
    }

    #[test]
    fn full_scan_over_fixtures_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let roots = fixture_roots(dir.path());
        let db = dir.path().join("buku.db");
        let e = engine(Store::open(&db).unwrap(), roots.clone(), Settings::default());
        let first = e.scan_all(|_| {}).unwrap();
        let n = all_events(&e).len();
        assert!(first.events > 0);
        // Cursor has no ingest yet (a later task adds it).
        for agent in STACK_ORDER.into_iter().filter(|a| *a != Agent::Cursor) {
            assert!(count(&e, agent) > 0, "{agent:?} has no events");
        }

        let second = e.scan_all(|_| {}).unwrap();
        assert_eq!(second.events, 0);
        assert_eq!(all_events(&e).len(), n);
        drop(e);

        // An app restart (fresh Codex contexts, same database) adds nothing either.
        let restarted = engine(Store::open(&db).unwrap(), roots, Settings::default());
        assert_eq!(restarted.scan_all(|_| {}).unwrap().events, 0);
        assert_eq!(all_events(&restarted).len(), n);
        let st = restarted.status();
        assert!(!st.running);
        assert_eq!(st.errors, 0);
    }

    #[test]
    fn scan_reports_progress_monotonic() {
        let dir = tempfile::tempdir().unwrap();
        let e = engine(Store::open_in_memory().unwrap(), fixture_roots(dir.path()), Settings::default());
        let seen = RefCell::new(Vec::new());
        e.scan_all(|p| seen.borrow_mut().push(p)).unwrap();
        let seen = seen.into_inner();
        // 2 Claude logs + 2 Codex logs (the one found under both roots counts once) + 2 databases
        assert!(seen.iter().all(|p| p.total == 6), "{seen:?}");
        assert_eq!(seen.first().unwrap().done, 0);
        assert!(seen.windows(2).all(|w| w[1].done == w[0].done + 1), "{seen:?}");
        assert_eq!(seen.last().unwrap().done, 6);
        let st = e.status();
        assert_eq!((st.running, st.done, st.total), (false, 6, 6));
        assert!(st.last_scan_at.is_some());
    }

    #[test]
    fn disabled_agent_is_not_scanned() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        settings.enabled_agents.insert(Agent::Codex, false);
        let e = engine(Store::open_in_memory().unwrap(), fixture_roots(dir.path()), settings.clone());
        let total = RefCell::new(0);
        e.scan_all(|p| *total.borrow_mut() = p.total).unwrap();
        assert_eq!(total.into_inner(), 4);
        assert_eq!(count(&e, Agent::Codex), 0);
        assert!(count(&e, Agent::ClaudeCode) > 0);

        // No cursor was stored while it was off, so turning it back on reads its whole history.
        settings.enabled_agents.insert(Agent::Codex, true);
        e.set_settings(settings);
        e.scan_all(|_| {}).unwrap();
        assert!(count(&e, Agent::Codex) > 0);
    }

    fn window(id: &str, used_pct: f64, resets_at: DateTime<Utc>, observed_at: DateTime<Utc>) -> QuotaWindow {
        QuotaWindow { id: id.into(), label: id.into(), used_pct, resets_at, observed_at, expired: false }
    }

    fn claude_quota(windows: Vec<QuotaWindow>) -> AgentQuota {
        AgentQuota {
            agent: Agent::ClaudeCode,
            plan: Some("Max 5×".into()),
            binding: binding_window(&windows),
            near_limit: false,
            windows,
            unavailable_reason: None,
            failure: None,
        }
    }

    fn no_roots() -> Roots {
        Roots::from_home(Path::new("/zz-buku-token-test-home"))
    }

    #[test]
    fn quota_poll_feeds_burn_and_failure_clears_windows() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        let resets = now() + Span::hours(3); // the 5-hour window started 2 hours ago
        let five = window("five_hour", 40.0, resets, now());
        let week = window("seven_day", 10.0, now() + Span::days(4), now());
        e.set_claude_quota(Ok(claude_quota(vec![five, week]))).unwrap();

        let store = lock(&e.store);
        assert_eq!(store.quota_samples(Agent::ClaudeCode, "five_hour", now() - Span::hours(5)).unwrap(), vec![(now(), 40.0)]);
        assert_eq!(store.quota_samples(Agent::ClaudeCode, "seven_day", now() - Span::days(7)).unwrap(), vec![(now(), 10.0)]);
        drop(store);
        let ov = e.overview(tz(), now()).unwrap();
        let burn = ov.burn.expect("burn from the five_hour samples");
        assert_eq!(burn.window_start, resets - Span::hours(5));
        assert_eq!(burn.points, vec![(120, 40.0)]);
        
        e.set_claude_quota(Err(QuotaError::Offline)).unwrap();
        let ov = e.overview(tz(), now()).unwrap();
        let q = ov.quotas.iter().find(|q| q.agent == Agent::ClaudeCode).unwrap();
        assert!(q.windows.is_empty() && q.binding.is_none() && !q.near_limit, "{q:?}");
        assert_eq!(q.unavailable_reason.as_deref(), Some("Sedang offline"));
        assert!(ov.burn.is_none());
    }

    #[test]
    fn a_missing_claude_login_is_hidden_unless_the_claude_folder_exists() {
        let home = tempfile::tempdir().unwrap();
        let e = engine(Store::open_in_memory().unwrap(), Roots::from_home(home.path()), Settings::default());
        e.set_claude_quota(Err(QuotaError::NotFound)).unwrap();
        let has_claude = |e: &Engine| e.overview(tz(), now()).unwrap().quotas.iter().any(|q| q.agent == Agent::ClaudeCode);
        assert!(!has_claude(&e), "no Claude folder, no Claude section");
        e.set_claude_quota(Err(QuotaError::Offline)).unwrap();
        assert!(has_claude(&e), "other failures still show");
        e.set_claude_quota(Err(QuotaError::NotFound)).unwrap();
        std::fs::create_dir_all(&e.roots().claude).unwrap();
        assert!(has_claude(&e), "with a Claude folder, the missing login is worth saying");
    }

    #[test]
    fn switching_the_language_rewords_quotas_already_in_memory_without_a_new_poll() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        let five = window("five_hour", 40.0, now() + Span::hours(3), now());
        let week = window("seven_day", 10.0, now() + Span::days(4), now());
        e.set_claude_quota(Ok(claude_quota(vec![five, week]))).unwrap();
        e.set_cursor_quota(Err(QuotaError::Expired));
        e.set_openrouter_at(e.openrouter_epoch(), Err(QuotaError::Offline));
        let labels = |lang: Lang| {
            let mut s = e.settings();
            s.language = lang;
            let before = e.version();
            e.set_settings(s);
            assert!(e.version() > before, "a language change bumps the version, so the UI is told");
            let ov = e.overview(tz(), now()).unwrap();
            let claude = ov.quotas.iter().find(|q| q.agent == Agent::ClaudeCode).unwrap();
            let cursor = ov.quotas.iter().find(|q| q.agent == Agent::Cursor).unwrap();
            let or = ov.openrouter.as_ref().unwrap();
            (
                claude.windows.iter().map(|w| w.label.clone()).collect::<Vec<_>>(),
                claude.binding.as_ref().map(|w| w.label.clone()),
                cursor.unavailable_reason.clone(),
                or.unavailable_reason.clone(),
                e.cursor_last_reason(),
            )
        };
        let id = labels(Lang::Id);
        assert_eq!(id.0, ["Sesi 5 jam", "Mingguan"]);
        assert_eq!(id.1.as_deref(), Some("Sesi 5 jam"));
        assert_eq!(id.2.as_deref(), Some("Sesi Cursor Habis, Tempel Ulang Di Pengaturan"));
        assert_eq!(id.3.as_deref(), Some("Sedang Offline"));
        let en = labels(Lang::En);
        assert_eq!(en.0, ["5h Session", "Weekly"]);
        assert_eq!(en.1.as_deref(), Some("5h Session"));
        assert_eq!(en.2.as_deref(), Some("Cursor Session Expired, Paste Again In Settings"));
        assert_eq!(en.3.as_deref(), Some("Offline"));
        assert_eq!(en.4.as_deref(), Some("Cursor Session Expired, Paste Again In Settings"));
        // Only ids reach SQLite.
        let store = lock(&e.store);
        let samples = store.quota_samples(Agent::ClaudeCode, "five_hour", now() - Span::hours(5)).unwrap();
        assert_eq!(samples, vec![(now(), 40.0)]);
    }

    #[test]
    fn quota_window_past_its_reset_is_expired_at_read_time() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        let polled = now() - Span::hours(2);
        e.set_claude_quota(Ok(claude_quota(vec![window("five_hour", 95.0, now() - Span::minutes(1), polled)]))).unwrap();
        let ov = e.overview(tz(), now()).unwrap();
        let q = &ov.quotas[0];
        assert!(q.windows[0].expired && q.binding.is_none() && !q.near_limit, "{q:?}");
        assert!(ov.burn.is_none());
    }

    #[test]
    fn rtk_saving_is_priced_with_todays_top_input_model() {
        let p = Pricing::bundled();
        let mut codex = ev(at(2026, 10, 30, 9, 0), Agent::Codex, "gpt-5", "", "c", tok(1000, 10), None);
        codex.tokens.cache_read = 950; // only 50 fresh input tokens
        let evs = vec![
            ev(at(2026, 10, 30, 8, 0), Agent::ClaudeCode, "claude-sonnet-4-5", "", "a", tok(100, 10), None),
            codex,
            ev(at(2026, 10, 29, 23, 0), Agent::ClaudeCode, "gpt-5", "", "b", tok(5000, 10), None), // yesterday
        ];
        let e = engine(store_priced(evs, &p), no_roots(), Settings::default());
        assert_eq!(e.overview(tz(), now()).unwrap().rtk, None);

        e.set_rtk(Some(RtkDay { date: NaiveDate::from_ymd_opt(2026, 10, 30).unwrap(), saved_tokens: 1000 }));
        let rtk = e.overview(tz(), now()).unwrap().rtk.unwrap();
        assert_eq!(rtk.saved_tokens, 1000);
        assert_eq!(rtk.model.as_deref(), Some("claude-sonnet-4-5"));
        close(rtk.approx_usd.unwrap(), 1000.0 * p.input_price("claude-sonnet-4-5").unwrap());
    }

    #[test]
    fn new_price_table_reprices_stored_events() {
        let evs = vec![ev(at(2026, 10, 30, 8, 0), Agent::ClaudeCode, "zz-new-model", "", "a", tok(1000, 0), None)];
        let e = engine(store_priced(evs, &Pricing::bundled()), no_roots(), Settings::default());
        assert_eq!(all_events(&e)[0].cost_usd, None);
        let table = r#"{"zz-new-model":{"input_cost_per_token":0.000002,"output_cost_per_token":0.00001}}"#;
        e.set_pricing(Pricing::from_json_str(table).unwrap().with_version("2026-10-30")).unwrap();
        close(all_events(&e)[0].cost_usd.unwrap(), 0.002);
        assert_eq!(e.status().pricing_version, "2026-10-30");
    }

    #[test]
    fn downloaded_price_table_wins_when_valid() {
        let dir = tempfile::tempdir().unwrap();
        let (json, version) = (dir.path().join("model_prices.json"), dir.path().join("model_prices.VERSION"));
        let bundled = Pricing::bundled().version();
        assert_eq!(load_pricing(&json, &version).version(), bundled);

        save_pricing(&json, &version, r#"{"m":{"input_cost_per_token":1e-6,"output_cost_per_token":2e-6}}"#, "2026-11-02")
            .unwrap();
        let p = load_pricing(&json, &version);
        assert_eq!(p.version(), "2026-11-02");
        assert_eq!(p.input_price("m"), Some(1e-6));

        std::fs::write(&json, "not json").unwrap();
        assert_eq!(load_pricing(&json, &version).version(), bundled);
    }

    #[test]
    fn history_and_session_days_are_clamped() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        assert_eq!(e.history(0, tz(), now()).unwrap().days.len(), 1);
        assert_eq!(e.history(10_000, tz(), now()).unwrap().days.len(), 60);
        let filter = SessionFilter { agent: None, project_path: None, before: None, days: 0 };
        assert!(!e.sessions(filter, tz(), now()).unwrap().has_more);
    }

    #[test]
    fn triggers_set_scan_and_quota_freshness() {
        let s = Duration::from_secs;
        let due = |t, scan_age, quota_age| {
            let (scan, quota) = thresholds(t);
            (is_due(Some(s(scan_age)), scan), is_due(Some(s(quota_age)), quota))
        };
        assert_eq!(due(Trigger::Tick, 59, 299), (false, false));
        assert_eq!(due(Trigger::Tick, 60, 300), (true, true));
        assert_eq!(due(Trigger::PopoverOpened, 29, 59), (false, false));
        assert_eq!(due(Trigger::PopoverOpened, 30, 60), (true, true));
        assert_eq!(due(Trigger::FilesChanged, 0, 299), (true, false));
        assert_eq!(due(Trigger::Refresh, 0, 0), (true, true));
        assert!(is_due(None, s(3600)));
    }

    #[test]
    fn only_log_and_db_changes_trigger_a_scan() {
        let r = Roots::from_home(Path::new("/Users/u"));
        assert!(r.is_relevant(Path::new("/Users/u/.claude/projects/-x/abc/subagents/agent-1.jsonl")));
        assert!(r.is_relevant(Path::new("/Users/u/.codex/archived_sessions/rollout-1.jsonl")));
        assert!(r.is_relevant(Path::new("/Users/u/.hermes/state.db-wal")));
        assert!(r.is_relevant(Path::new("/Users/u/.local/share/opencode/opencode.db")));
        assert!(!r.is_relevant(Path::new("/Users/u/.claude/projects/-x/notes.md")));
        assert!(!r.is_relevant(Path::new("/Users/u/.claude/todos/x.jsonl")));
        assert!(!r.is_relevant(Path::new("/Users/u/.hermes/logs/agent.log")));
        assert!(!r.is_relevant(Path::new("/Users/u/.hermes/other.db")));
    }

    #[test]
    fn database_noise_does_not_trigger_a_scan() {
        let r = Roots::from_home(Path::new("/Users/u"));
        assert!(r.is_relevant(Path::new("/Users/u/.hermes/state.db")));
        assert!(r.is_relevant(Path::new("/Users/u/.local/share/opencode/opencode.db-wal")));
        for noise in [
            "/Users/u/.hermes/state.db-shm",
            "/Users/u/.hermes/state.db-journal",
            "/Users/u/.hermes/state.db.fts_rebuild.lock",
            "/Users/u/.hermes/state.db.quarantine.lock",
            "/Users/u/.local/share/opencode/opencode.db-shm",
            "/Users/u/.local/share/opencode/opencode.db-journal",
            "/Users/u/.local/share/opencode/opencode.db.lock",
            "/Users/u/.hermes/sub/state.db-wal",
        ] {
            assert!(!r.is_relevant(Path::new(noise)), "{noise}");
        }
    }

    #[test]
    fn the_apps_own_folders_are_never_watched() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        for d in [
            ".claude/projects",
            ".codex/sessions",
            ".codex/archived_sessions",
            ".hermes",
            ".local/share/opencode",
            "Library/Application Support/local.toolslab.bukutoken",
            "Library/Caches/local.toolslab.bukutoken/snapshots/hermes",
        ] {
            std::fs::create_dir_all(h.join(d)).unwrap();
        }
        let r = Roots::from_home(h);
        let targets = r.watch_targets();
        assert_eq!(targets.len(), 5, "{targets:?}");
        let data = h.join("Library/Application Support/local.toolslab.bukutoken");
        let cache = h.join("Library/Caches/local.toolslab.bukutoken");
        for (t, _) in &targets {
            assert!(!data.starts_with(t) && !cache.starts_with(t), "{t:?} covers an app folder");
        }
        assert!(!r.is_relevant(&data.join("buku.db-wal")));
        assert!(!r.is_relevant(&cache.join("snapshots/hermes/state.db-wal")));
    }

    /// A Hermes and an OpenCode database with their rows only in the WAL, in folders of their own.
    fn wal_agents(home: &Path) -> Roots {
        let (hm, oc) = (home.join(".hermes"), home.join(".local/share/opencode"));
        std::fs::create_dir_all(&hm).unwrap();
        std::fs::create_dir_all(&oc).unwrap();
        wal_only_db(
            &hm.join("state.db"),
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, cwd TEXT, git_repo_root TEXT, started_at REAL);
             CREATE TABLE session_model_usage (
               session_id TEXT, model TEXT, billing_provider TEXT, billing_base_url TEXT, billing_mode TEXT,
               task TEXT, api_call_count INTEGER, input_tokens INTEGER, output_tokens INTEGER,
               cache_read_tokens INTEGER, cache_write_tokens INTEGER, reasoning_tokens INTEGER,
               estimated_cost_usd REAL, actual_cost_usd REAL, cost_status TEXT, cost_source TEXT,
               first_seen REAL, last_seen REAL);
             INSERT INTO session_model_usage VALUES
               ('hs1','gpt-x','p','u','m',NULL,3,100,50,20,7,9,0.01,NULL,'s','src',1782120256.0,1782120256.0);",
        );
        wal_only_db(
            &oc.join("opencode.db"),
            r#"CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER,
                 time_updated INTEGER, data TEXT);
               INSERT INTO message VALUES ('msg_a', 'ses_1', 1782120256885, 1782120256885,
                 '{"role":"assistant","modelID":"big-pickle","tokens":{"input":10,"output":5,"reasoning":0,
                   "cache":{"write":0,"read":0}},"path":{"cwd":"/tmp/oc-proj"}}');"#,
        );
        Roots {
            claude: home.join("no-claude"),
            codex: vec![],
            hermes: hm.join("state.db"),
            opencode: oc.join("opencode.db"),
            cursor_projects: home.join("no-cursor-projects"),
            antigravity: vec![],
            antigravity_summaries: PathBuf::new(),
        }
    }

    fn engine_with_snapshots(roots: Roots, snapshots: &Path) -> Engine {
        let store = Store::open_in_memory().unwrap();
        Engine::new(store, Pricing::bundled(), Settings::default(), roots, PathBuf::from("/zz-buku-token-test-home"), snapshots.to_path_buf())
    }

    /// Every regular file under `dir`, however deep.
    fn files_under(dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
        entries
            .flat_map(|e| {
                let p = e.unwrap().path();
                if p.is_dir() { files_under(&p) } else { vec![p] }
            })
            .collect()
    }

    #[test]
    fn scanning_reads_wal_rows_and_leaves_no_trace_anywhere() {
        let (home, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let roots = wal_agents(home.path());
        let folders = [roots.hermes.parent().unwrap().to_path_buf(), roots.opencode.parent().unwrap().to_path_buf()];
        let before = folders.iter().map(|d| folder_state(d)).collect::<Vec<_>>();

        let snapshots = cache.path().join("snapshots");
        let e = engine_with_snapshots(roots, &snapshots);
        for _ in 0..2 {
            e.scan_all(|_| {}).unwrap();
            assert_eq!(e.status().errors, 0);
            assert_eq!((count(&e, Agent::Hermes), count(&e, Agent::OpenCode)), (1, 1));
            assert_eq!(files_under(&snapshots), Vec::<PathBuf>::new(), "no copy stays after a read");
        }
        let after = folders.iter().map(|d| folder_state(d)).collect::<Vec<_>>();
        assert_eq!(after, before, "agent folders must stay byte-for-byte untouched");
    }

    #[test]
    fn unchanged_databases_are_not_copied_again_and_new_rows_are() {
        let (home, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let roots = wal_agents(home.path());
        let hermes = roots.hermes.clone();
        let snapshots = cache.path().join("snapshots");
        let e = engine_with_snapshots(roots, &snapshots);
        e.scan_all(|_| {}).unwrap();

        // A file where the snapshot folders go: any attempt to copy (or clean up) would fail.
        std::fs::create_dir_all(&snapshots).unwrap();
        for folder in ["hermes", "opencode"] {
            std::fs::write(snapshots.join(folder), b"in the way").unwrap();
        }
        e.scan_all(|_| {}).unwrap();
        assert_eq!(e.status().errors, 0, "nothing changed, so nothing is copied");
        assert_eq!(std::fs::read(snapshots.join("hermes")).unwrap(), b"in the way");

        // The owner writes a row: the WAL changes, so the next pass copies and reads it.
        std::fs::remove_file(snapshots.join("hermes")).unwrap();
        let owner = Connection::open(&hermes).unwrap();
        owner
            .execute_batch(
                "INSERT INTO session_model_usage VALUES
                   ('hs2','gpt-x','p','u','m',NULL,3,100,50,20,7,9,0.01,NULL,'s','src',1782120300.0,1782120300.0);",
            )
            .unwrap();
        drop(owner);
        e.scan_all(|_| {}).unwrap();
        assert_eq!(e.status().errors, 0);
        assert_eq!(count(&e, Agent::Hermes), 2);
        assert!(!snapshots.join("hermes").exists());
    }

    #[test]
    fn leftover_snapshots_are_removed_at_startup() {
        let (home, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let snapshots = cache.path().join("snapshots");
        std::fs::create_dir_all(snapshots.join("hermes")).unwrap();
        std::fs::write(snapshots.join("hermes/state.db"), b"left by a killed run").unwrap();
        // The agent is gone too, so no read would ever clean this up.
        let _e = engine_with_snapshots(Roots::from_home(home.path()), &snapshots);
        assert_eq!(files_under(&snapshots), Vec::<PathBuf>::new());
    }

    #[test]
    fn disabling_an_agent_removes_its_leftover_snapshot() {
        let (home, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let snapshots = cache.path().join("snapshots");
        let e = engine_with_snapshots(Roots::from_home(home.path()), &snapshots);
        for f in ["hermes/state.db", "opencode/opencode.db"] {
            std::fs::create_dir_all(snapshots.join(f).parent().unwrap()).unwrap();
            std::fs::write(snapshots.join(f), b"left behind").unwrap();
        }
        let mut settings = Settings::default();
        settings.enabled_agents.insert(Agent::Hermes, false);
        e.set_settings(settings);
        assert_eq!(files_under(&snapshots), vec![snapshots.join("opencode/opencode.db")]);
    }

    #[test]
    fn unreadable_snapshot_is_counted_and_retried() {
        let (home, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let roots = wal_agents(home.path());
        let hermes = roots.hermes.clone();
        std::fs::write(&hermes, b"this is not a sqlite database, like a copy torn mid-write").unwrap();
        std::fs::remove_file(hermes.with_file_name("state.db-wal")).unwrap();
        std::fs::remove_file(hermes.with_file_name("state.db-shm")).unwrap();
        let snapshots = cache.path().join("snapshots");
        let e = engine_with_snapshots(roots, &snapshots);
        // The stamp is unchanged on the second pass: it must still try again, not skip.
        for _ in 0..2 {
            e.scan_all(|_| {}).unwrap();
            assert_eq!(e.status().errors, 1);
            assert_eq!(count(&e, Agent::OpenCode), 1, "the other database still works");
            assert_eq!(files_under(&snapshots), Vec::<PathBuf>::new(), "a failed read leaves no copy either");
        }
        std::fs::remove_file(&hermes).unwrap();
        wal_only_db(
            &hermes,
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, cwd TEXT, git_repo_root TEXT, started_at REAL);
             CREATE TABLE session_model_usage (
               session_id TEXT, model TEXT, billing_provider TEXT, billing_base_url TEXT, billing_mode TEXT,
               task TEXT, api_call_count INTEGER, input_tokens INTEGER, output_tokens INTEGER,
               cache_read_tokens INTEGER, cache_write_tokens INTEGER, reasoning_tokens INTEGER,
               estimated_cost_usd REAL, actual_cost_usd REAL, cost_status TEXT, cost_source TEXT,
               first_seen REAL, last_seen REAL);",
        );
        e.scan_all(|_| {}).unwrap();
        assert_eq!(e.status().errors, 0);
    }

    #[test]
    fn claude_quota_needs_onboarding_and_respects_a_refused_keychain() {
        let settings = |onboarded| Settings { onboarded, ..Settings::default() };
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), settings(false));
        assert!(!e.wants_claude_quota(false), "the welcome screen explains the Keychain prompt first");
        assert!(!e.wants_claude_quota(true), "not even a manual refresh before onboarding");

        e.set_settings(settings(true));
        assert!(e.wants_claude_quota(false));

        e.set_claude_quota(Err(QuotaError::KeychainDenied)).unwrap();
        assert!(!e.wants_claude_quota(false), "no repeat prompts after a refusal");
        assert!(e.wants_claude_quota(true), "only the user asks again");

        e.set_claude_quota(Err(QuotaError::Offline)).unwrap();
        assert!(e.wants_claude_quota(false), "any other outcome clears the refusal");

        let mut off = settings(true);
        off.enabled_agents.insert(Agent::ClaudeCode, false);
        e.set_settings(off);
        assert!(!e.wants_claude_quota(true), "a disabled agent is never polled");
    }

    fn cursor_roots(home: &Path) -> Roots {
        Roots::from_home(home)
    }

    fn cursor_quota(windows: Vec<QuotaWindow>) -> AgentQuota {
        AgentQuota {
            agent: Agent::Cursor,
            plan: Some("Pro".into()),
            binding: windows.iter().find(|w| w.id == "total").cloned(),
            near_limit: false,
            windows,
            unavailable_reason: None,
            failure: None,
        }
    }

    #[test]
    fn wants_cursor_needs_onboarding_and_an_enabled_agent() {
        let home = tempfile::tempdir().unwrap();
        let roots = cursor_roots(home.path());
        let settings = |onboarded| Settings { onboarded, ..Settings::default() };

        let e = engine(Store::open_in_memory().unwrap(), roots, settings(false));
        assert!(!e.wants_cursor(), "not before onboarding");

        e.set_settings(settings(true));
        assert!(e.wants_cursor());

        let mut off = settings(true);
        off.enabled_agents.insert(Agent::Cursor, false);
        e.set_settings(off);
        assert!(!e.wants_cursor(), "a disabled agent is never polled");
    }

    #[test]
    fn rtk_estimate_ignores_cursor_input() {
        let p = Pricing::bundled();
        let evs = vec![
            ev(at(2026, 10, 30, 8, 0), Agent::ClaudeCode, "claude-sonnet-4-5", "", "a", tok(100, 10), None),
            // Far more input than Claude, on a model with no price.
            ev(at(2026, 10, 30, 9, 0), Agent::Cursor, "default", "", "c", tok(900_000, 10), None),
        ];
        let e = engine(store_priced(evs, &p), no_roots(), Settings::default());
        e.set_rtk(Some(RtkDay { date: NaiveDate::from_ymd_opt(2026, 10, 30).unwrap(), saved_tokens: 1000 }));
        let rtk = e.overview(tz(), now()).unwrap().rtk.unwrap();
        assert_eq!(rtk.model.as_deref(), Some("claude-sonnet-4-5"));
        assert!(rtk.approx_usd.is_some(), "the estimate must not disappear");
    }

    #[test]
    fn folder_list_includes_the_cursor_projects_folder() {
        let home = tempfile::tempdir().unwrap();
        let e = engine(Store::open_in_memory().unwrap(), cursor_roots(home.path()), Settings::default());
        let src = e.status().sources;
        let cursor: Vec<_> = src.iter().filter(|s| s.agent == Agent::Cursor).collect();
        assert_eq!(cursor.len(), 1);
        assert!(cursor[0].path.ends_with(".cursor/projects"));
        assert!(!cursor[0].found);
        std::fs::create_dir_all(home.path().join(".cursor/projects")).unwrap();
        assert!(e.status().sources.iter().any(|s| s.agent == Agent::Cursor && s.found));
    }

    #[test]
    fn cursor_keychain_refusal_sticks_until_connect_or_disconnect() {
        let home = tempfile::tempdir().unwrap();
        let e = engine(Store::open_in_memory().unwrap(), cursor_roots(home.path()), Settings::default());
        assert!(!e.cursor_keychain_denied());
        e.set_cursor_keychain_denied(true);
        assert!(e.cursor_keychain_denied());
        e.begin_cursor_session();
        assert!(!e.cursor_keychain_denied(), "Hubungkan clears it");
        e.set_cursor_keychain_denied(true);
        e.end_cursor_session();
        assert!(!e.cursor_keychain_denied(), "Putuskan clears it");
    }

    #[test]
    fn cursor_last_reason_follows_the_quota_result() {
        let home = tempfile::tempdir().unwrap();
        let e = engine(Store::open_in_memory().unwrap(), cursor_roots(home.path()), Settings::default());
        assert_eq!(e.cursor_last_reason(), None);
        e.set_cursor_quota(Err(QuotaError::Expired));
        assert_eq!(e.cursor_last_reason().as_deref(), Some("Sesi Cursor Habis, Tempel Ulang Di Pengaturan"));
        e.set_cursor_quota(Ok(cursor_quota(vec![])));
        assert_eq!(e.cursor_last_reason(), None);
    }

    #[test]
    fn cursor_history_backoff_is_set_and_cleared_explicitly() {
        let home = tempfile::tempdir().unwrap();
        let e = engine(Store::open_in_memory().unwrap(), cursor_roots(home.path()), Settings::default());
        assert!(e.cursor_history_due() && !e.cursor_history_failed());
        e.mark_cursor_history_failed(cursor_ingest::HISTORY_BACKOFF);
        assert!(!e.cursor_history_due() && e.cursor_history_failed());
        e.clear_cursor_history_backoff();
        assert!(e.cursor_history_due() && !e.cursor_history_failed());
        e.mark_cursor_history_failed(cursor_ingest::TRANSIENT_BACKOFF);
        e.begin_cursor_session();
        assert!(e.cursor_history_due(), "a new connect clears it");
        e.mark_cursor_history_failed(cursor_ingest::TRANSIENT_BACKOFF);
        e.end_cursor_session();
        assert!(e.cursor_history_due(), "a disconnect clears it");
    }

    #[test]
    fn clear_cursor_quota_removes_the_row_and_dates_the_poll() {
        let home = tempfile::tempdir().unwrap();
        let e = engine(Store::open_in_memory().unwrap(), cursor_roots(home.path()), Settings::default());
        e.set_cursor_quota(Err(QuotaError::Expired));
        let rows = |e: &Engine| e.overview(tz(), now()).unwrap().quotas.iter().filter(|q| q.agent == Agent::Cursor).count();
        assert_eq!(rows(&e), 1);
        e.clear_cursor_quota();
        assert_eq!(rows(&e), 0, "no unavailable row when not connected");
        assert!(e.cursor_age().is_some());
    }

    #[test]
    fn stale_quota_write_after_disconnect_does_not_bring_the_row_back() {
        let home = tempfile::tempdir().unwrap();
        let e = engine(Store::open_in_memory().unwrap(), cursor_roots(home.path()), Settings::default());
        let rows = |e: &Engine| e.overview(tz(), now()).unwrap().quotas.iter().filter(|q| q.agent == Agent::Cursor).count();
        let started = e.cursor_epoch();
        assert!(e.set_cursor_quota_at(started, Err(QuotaError::Expired)));
        assert_eq!(rows(&e), 1);
        e.end_cursor_session();
        assert_ne!(e.cursor_epoch(), started, "running rounds see the change");
        assert!(!e.set_cursor_quota_at(started, Ok(cursor_quota(vec![]))));
        assert_eq!(rows(&e), 0);
        // A new cookie also invalidates, and makes the next tick due.
        let before = e.cursor_epoch();
        e.begin_cursor_session();
        assert_ne!(e.cursor_epoch(), before);
        assert!(e.cursor_age().is_none());
        assert!(!e.set_cursor_quota_at(before, Err(QuotaError::Expired)));
    }

    #[test]
    fn cursor_quota_ok_and_error_and_age_is_separate() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        assert!(e.cursor_age().is_none());
        e.set_cursor_quota(Ok(cursor_quota(vec![window("total", 40.0, now() + Span::days(3), now())])));
        assert!(e.cursor_age().is_some());
        assert!(e.quota_age().is_none(), "Claude's schedule is untouched");
        let q = e.overview(tz(), now()).unwrap().quotas.into_iter().find(|q| q.agent == Agent::Cursor).unwrap();
        assert_eq!(q.plan.as_deref(), Some("Pro"));
        assert_eq!(q.binding.as_ref().map(|w| w.id.as_str()), Some("total"));

        e.set_cursor_quota(Err(QuotaError::Expired));
        let q = e.overview(tz(), now()).unwrap().quotas.into_iter().find(|q| q.agent == Agent::Cursor).unwrap();
        assert!(q.windows.is_empty() && q.plan.is_none() && q.binding.is_none() && !q.near_limit, "{q:?}");
        assert_eq!(q.unavailable_reason.as_deref(), Some("Sesi Cursor Habis, Tempel Ulang Di Pengaturan"));
        assert!(e.cursor_age().is_some(), "a failure is dated too, so it is not retried every tick");
    }

    #[test]
    fn cursor_window_past_its_reset_is_expired_at_read_time() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        e.set_cursor_quota(Ok(cursor_quota(vec![window("total", 99.0, now() - Span::minutes(1), now() - Span::hours(2))])));
        let q = e.overview(tz(), now()).unwrap().quotas.into_iter().find(|q| q.agent == Agent::Cursor).unwrap();
        assert!(q.windows[0].expired && q.binding.is_none() && !q.near_limit, "{q:?}");
    }

    #[test]
    fn quotas_are_ordered_claude_codex_cursor() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        e.set_cursor_quota(Ok(cursor_quota(vec![window("total", 10.0, now() + Span::days(3), now())])));
        e.set_claude_quota(Ok(claude_quota(vec![window("five_hour", 10.0, now() + Span::hours(3), now())]))).unwrap();
        *lock(&e.codex_limits) = Some(CodexRateLimits {
            observed_at: now(),
            plan: None,
            primary: None,
            secondary: None,
        });
        let agents: Vec<Agent> = e.overview(tz(), now()).unwrap().quotas.iter().map(|q| q.agent).collect();
        assert_eq!(agents, vec![Agent::ClaudeCode, Agent::Codex, Agent::Cursor]);
    }

    #[test]
    fn cursor_events_upsert_and_reach_the_aggregates() {
        use crate::model::Tokens;
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        assert_eq!(e.cursor_watermark().unwrap(), 0);
        let mk = |cost: f64| UsageEvent {
            ts: now() - Span::hours(1),
            agent: Agent::Cursor,
            model: "default".into(),
            cwd: String::new(),
            session_id: "c1".into(),
            tokens: Tokens { input: 10, output: 5, ..Tokens::default() },
            reported_cost_usd: Some(cost),
            dedup_key: "cr:e1".into(),
        };
        e.store_cursor_events(&[mk(1.0)]).unwrap();
        e.store_cursor_events(&[mk(2.5)]).unwrap();
        let cr: Vec<_> = all_events(&e).into_iter().filter(|x| x.agent == Agent::Cursor).collect();
        assert_eq!(cr.len(), 1, "same usageEventId is replaced, not duplicated");
        let ov = e.overview(tz(), now()).unwrap();
        assert!((ov.today.cost_usd - 2.5).abs() < 1e-9, "{}", ov.today.cost_usd);
        e.set_cursor_watermark(1234).unwrap();
        assert_eq!(e.cursor_watermark().unwrap(), 1234);
    }

    #[test]
    fn cursor_database_never_triggers_a_scan() {
        let r = Roots::from_home(Path::new("/Users/u"));
        let dir = "/Users/u/Library/Application Support/Cursor/User/globalStorage";
        for f in ["state.vscdb", "state.vscdb-wal", "state.vscdb-shm", "state.vscdb-journal"] {
            assert!(!r.is_relevant(Path::new(&format!("{dir}/{f}"))), "{f}");
        }
        assert!(!r.is_relevant(Path::new("/Users/u/.cursor/projects/x/agent-transcripts/a.jsonl")));
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("Library/Application Support/Cursor/User/globalStorage")).unwrap();
        std::fs::create_dir_all(home.path().join(".cursor/projects")).unwrap();
        assert!(Roots::from_home(home.path()).watch_targets().is_empty(), "Cursor folders are not watched");
    }

    // ---- OpenRouter and Antigravity: outside the agents and every total ----

    fn or_data() -> quota::openrouter::OpenRouterData {
        quota::openrouter::parse_key(include_str!("../tests/fixtures/openrouter_key.json")).unwrap()
    }

    fn or_view(e: &Engine) -> Option<quota::openrouter::OpenRouterView> {
        e.overview(tz(), now()).unwrap().openrouter
    }

    #[test]
    fn wants_openrouter_needs_onboarding_and_its_own_switch() {
        let settings = |onboarded| Settings { onboarded, ..Settings::default() };
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), settings(false));
        assert!(!e.wants_openrouter(), "not before onboarding");
        e.set_settings(settings(true));
        assert!(e.wants_openrouter());
        e.set_settings(Settings { openrouter_enabled: false, ..settings(true) });
        assert!(!e.wants_openrouter(), "its own switch stops the polling");
        let mut no_cursor = settings(true);
        no_cursor.enabled_agents.insert(Agent::Cursor, false);
        e.set_settings(no_cursor);
        assert!(e.wants_openrouter(), "the agent switches are not its switch");
    }

    #[test]
    fn openrouter_ok_and_error_show_in_the_overview_and_the_poll_is_dated() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        assert!(e.openrouter_age().is_none() && or_view(&e).is_none());
        assert!(e.set_openrouter_at(e.openrouter_epoch(), Ok(or_data())));
        let v = or_view(&e).unwrap();
        assert_eq!((v.data.unwrap().month_usd, v.unavailable_reason), (21.5, None));
        assert!(e.openrouter_age().is_some());
        assert!(e.cursor_age().is_none() && e.quota_age().is_none(), "other schedules are untouched");

        e.set_openrouter_at(e.openrouter_epoch(), Err(QuotaError::Expired));
        let v = or_view(&e).unwrap();
        assert!(v.data.is_none(), "a failure never leaves old numbers");
        assert_eq!(v.unavailable_reason.as_deref(), Some("Key OpenRouter Ditolak, Tempel Ulang Di Pengaturan"));
        assert_eq!(e.openrouter_last_reason().as_deref(), Some("Key OpenRouter Ditolak, Tempel Ulang Di Pengaturan"));
    }

    #[test]
    fn a_switched_off_openrouter_leaves_the_overview() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        e.set_openrouter_at(e.openrouter_epoch(), Ok(or_data()));
        assert!(or_view(&e).is_some());
        e.set_settings(Settings { openrouter_enabled: false, ..Settings::default() });
        assert!(or_view(&e).is_none());
    }

    #[test]
    fn disconnecting_openrouter_bumps_the_epoch_and_drops_late_answers() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        let started = e.openrouter_epoch();
        assert!(e.set_openrouter_at(started, Ok(or_data())));
        e.end_openrouter_session();
        assert_ne!(e.openrouter_epoch(), started, "running rounds see the change");
        assert!(or_view(&e).is_none(), "Putuskan removes the section");
        assert!(!e.set_openrouter_at(started, Ok(or_data())), "an answer for the old key is dropped");
        assert!(or_view(&e).is_none());
        let before = e.openrouter_epoch();
        e.begin_openrouter_session();
        assert_ne!(e.openrouter_epoch(), before, "a new key also ends rounds using the old one");
        assert!(e.openrouter_age().is_none(), "and makes the next tick poll");
        assert!(!e.set_openrouter_at(before, Err(QuotaError::Expired)));
    }

    #[test]
    fn clear_openrouter_removes_the_section_and_dates_the_poll() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        e.set_openrouter_at(e.openrouter_epoch(), Ok(or_data()));
        let stale = e.openrouter_epoch();
        e.begin_openrouter_session();
        e.set_openrouter_at(e.openrouter_epoch(), Ok(or_data()));
        e.clear_openrouter_at(stale);
        assert!(or_view(&e).is_some(), "a round that started before the new key cannot clear it");
        e.clear_openrouter_at(e.openrouter_epoch());
        assert!(or_view(&e).is_none() && e.openrouter_age().is_some());
    }

    #[test]
    fn a_refused_keychain_removes_old_figures_and_a_stale_round_cannot_set_the_flag() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        let started = e.openrouter_epoch();
        e.set_openrouter_at(started, Ok(or_data()));
        assert!(e.deny_openrouter_keychain_at(started));
        assert!(e.openrouter_keychain_denied() && or_view(&e).is_none(), "the status command clears the view");
        e.set_openrouter_keychain_denied(false);
        e.set_openrouter_at(started, Ok(or_data()));
        e.end_openrouter_session(); // a Putuskan or a new key while the Keychain was being read
        e.set_openrouter_at(e.openrouter_epoch(), Ok(or_data()));
        assert!(!e.deny_openrouter_keychain_at(started), "a read that began before the change is ignored");
        assert!(!e.openrouter_keychain_denied() && or_view(&e).is_some());

        e.begin_openrouter_session();
        e.set_openrouter_at(e.openrouter_epoch(), Ok(or_data()));
        assert!(or_view(&e).is_some());
        e.begin_openrouter_session();
        assert!(or_view(&e).is_none(), "a new key drops the old key's figures at once");

        assert!(!e.set_openrouter_at(started, Err(QuotaError::KeychainDenied)));
        assert!(!e.openrouter_keychain_denied(), "a round of an older key cannot refuse the new one");
        assert!(e.set_openrouter_at(e.openrouter_epoch(), Err(QuotaError::KeychainDenied)));
        assert!(e.openrouter_keychain_denied());
    }

    #[test]
    fn openrouter_keychain_refusal_and_backoff_are_cleared_by_connect_and_disconnect() {
        let e = engine(Store::open_in_memory().unwrap(), no_roots(), Settings::default());
        assert!(!e.openrouter_keychain_denied() && e.openrouter_due());
        e.set_openrouter_keychain_denied(true);
        e.mark_openrouter_failed(cursor_ingest::TRANSIENT_BACKOFF);
        assert!(e.openrouter_keychain_denied() && !e.openrouter_due());
        e.begin_openrouter_session();
        assert!(!e.openrouter_keychain_denied() && e.openrouter_due(), "Hubungkan clears both");
        e.set_openrouter_keychain_denied(true);
        e.mark_openrouter_failed(cursor_ingest::HISTORY_BACKOFF);
        e.end_openrouter_session();
        assert!(!e.openrouter_keychain_denied() && e.openrouter_due(), "Putuskan clears both");
        e.mark_openrouter_failed(cursor_ingest::TRANSIENT_BACKOFF);
        e.clear_openrouter_backoff();
        assert!(e.openrouter_due(), "Refresh clears the backoff");
        assert!(e.cursor_history_due(), "Cursor's own backoff is separate");
    }

    #[test]
    fn openrouter_and_antigravity_cannot_change_the_donut_totals_quotas_or_share_card() {
        use crate::model::Tokens;
        let home = tempfile::tempdir().unwrap();
        let mk = |agent, cost| UsageEvent {
            ts: now() - Span::hours(1),
            agent,
            model: "m".into(),
            cwd: String::new(),
            session_id: "s".into(),
            tokens: Tokens { input: 10, output: 5, ..Tokens::default() },
            reported_cost_usd: Some(cost),
            dedup_key: format!("{agent:?}"),
        };
        let build = |with: bool| {
            let e = engine(Store::open_in_memory().unwrap(), Roots::from_home(home.path()), Settings::default());
            e.store_cursor_events(&[mk(Agent::OpenCode, 2.0), mk(Agent::Hermes, 1.0)]).unwrap();
            e.set_cursor_quota(Ok(cursor_quota(vec![window("total", 40.0, now() + Span::days(3), now())])));
            if with {
                e.set_openrouter_at(e.openrouter_epoch(), Ok(or_data()));
                for dir in crate::ingest::antigravity::folders(home.path()) {
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::File::create(dir.join("conv.pb")).unwrap().set_modified(std::time::SystemTime::from(now() - Span::hours(1))).unwrap();
                }
                e.scan_all(|_| {}).unwrap();
            }
            e
        };
        let (plain, both) = (build(false), build(true));
        let (a, b) = (plain.overview(tz(), now()).unwrap(), both.overview(tz(), now()).unwrap());
        assert!(b.openrouter.is_some(), "the data is there");
        assert!(b.antigravity.is_some() && a.antigravity.is_none(), "the activity is there too");
        assert_eq!((&a.today, &a.quotas, &a.burn, &a.rtk), (&b.today, &b.quotas, &b.burn, &b.rtk));
        assert!(a.today.by_agent.len() == 5 && b.today.cost_usd == 3.0, "only the stored agent events add up");
        assert!(b.quotas.iter().all(|q| q.agent != Agent::OpenCode) && b.quotas.len() == 1);
        let req = ShareRequest { period: Period::Today, agent: None, hide_cost: false, dark: false };
        assert_eq!(plain.share_data(&req, tz(), now()).unwrap(), both.share_data(&req, tz(), now()).unwrap());
        assert_eq!(all_events(&plain).len(), all_events(&both).len(), "nothing is stored as usage");
        let items = |o: &Overview| crate::tray::tray_items(&o.quotas, crate::settings::QuotaDisplay::Left);
        assert_eq!(items(&a), items(&b));
    }

    #[test]
    fn antigravity_activity_comes_from_the_scan_and_follows_its_own_switch() {
        let home = tempfile::tempdir().unwrap();
        let e = engine(Store::open_in_memory().unwrap(), Roots::from_home(home.path()), Settings::default());
        assert!(e.overview(tz(), now()).unwrap().antigravity.is_none(), "no folders, no row");
        let [a, b] = [0, 1].map(|i| crate::ingest::antigravity::folders(home.path())[i].clone());
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let recent = std::time::SystemTime::from(now() - Span::hours(1));
        for (dir, name) in [(&a, "one.pb"), (&b, "one.pb"), (&b, "two.pb")] {
            std::fs::File::create(dir.join(name)).unwrap().set_modified(recent).unwrap();
        }
        assert!(e.overview(tz(), now()).unwrap().antigravity.is_none(), "the schedule is the scan's");
        e.scan_all(|_| {}).unwrap();
        let act = e.overview(tz(), now()).unwrap().antigravity.unwrap();
        assert_eq!((act.today, act.last_7_days), (2, 2), "one conversation in both folders counts once");
        e.set_settings(Settings { antigravity_enabled: false, ..Settings::default() });
        assert!(e.overview(tz(), now()).unwrap().antigravity.is_none());
        e.scan_all(|_| {}).unwrap();
        e.set_settings(Settings::default());
        assert!(e.overview(tz(), now()).unwrap().antigravity.is_none(), "a switched-off scan keeps nothing");
    }

    #[test]
    fn antigravity_folder_is_listed_with_the_folders_read_but_is_not_an_agent_source() {
        let home = tempfile::tempdir().unwrap();
        let e = engine(Store::open_in_memory().unwrap(), Roots::from_home(home.path()), Settings::default());
        let st = e.status();
        assert!(st.sources.iter().all(|s| s.path.find("gemini").is_none()));
        let names: Vec<_> = st.activity_sources.iter().map(|s| (s.name.as_str(), s.found)).collect();
        assert_eq!(names, [("Antigravity", false), ("Antigravity", false), ("Antigravity", false)]);
        assert!(st.activity_sources[2].path.ends_with(".gemini/antigravity-cli/conversation_summaries.db"));
        assert!(st.activity_sources[0].path.ends_with(".gemini/antigravity/conversations"));
        std::fs::create_dir_all(&crate::ingest::antigravity::folders(home.path())[1]).unwrap();
        assert!(e.status().activity_sources[1].found);
        assert!(Roots::from_home(home.path()).watch_targets().is_empty(), "no new watcher");
    }

    #[test]
    fn old_antigravity_sessions_still_show_with_totals_and_the_last_date() {
        let home = tempfile::tempdir().unwrap();
        let e = engine(Store::open_in_memory().unwrap(), Roots::from_home(home.path()), Settings::default());
        let dir = crate::ingest::antigravity::folders(home.path())[0].clone();
        std::fs::create_dir_all(&dir).unwrap();
        let old = std::time::SystemTime::from(now() - Span::days(60));
        std::fs::File::create(dir.join("a.pb")).unwrap().set_modified(old).unwrap();
        let cli = home.path().join(crate::ingest::antigravity::SUMMARIES);
        std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
        let conn = Connection::open(&cli).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (conversation_id TEXT, title TEXT, preview TEXT, workspace_uris TEXT, step_count INTEGER, last_modified_time TEXT);
             INSERT INTO conversation_summaries VALUES ('a', 't', 'p', 'w', 40, '0001-01-01 00:00:00+00:00');
             INSERT INTO conversation_summaries VALUES ('b', 't', 'p', 'w', 60, '0001-01-01 00:00:00+00:00');",
        )
        .unwrap();
        drop(conn);
        e.scan_all(|_| {}).unwrap();
        let act = e.overview(tz(), now()).unwrap().antigravity.expect("shown although nothing is recent");
        assert_eq!((act.today, act.last_7_days, act.total_sessions, act.total_steps), (0, 0, 2, 100));
        assert_eq!(act.last_active.map(|t| t.timestamp()), Some((now() - Span::days(60)).timestamp()));
        e.set_settings(Settings { antigravity_enabled: false, ..Settings::default() });
        assert!(e.overview(tz(), now()).unwrap().antigravity.is_none(), "off hides it");
    }
}
