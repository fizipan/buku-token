//! Menu bar item, style F: one logo + remaining-quota percentage per agent, drawn into an image.
//! `tray_items` and `render` are pure; the Tauri wiring lives at the bottom of the file.

use crate::model::Agent;
use crate::quota::AgentQuota;
use crate::settings::QuotaDisplay;
use resvg::{tiny_skia, usvg};
use std::future::Future;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{Local, Utc};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, LogicalPosition, Manager, Rect, Theme, WindowEvent};

use crate::commands;
use crate::engine::Engine;
use crate::i18n;
use crate::settings::Lang;

/// Height of the menu bar image in pixels (18 pt @2x).
pub const HEIGHT: u32 = 36;

#[derive(Clone, Debug, PartialEq)]
pub struct TrayItem {
    /// `None` is the neutral placeholder mark, shown while no agent has data.
    pub agent: Option<Agent>,
    /// "62%", or "–" when the quota is unknown.
    pub text: String,
    pub alert: bool,
}

pub struct TrayImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// True when macOS may recolor the image for the menu bar (no alert present).
    pub template: bool,
}

/// One item per agent that has quota data (Claude Code, Codex, Cursor), in that order. The percentage is
/// the binding window's remaining (or, with `QuotaDisplay::Used`, used) quota; "–" when no window is live.
pub fn tray_items(quotas: &[AgentQuota], display: QuotaDisplay) -> Vec<TrayItem> {
    [Agent::ClaudeCode, Agent::Codex, Agent::Cursor]
        .into_iter()
        .filter_map(|agent| {
            let q = quotas.iter().find(|q| q.agent == agent)?;
            let remaining = q.binding.as_ref().map(|w| w.remaining()).filter(|r| r.is_finite());
            let shown = remaining.map(|r| r.clamp(0.0, 100.0)).map(|r| if display == QuotaDisplay::Used { 100.0 - r } else { r });
            let text = shown.map_or_else(|| DASH.to_string(), |p| format!("{}%", p.round()));
            Some(TrayItem { agent: Some(agent), text, alert: q.near_limit })
        })
        .collect()
}

/// While the screen is shared the menu bar keeps its logos but no figure and no alert color.
pub fn hidden_items(items: &[TrayItem]) -> Vec<TrayItem> {
    items.iter().map(|i| TrayItem { agent: i.agent, text: String::new(), alert: false }).collect()
}

/// The items to draw. `None` (the overview could not be read) or no agent with data gives the
/// placeholder; while hidden, either way draws logos only.
pub fn choose_items(quotas: Option<&[AgentQuota]>, display: QuotaDisplay, hidden: bool) -> Vec<TrayItem> {
    let items = quotas.map(|q| tray_items(q, display)).filter(|i| !i.is_empty()).unwrap_or_else(placeholder_items);
    if hidden { hidden_items(&items) } else { items }
}

/// What the tray shows before the first data arrives, or when no agent has any: a neutral mark, so a
/// Mac without Claude Code does not wear its logo.
pub fn placeholder_items() -> Vec<TrayItem> {
    vec![TrayItem { agent: None, text: DASH.into(), alert: false }]
}

const DASH: &str = "–";
const ALERT_COLOR: &str = "#ff5f57";
const LOGO_PX: f32 = 30.0; // 15 pt @2x
const LOGO_TEXT_GAP_PX: f32 = 8.0; // 4 pt @2x
const PAIR_GAP_PX: f32 = 12.0; // 6 pt @2x
const FONT_PX: f32 = 26.0; // 13 pt @2x
/// Baseline that centers digits (cap height about 0.7 em) in the 36 px strip.
const BASELINE_PX: f32 = 27.2;
const FONT_FILES: [&str; 2] = ["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc"];

fn logo_svg(agent: Option<Agent>) -> &'static str {
    match agent {
        None => include_str!("../assets/logos/neutral.svg"),
        Some(Agent::Codex) => include_str!("../assets/logos/openai.svg"),
        Some(Agent::Cursor) => include_str!("../assets/logos/cursor.svg"),
        // Only Claude Code, Codex and Cursor reach the tray; anything else would be a bug upstream.
        _ => include_str!("../assets/logos/claude.svg"),
    }
}

pub(crate) type Font = (std::sync::Arc<usvg::fontdb::Database>, String);

/// The system font database and the family name to ask for, or `None` when no font file loads.
fn load_font() -> Option<Font> {
    let mut db = usvg::fontdb::Database::new();
    for path in FONT_FILES {
        if db.load_font_file(path).is_ok() {
            break;
        }
    }
    let family = db.faces().next()?.families.first()?.0.clone();
    Some((std::sync::Arc::new(db), family))
}

/// [`load_font`], once per app run: the file is several MB and the icon redraws every few seconds
/// while agents work.
pub(crate) fn font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| {
        let loaded = load_font();
        if loaded.is_none() {
            eprintln!("[buku-token] tray: no system font found, the menu bar shows logos only");
        }
        loaded
    })
    .as_ref()
}

fn text_svg(text: &str, x: f32, family: &str, fill: &str) -> String {
    format!(
        r#"<text x="{x}" y="{BASELINE_PX}" font-family="{family}" font-size="{FONT_PX}" font-weight="500" fill="{fill}">{text}</text>"#
    )
}

fn tree_from(svg: &str, fonts: Option<&std::sync::Arc<usvg::fontdb::Database>>) -> Option<usvg::Tree> {
    let mut opt = usvg::Options::default();
    if let Some(db) = fonts {
        opt.fontdb = db.clone();
    }
    usvg::Tree::from_str(svg, &opt).ok()
}

/// Width of `text` in the menu bar font, from the laid-out text's bounding box.
fn text_width(text: &str, family: &str, fonts: &std::sync::Arc<usvg::fontdb::Database>) -> f32 {
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="600" height="{HEIGHT}">{}</svg>"#,
        text_svg(text, 0.0, family, "#000")
    );
    tree_from(&svg, Some(fonts)).map_or(0.0, |t| t.root().abs_bounding_box().right().max(0.0))
}

/// Draws the items left to right: logo, gap, percentage, then a wider gap to the next pair.
/// Glyphs are black on transparent for template mode; with an alert the image is not a template,
/// the alerting logo is red and the rest follow `dark_menu_bar` (white on dark, black on light).
pub fn render(items: &[TrayItem], dark_menu_bar: bool) -> TrayImage {
    let template = !items.iter().any(|i| i.alert);
    let neutral = match (template, dark_menu_bar) {
        (true, _) => "#000000",
        (false, true) => "#ffffff",
        (false, false) => "#000000",
    };
    let font = font();
    let logo_y = (HEIGHT as f32 - LOGO_PX) / 2.0;

    let (mut body, mut x) = (String::new(), 0.0_f32);
    for (n, item) in items.iter().enumerate() {
        if n > 0 {
            x += PAIR_GAP_PX;
        }
        let color = if item.alert { ALERT_COLOR } else { neutral };
        let logo = logo_svg(item.agent)
            .replace("fill=\"currentColor\"", &format!("fill=\"{color}\" x=\"{x}\" y=\"{logo_y}\""))
            .replace("height=\"1em\"", &format!("height=\"{LOGO_PX}\""))
            .replace("width=\"1em\"", &format!("width=\"{LOGO_PX}\""));
        body.push_str(&logo);
        x += LOGO_PX;
        if let (Some((db, family)), false) = (font, item.text.is_empty()) {
            x += LOGO_TEXT_GAP_PX;
            body.push_str(&text_svg(&item.text, x, family, neutral));
            x += text_width(&item.text, family, db);
        }
    }

    let width = (x.ceil() as u32).max(1);
    let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{HEIGHT}">{body}</svg>"#);
    let mut pixmap = tiny_skia::Pixmap::new(width, HEIGHT).expect("non-zero size");
    if let Some(tree) = tree_from(&svg, font.map(|(db, _)| db)) {
        resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    }
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    TrayImage { rgba, width, height: HEIGHT, template }
}

// --- Tauri wiring ---------------------------------------------------------------------------

const TRAY_ID: &str = "main";
const POPOVER: &str = "popover";
const SETTINGS_WINDOW: &str = "settings";
/// A tray click this soon after the popover hid itself on blur is the click that closed it.
const BLUR_CLICK_GUARD: Duration = Duration::from_millis(200);

struct TrayState {
    last_blur_hide: Mutex<Option<Instant>>,
    /// The menu items and the language their text is in, so a language change can reword them.
    menu: Mutex<(Vec<MenuItem<tauri::Wry>>, Lang)>,
}

/// The (item id, text) pairs the menu shows in `lang`.
pub fn menu_entries(lang: Lang) -> [(&'static str, &'static str); 3] {
    let t = i18n::tray_menu(lang);
    [("refresh", t.refresh), ("settings", t.settings), ("quit", t.quit)]
}

/// Rewords the menu (and the settings window title) when the language changed since the last time.
fn sync_language(app: &AppHandle) {
    let lang = app.state::<Engine>().settings().language;
    let state = app.state::<TrayState>();
    let mut menu = lock(&state.menu);
    if menu.1 == lang {
        return;
    }
    let entries = menu_entries(lang);
    for item in &menu.0 {
        if let Some((_, text)) = entries.iter().find(|(id, _)| item.id().as_ref() == *id)
            && let Err(e) = item.set_text(text)
        {
            eprintln!("[buku-token] tray menu text: {e}");
        }
    }
    menu.1 = lang;
    sync_title(app, lang);
}

/// The settings window's title in `lang`.
fn sync_title(app: &AppHandle, lang: Lang) {
    if let Some(win) = app.get_webview_window(SETTINGS_WINDOW) {
        let _ = win.set_title(i18n::settings_title(lang));
    }
}

pub fn setup(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let lang = app.state::<Engine>().settings().language;
    let [refresh, settings, quit] = menu_entries(lang);
    let items = vec![
        MenuItem::with_id(app, refresh.0, refresh.1, true, None::<&str>)?,
        MenuItem::with_id(app, settings.0, settings.1, true, None::<&str>)?,
        MenuItem::with_id(app, quit.0, quit.1, true, None::<&str>)?,
    ];
    let menu = Menu::with_items(app, &[&items[0], &items[1], &PredefinedMenuItem::separator(app)?, &items[2]])?;
    app.manage(TrayState { last_blur_hide: Mutex::new(None), menu: Mutex::new((items, lang)) });
    sync_title(app.handle(), lang);
    let first = render(&placeholder_items(), is_dark(app.handle()));
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::new_owned(first.rgba, first.width, first.height))
        .icon_as_template(first.template)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "refresh" => spawn_command(app, "refresh", |a| async move { commands::refresh(a).await }),
            "settings" => spawn_command(app, "open_settings", |a| async move { commands::open_settings(a).await }),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = event {
                toggle_popover(tray.app_handle(), &rect);
            }
        })
        .build(app)?;

    if let Some(win) = app.get_webview_window(POPOVER) {
        let handle = app.handle().clone();
        win.on_window_event(move |e| {
            if let WindowEvent::Focused(false) = e {
                if let Some(win) = handle.get_webview_window(POPOVER) {
                    let _ = win.hide();
                }
                *lock(&handle.state::<TrayState>().last_blur_hide) = Some(Instant::now());
            }
        });
    }

    let handle = app.handle().clone();
    app.listen(commands::DATA_UPDATED, move |_| {
        let app = handle.clone();
        // The overview reads SQLite; keep it off the event thread.
        tauri::async_runtime::spawn_blocking(move || {
            sync_language(&app);
            update_icon(&app);
        });
    });
    // The scheduler is already running: catch an update that fired before the listener existed.
    let app = app.handle().clone();
    tauri::async_runtime::spawn_blocking(move || update_icon(&app));
    Ok(())
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn spawn_command<F, Fut>(app: &AppHandle, name: &'static str, f: F)
where
    F: FnOnce(AppHandle) -> Fut,
    Fut: Future<Output = Result<(), String>> + Send + 'static,
{
    let fut = f(app.clone());
    tauri::async_runtime::spawn(async move {
        if let Err(e) = fut.await {
            eprintln!("[buku-token] tray menu {name}: {e}");
        }
    });
}

/// Approximation: follows the system theme as the popover reports it. The macOS menu bar can
/// also follow the wallpaper, so this can be wrong. It only matters while an alert is shown.
fn is_dark(app: &AppHandle) -> bool {
    app.get_webview_window(POPOVER).and_then(|w| w.theme().ok()).is_some_and(|t| t == Theme::Dark)
}

pub(crate) fn update_icon(app: &AppHandle) {
    // Renders run on separate blocking threads: one at a time, each reading the latest data, so
    // an older image can never replace a newer one.
    static RENDERING: Mutex<()> = Mutex::new(());
    let _one_at_a_time = lock(&RENDERING);
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let engine = app.state::<Engine>();
    let (display, hidden) = {
        let st = engine.settings();
        (st.quota_display, st.hide_on_screen_share && crate::screen::is_shared(app))
    };
    let items = match engine.shared_overview(*Local::now().offset(), Utc::now()) {
        Ok(o) => choose_items(Some(&o.quotas), display, hidden),
        Err(e) => {
            eprintln!("[buku-token] tray: cannot read overview: {e:#}");
            // Keep the old image, unless it may still show figures on a shared screen.
            if !hidden {
                return;
            }
            choose_items(None, display, true)
        }
    };
    let img = render(&items, is_dark(app));
    // Both at once: `set_icon` alone clears the template flag, leaving black glyphs on a dark menu bar.
    let _ = tray.set_icon_with_as_template(Some(Image::new_owned(img.rgba, img.width, img.height)), img.template);
}

/// A display, in physical pixels: its logical bounds times its own scale.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Screen {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
}

impl Screen {
    fn of(m: &tauri::Monitor) -> Self {
        let (p, s) = (m.position(), m.size());
        Self { x: f64::from(p.x), y: f64::from(p.y), width: f64::from(s.width), height: f64::from(s.height), scale: m.scale_factor() }
    }
}

/// The tray item as (x, y, width, height) in physical pixels of the display it sits on.
fn tray_box(rect: &Rect) -> (f64, f64, f64, f64) {
    let (p, s) = (rect.position.to_physical::<f64>(1.0), rect.size.to_physical::<f64>(1.0));
    (p.x, p.y, s.width, s.height)
}

const POPOVER_MARGIN: f64 = 8.0;

/// Top-left point (logical) that hangs the popover under the tray item, centered and kept on its
/// display. The scale comes from the display under the tray item, never from the hidden window:
/// that window can sit on a display with another scale, which threw it off screen.
fn popover_origin(tray: (f64, f64, f64, f64), screens: &[Screen], popover_width: f64) -> (f64, f64) {
    let (x, y, width, height) = tray;
    let center = x + width / 2.0;
    let screen = screens
        .iter()
        .find(|s| center >= s.x && center < s.x + s.width && y >= s.y && y < s.y + s.height)
        .or(screens.first());
    let Some(s) = screen else { return (center - popover_width / 2.0, y + height) };
    let left = s.x / s.scale + POPOVER_MARGIN;
    let right = (s.x + s.width) / s.scale - popover_width - POPOVER_MARGIN;
    ((center / s.scale - popover_width / 2.0).clamp(left, right.max(left)), (y + height) / s.scale)
}

fn toggle_popover(app: &AppHandle, tray_rect: &Rect) {
    let Some(win) = app.get_webview_window(POPOVER) else { return };
    if win.is_visible().unwrap_or(false) {
        let _ = win.hide();
        return;
    }
    let just_blurred = lock(&app.state::<TrayState>().last_blur_hide).is_some_and(|t| t.elapsed() < BLUR_CLICK_GUARD);
    if just_blurred {
        return;
    }
    if let Ok(size) = win.outer_size() {
        let scale = win.scale_factor().unwrap_or(1.0);
        let screens: Vec<Screen> = win.available_monitors().unwrap_or_default().iter().map(Screen::of).collect();
        let (x, y) = popover_origin(tray_box(tray_rect), &screens, f64::from(size.width) / scale);
        let _ = win.set_position(LogicalPosition::new(x, y));
    }
    let _ = win.show();
    // Focusing sends the engine's "popover opened" trigger (see `commands::init`).
    let _ = win.set_focus();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::QuotaWindow;
    use chrono::{TimeZone, Utc};

    // Laptop at (0, 0) looking like 1680x1050 at 2x, and a 1x monitor above it at (-130, -1080).
    const LAPTOP: Screen = Screen { x: 0.0, y: 0.0, width: 3360.0, height: 2100.0, scale: 2.0 };
    const MONITOR: Screen = Screen { x: -130.0, y: -1080.0, width: 1920.0, height: 1080.0, scale: 1.0 };

    #[test]
    fn popover_hangs_under_a_retina_tray_item() {
        // Item at logical x 1169, y 3, 148x24: the case that landed at x 2297, off screen.
        let got = popover_origin((2338.0, 6.0, 296.0, 48.0), &[LAPTOP, MONITOR], 380.0);
        assert_eq!(got, (1053.0, 27.0));
    }

    #[test]
    fn popover_uses_the_scale_of_the_display_under_the_item() {
        let got = popover_origin((500.0, -1080.0, 148.0, 24.0), &[LAPTOP, MONITOR], 380.0);
        assert_eq!(got, (384.0, -1056.0));
    }

    #[test]
    fn popover_stays_on_its_display() {
        let right = popover_origin((3200.0, 6.0, 100.0, 48.0), &[LAPTOP], 380.0);
        assert_eq!(right.0, 1680.0 - 380.0 - POPOVER_MARGIN);
        let left = popover_origin((0.0, 6.0, 40.0, 48.0), &[LAPTOP], 380.0);
        assert_eq!(left.0, POPOVER_MARGIN);
    }

    #[test]
    fn popover_falls_back_when_no_display_matches() {
        assert_eq!(popover_origin((5000.0, 6.0, 100.0, 48.0), &[LAPTOP], 380.0).0, 1680.0 - 380.0 - POPOVER_MARGIN);
        assert_eq!(popover_origin((500.0, 6.0, 100.0, 48.0), &[], 380.0), (360.0, 54.0));
    }

    #[test]
    fn menu_entries_follow_the_language_and_keep_their_ids() {
        let id = menu_entries(Lang::Id);
        let en = menu_entries(Lang::En);
        assert_eq!(id, [("refresh", "Perbarui"), ("settings", "Pengaturan…"), ("quit", "Keluar Buku Token")]);
        assert_eq!(en, [("refresh", "Refresh"), ("settings", "Settings…"), ("quit", "Quit Buku Token")]);
    }

    fn window(used: f64, expired: bool) -> QuotaWindow {
        let t = Utc.with_ymd_and_hms(2026, 10, 30, 5, 0, 0).unwrap();
        QuotaWindow { id: "five_hour".into(), label: "5h".into(), used_pct: used, resets_at: t, observed_at: t, expired }
    }

    fn quota(agent: Agent, binding: Option<QuotaWindow>, near_limit: bool) -> AgentQuota {
        AgentQuota { agent, plan: None, windows: binding.iter().cloned().collect(), binding, near_limit, unavailable_reason: None, failure: None }
    }

    fn item(agent: Agent, text: &str, alert: bool) -> TrayItem {
        TrayItem { agent: Some(agent), text: text.into(), alert }
    }

    #[test]
    fn hidden_items_keep_the_logos_but_no_figure_or_alert() {
        let shown = tray_items(&[quota(Agent::ClaudeCode, Some(window(95.0, false)), true), quota(Agent::Codex, Some(window(45.0, false)), false)], QuotaDisplay::Left);
        let hidden = hidden_items(&shown);
        assert_eq!(hidden, vec![item(Agent::ClaudeCode, "", false), item(Agent::Codex, "", false)]);
        let (a, b) = (render(&shown, false), render(&hidden, false));
        assert!(!a.template && b.template, "no red alert while hidden");
        assert!(b.width < a.width, "no percentage drawn");
        assert!(b.rgba.chunks(4).any(|p| p[3] > 0), "the logos are still drawn");
    }

    #[test]
    fn an_unreadable_overview_while_shared_draws_logos_without_figures() {
        let q = [quota(Agent::ClaudeCode, Some(window(95.0, false)), true)];
        let neutral = vec![TrayItem { agent: None, text: String::new(), alert: false }];
        assert_eq!(choose_items(None, QuotaDisplay::Left, true), neutral);
        assert_eq!(choose_items(Some(&[]), QuotaDisplay::Left, true), neutral);
        assert_eq!(choose_items(Some(&q), QuotaDisplay::Left, true), vec![item(Agent::ClaudeCode, "", false)]);
        assert_eq!(choose_items(Some(&q), QuotaDisplay::Left, false), vec![item(Agent::ClaudeCode, "5%", true)]);
        assert_eq!(choose_items(None, QuotaDisplay::Left, false), placeholder_items());
    }

    #[test]
    fn the_placeholder_is_neutral_and_still_draws() {
        let items = placeholder_items();
        assert_eq!(items[0].agent, None, "no agent's logo before there is data");
        let img = render(&items, false);
        assert!(img.rgba.chunks(4).any(|p| p[3] > 0));
        assert_ne!(logo_svg(None), logo_svg(Some(Agent::ClaudeCode)));
    }

    #[test]
    fn items_follow_binding_remaining() {
        let items = tray_items(&[
            quota(Agent::ClaudeCode, Some(window(38.0, false)), false),
            quota(Agent::Codex, Some(window(45.0, false)), false),
        ], QuotaDisplay::Left);
        assert_eq!(items, vec![item(Agent::ClaudeCode, "62%", false), item(Agent::Codex, "55%", false)]);
    }

    #[test]
    fn used_display_shows_what_is_used() {
        let qs = [quota(Agent::ClaudeCode, Some(window(38.0, false)), false), quota(Agent::Codex, None, false)];
        assert_eq!(tray_items(&qs, QuotaDisplay::Used), vec![item(Agent::ClaudeCode, "38%", false), item(Agent::Codex, "–", false)]);
        assert_eq!(tray_items(&qs, QuotaDisplay::Left)[0].text, "62%");
        let over = [quota(Agent::ClaudeCode, Some(window(112.0, false)), true)];
        assert_eq!(tray_items(&over, QuotaDisplay::Used)[0].text, "100%");
    }

    #[test]
    fn cursor_is_the_third_item() {
        let items = tray_items(&[
            quota(Agent::Cursor, Some(window(10.0, false)), false),
            quota(Agent::Codex, Some(window(45.0, false)), false),
            quota(Agent::ClaudeCode, Some(window(38.0, false)), false),
        ], QuotaDisplay::Left);
        assert_eq!(
            items,
            vec![item(Agent::ClaudeCode, "62%", false), item(Agent::Codex, "55%", false), item(Agent::Cursor, "90%", false)]
        );
    }

    #[test]
    fn cursor_render_is_not_blank() {
        let img = render(&[item(Agent::Cursor, "", false)], true);
        assert!(opaque_pixels(&img) > 0);
        assert!(opaque_pixels(&img) < opaque_pixels(&render(&[item(Agent::ClaudeCode, "", false), item(Agent::Cursor, "", false)], true)));
    }

    #[test]
    fn remaining_is_clamped_and_rounded() {
        let items = tray_items(&[
            quota(Agent::ClaudeCode, Some(window(112.0, false)), true),
            quota(Agent::Codex, Some(window(37.6, false)), false),
        ], QuotaDisplay::Left);
        assert_eq!(items[0].text, "0%");
        assert_eq!(items[1].text, "62%");
    }

    #[test]
    fn expired_codex_shows_dash() {
        let items = tray_items(&[quota(Agent::Codex, None, false)], QuotaDisplay::Left);
        assert_eq!(items, vec![item(Agent::Codex, "–", false)]);
    }

    #[test]
    fn nan_remaining_shows_dash() {
        let items = tray_items(&[quota(Agent::ClaudeCode, Some(window(f64::NAN, false)), false)], QuotaDisplay::Left);
        assert_eq!(items, vec![item(Agent::ClaudeCode, "–", false)]);
    }

    #[test]
    fn font_is_loaded_once() {
        let (a, b) = (font().expect("a system font on macOS"), font().expect("a system font on macOS"));
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn near_limit_marks_alert() {
        let items = tray_items(&[quota(Agent::ClaudeCode, Some(window(90.0, false)), true)], QuotaDisplay::Left);
        assert_eq!(items, vec![item(Agent::ClaudeCode, "10%", true)]);
    }

    #[test]
    fn no_quota_agents_hidden() {
        let items = tray_items(&[
            quota(Agent::OpenCode, Some(window(10.0, false)), false),
            quota(Agent::Hermes, None, false),
        ], QuotaDisplay::Left);
        assert!(items.is_empty());
    }

    fn opaque_pixels(img: &TrayImage) -> usize {
        img.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count()
    }

    #[test]
    fn render_height_is_36_and_not_blank() {
        let img = render(&[item(Agent::ClaudeCode, "62%", false), item(Agent::Codex, "55%", false)], true);
        assert_eq!(img.height, 36);
        assert!(img.width > 36);
        assert_eq!(img.rgba.len(), (img.width * img.height * 4) as usize);
        assert!(opaque_pixels(&img) > 0);
    }

    #[test]
    fn two_items_are_wider_than_one() {
        let one = render(&[item(Agent::ClaudeCode, "62%", false)], true);
        let two = render(&[item(Agent::ClaudeCode, "62%", false), item(Agent::Codex, "55%", false)], true);
        assert!(two.width > one.width);
    }

    #[test]
    fn text_draws_more_than_the_logo_alone() {
        // With a system font the percentage adds pixels beyond the logo's.
        let logo_only = render(&[item(Agent::ClaudeCode, "", false)], true);
        let with_text = render(&[item(Agent::ClaudeCode, "62%", false)], true);
        assert!(opaque_pixels(&with_text) > opaque_pixels(&logo_only));
    }

    #[test]
    fn alert_disables_template() {
        let calm = render(&[item(Agent::ClaudeCode, "62%", false)], true);
        let alert = render(&[item(Agent::ClaudeCode, "8%", true)], true);
        assert!(calm.template);
        assert!(!alert.template);
    }

    #[test]
    fn alert_logo_is_red_and_template_is_black() {
        let alert = render(&[item(Agent::ClaudeCode, "", true)], true);
        assert!(alert.rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 255 && p[0] == 0xff && p[1] == 0x5f && p[2] == 0x57));
        let calm = render(&[item(Agent::ClaudeCode, "", false)], true);
        assert!(calm.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0));
    }

    #[test]
    fn alert_text_follows_menu_bar_theme() {
        // Only the alerting item exists, so the neutral pixels can only be its text.
        let items = [item(Agent::ClaudeCode, "8%", true)];
        let white = |img: &TrayImage| img.rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 255 && p[..3] == [255, 255, 255]);
        let black = |img: &TrayImage| img.rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 255 && p[..3] == [0, 0, 0]);
        assert!(white(&render(&items, true)));
        assert!(!black(&render(&items, true)));
        assert!(black(&render(&items, false)));
        assert!(!white(&render(&items, false)));
    }

    /// Writes previews (menu-bar-like background, 4x zoom) for eyeballing:
    /// `TRAY_PNG_DIR=/some/dir cargo test tray::tests::write_previews -- --ignored`.
    #[test]
    #[ignore = "manual: writes PNG previews"]
    fn write_previews() {
        let dir = std::env::var("TRAY_PNG_DIR").expect("set TRAY_PNG_DIR");
        let cases = [
            ("normal", vec![item(Agent::ClaudeCode, "62%", false), item(Agent::Codex, "55%", false)]),
            ("three", vec![item(Agent::ClaudeCode, "62%", false), item(Agent::Codex, "55%", false), item(Agent::Cursor, "27%", false)]),
            ("codex-expired", vec![item(Agent::ClaudeCode, "62%", false), item(Agent::Codex, "–", false)]),
            ("claude-alert", vec![item(Agent::ClaudeCode, "8%", true), item(Agent::Codex, "55%", false)]),
        ];
        for (name, items) in cases {
            for (theme, dark) in [("dark", true), ("light", false)] {
                let img = render(&items, dark);
                let (bg, glyph) = if dark { ((0x3a, 0x40, 0x4d), 255u8) } else { ((0xf0, 0xf1, 0xf4), 0u8) };
                let mut canvas = tiny_skia::Pixmap::new(img.width, img.height).unwrap();
                canvas.fill(tiny_skia::Color::from_rgba8(bg.0, bg.1, bg.2, 255));
                let mut glyphs = tiny_skia::Pixmap::new(img.width, img.height).unwrap();
                for (dst, px) in glyphs.pixels_mut().iter_mut().zip(img.rgba.as_chunks::<4>().0) {
                    // A template image is recolored by macOS; mimic that here.
                    let (r, g, b) = if img.template { (glyph, glyph, glyph) } else { (px[0], px[1], px[2]) };
                    *dst = tiny_skia::ColorU8::from_rgba(r, g, b, px[3]).premultiply();
                }
                canvas.draw_pixmap(0, 0, glyphs.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
                let mut big = tiny_skia::Pixmap::new(img.width * 4, img.height * 4).unwrap();
                let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
                big.draw_pixmap(0, 0, canvas.as_ref(), &paint, tiny_skia::Transform::from_scale(4.0, 4.0), None);
                big.save_png(format!("{dir}/tray-{name}-{theme}.png")).unwrap();
            }
        }
    }
}
