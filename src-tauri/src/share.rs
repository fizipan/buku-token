//! Share usage as an image: a card drawn as SVG, rendered to PNG with resvg, everything local.
//! The card never carries project names, paths, session ids or anything from a conversation:
//! `ShareData` has no field that could hold them.

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use resvg::{tiny_skia, usvg};
use serde::Deserialize;

use crate::aggregate::{days_start, enabled_events, total_tokens, Period};
use crate::fmt_id;
use crate::i18n;
use crate::model::{capitalize, Agent, STACK_ORDER};
use crate::settings::{Lang, Settings};
use crate::store::Store;

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareRequest {
    pub period: Period,
    pub agent: Option<Agent>,
    pub hide_cost: bool,
    pub dark: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentShare {
    pub agent: Agent,
    pub cost_usd: f64,
    pub tokens: u64,
    pub unpriced_tokens: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShareData {
    pub agents: Vec<AgentShare>,
    pub cost_usd: f64,
    pub tokens: u64,
    pub unpriced_tokens: u64,
    pub sessions: u64,
    pub top_model: Option<String>,
}

/// Per-agent, per-model and session totals for the request, from enabled agents only. The window
/// is the same one `models` uses (last N local days including today).
pub fn share_data(store: &Store, settings: &Settings, request: &ShareRequest, tz: FixedOffset, now: DateTime<Utc>) -> Result<ShareData> {
    let mut events = enabled_events(store, settings, days_start(request.period.days(), tz, now), now)?;
    if let Some(agent) = request.agent {
        events.retain(|e| e.agent == agent);
    }
    let mut agents: HashMap<Agent, AgentShare> = HashMap::new();
    // Grouped by label; the raw id of the group is kept only to decide what may be shown.
    let mut models: HashMap<String, (f64, u64, &str)> = HashMap::new();
    let mut sessions: HashSet<(Agent, &str)> = HashSet::new();
    let mut data = ShareData { agents: vec![], cost_usd: 0.0, tokens: 0, unpriced_tokens: 0, sessions: 0, top_model: None };
    for e in &events {
        let n = total_tokens(e.agent, &e.tokens);
        let row = agents.entry(e.agent).or_insert(AgentShare { agent: e.agent, cost_usd: 0.0, tokens: 0, unpriced_tokens: 0 });
        let model = models.entry(model_label(&e.model)).or_insert((0.0, 0, &e.model));
        row.tokens += n;
        model.1 += n;
        data.tokens += n;
        match e.cost_usd {
            Some(c) => {
                row.cost_usd += c;
                model.0 += c;
                data.cost_usd += c;
            }
            None => {
                row.unpriced_tokens += n;
                data.unpriced_tokens += n;
            }
        }
        sessions.insert((e.agent, &e.session_id));
    }
    data.sessions = sessions.len() as u64;
    data.agents = agents.into_values().collect();
    data.agents.sort_by(|a, b| {
        b.cost_usd
            .total_cmp(&a.cost_usd)
            .then(b.tokens.cmp(&a.tokens))
            .then_with(|| stack_index(a.agent).cmp(&stack_index(b.agent)))
    });
    // Highest cost; without any cost every row ties on 0 and tokens decide. With the cost hidden the
    // ranking must not depend on it either, or the image would hint at which model is expensive.
    data.top_model = models
        .into_iter()
        .max_by(|(an, a), (bn, b)| {
            let by_cost = if request.hide_cost { std::cmp::Ordering::Equal } else { a.0.total_cmp(&b.0) };
            by_cost.then(a.1.cmp(&b.1)).then_with(|| bn.cmp(an))
        })
        .map(|(_, (_, _, raw))| public_model_label(raw, settings.language));
    Ok(data)
}

fn stack_index(agent: Agent) -> usize {
    STACK_ORDER.iter().position(|a| *a == agent).expect("every agent is in STACK_ORDER")
}

// --- model labels ---------------------------------------------------------------------------

/// "claude-opus-5-5" -> "Opus 5.5", "gpt-5.2-codex" -> "GPT-5.2 Codex", "default" -> "Auto". Same rules as
/// `modelLabel` in `src/lib/agents.ts`: Cursor's effort suffix is dropped, other ids stay as they are.
pub fn model_label(id: &str) -> String {
    if id == "default" || id == "auto" {
        return "Auto".into();
    }
    // "gpt-5.1-codex-max" is a model name, not an effort level, so that one keeps its suffix.
    let model = if id.ends_with("codex-max") {
        id
    } else {
        ["-low", "-medium", "-high", "-max"].iter().find_map(|s| id.strip_suffix(s)).unwrap_or(id)
    };
    if id.strip_prefix("composer-").is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit())) {
        return id.split('-').map(capitalize).collect::<Vec<_>>().join(" ");
    }
    if let Some(label) = gpt_label(model).or_else(|| claude_label(model)) {
        return label;
    }
    id.to_string()
}

/// Families whose ids are public product names. Custom ids (a local model file, a private name, an
/// address) can hold paths, emails or project names, so the card shows only these.
const KNOWN_FAMILIES: [&str; 17] = [
    "claude", "gpt", "o1", "o3", "o4", "composer", "default", "auto", "kimi", "minimax", "glm", "grok", "gemini", "deepseek", "qwen",
    "llama", "mistral",
];

/// Words that can follow a family and a version in a public model id. Anything else could be a
/// project name that someone tacked on.
const COMMON_WORDS: [&str; 17] = [
    "pro", "flash", "mini", "turbo", "fast", "thinking", "max", "sonnet", "opus", "haiku", "latest", "preview", "coder", "codex", "low",
    "medium", "high",
];

/// A token after the family that is safe to show: a number of at most four digits (a version, a year), a
/// common word, or one letter and at most two digits (`k2`, `v3`, `m27`). A longer run of digits could be
/// an id or a phone number; the one exception is the date of a Claude id, handled by the caller.
fn public_token(t: &str) -> bool {
    let mut cs = t.chars();
    let letter_number = cs.next().is_some_and(|c| c.is_ascii_lowercase()) && digits(cs.as_str()) && cs.as_str().len() <= 2;
    (digits(t) && t.len() <= 4) || COMMON_WORDS.contains(&t) || letter_number
}

/// `model_label` for the shared image. It shows an id only when its shape is understood: a Claude, GPT or
/// Composer id `model_label` recognizes (and any extra word is a common one), or `family[-.]number` of a
/// known family followed only by numbers and common words. Everything else is "Model Lain" ("Other Model").
pub fn public_model_label(id: &str, lang: Lang) -> String {
    let other = i18n::share(lang).other_model;
    let lower = id.to_lowercase();
    let plain = lower.starts_with(|c: char| c.is_ascii_alphanumeric())
        && lower.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !plain {
        return other.into();
    }
    if lower == "default" || lower == "auto" {
        return "Auto".into();
    }
    let tokens: Vec<&str> = lower.split(['-', '.']).collect();
    let family = tokens[0].trim_end_matches(|c: char| c.is_ascii_digit());
    let family_ok = KNOWN_FAMILIES.contains(&tokens[0]) || (KNOWN_FAMILIES.contains(&family) && !family.is_empty());
    let numbered = tokens[0].ends_with(|c: char| c.is_ascii_digit()) || tokens.get(1).is_some_and(|t| t.contains(|c: char| c.is_ascii_digit()));
    let label = model_label(id);
    if label != id && matches!(family, "claude" | "gpt" | "composer") && tokens[0] == family {
        // Every token after the family must be public: a number, a common word or a letter and a number.
        // `claude_label` already accepted the shape, so an 8-digit token here is its date.
        let words_ok = tokens[1..].iter().all(|t| public_token(t) || (digits(t) && t.len() == 8));
        return if words_ok { label } else { other.into() };
    }
    if family_ok && numbered && tokens[1..].iter().all(|t| public_token(t)) { label } else { other.into() }
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `^gpt-(\d+(?:\.\d+)?)((?:-[a-z][a-z0-9]*)*)$`, case-insensitive.
fn gpt_label(model: &str) -> Option<String> {
    let rest = model.get(..4).filter(|p| p.eq_ignore_ascii_case("gpt-")).map(|_| &model[4..])?;
    let mut parts = rest.split('-');
    let version = parts.next()?;
    let ok = match version.split_once('.') {
        Some((a, b)) => digits(a) && digits(b),
        None => digits(version),
    };
    let words: Vec<String> = parts.map(str::to_string).collect();
    let word_ok = |w: &String| w.starts_with(|c: char| c.is_ascii_alphabetic()) && w.bytes().all(|b| b.is_ascii_alphanumeric());
    if !ok || !words.iter().all(word_ok) {
        return None;
    }
    Some(std::iter::once(format!("GPT-{version}")).chain(words.iter().map(|w| capitalize(w))).collect::<Vec<_>>().join(" "))
}

/// `claude-(opus|sonnet|haiku)-M[-m][-date]` and `claude-M[-m]-(opus|sonnet|haiku)[-date]`.
fn claude_label(model: &str) -> Option<String> {
    let parts: Vec<&str> = model.split('-').collect();
    let family = |s: &str| matches!(s, "opus" | "sonnet" | "haiku");
    let minor = |s: &str| digits(s) && s.len() <= 2;
    let date = |s: &str| digits(s) && s.len() == 8;
    let tail_ok = |t: &[&str]| t.len() <= 1 && t.iter().all(|s| date(s));
    let (fam, major, mnr) = match parts.as_slice() {
        ["claude", f, major, rest @ ..] if family(f) && digits(major) => {
            let (mnr, tail) = match rest {
                [m, tail @ ..] if minor(m) => (Some(*m), tail),
                _ => (None, rest),
            };
            if !tail_ok(tail) {
                return None;
            }
            (*f, *major, mnr)
        }
        ["claude", major, rest @ ..] if digits(major) => {
            let (mnr, rest) = match rest {
                [m, rest @ ..] if minor(m) => (Some(*m), rest),
                _ => (None, rest),
            };
            match rest {
                [f, tail @ ..] if family(f) && tail_ok(tail) => (*f, *major, mnr),
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(format!("{} {major}{}", capitalize(fam), mnr.map_or_else(String::new, |m| format!(".{m}"))))
}

// --- the card -------------------------------------------------------------------------------

const WIDTH: f64 = 360.0;
const PAD: f64 = 16.0;
const HEAD_H: f64 = 22.0;
const BLOCK_TOP: f64 = PAD + HEAD_H + 14.0;
const DONUT: f64 = 118.0;
const RING_R: f64 = 47.5;
const RING_W: f64 = 14.8;
const ROW_PITCH: f64 = 25.0;
const ROW_GAP: f64 = 8.0;
const STATS_H: f64 = 52.0;
const LEGEND_X: f64 = PAD + DONUT + 18.0;
const SCALE: f32 = 3.0;

struct Palette {
    bg: &'static str,
    border: &'static str,
    fg: &'static str,
    muted_fg: &'static str,
    muted: &'static str,
}

/// Surface colours from `index.css`, the card surface from the approved mockup.
fn palette(dark: bool) -> Palette {
    if dark {
        Palette { bg: "#141417", border: "#27272a", fg: "#fafafa", muted_fg: "#a1a1aa", muted: "#1f1f23" }
    } else {
        Palette { bg: "#ffffff", border: "#e4e4e7", fg: "#09090b", muted_fg: "#5f5f69", muted: "#f4f4f5" }
    }
}

/// Agent colours from `styles/tokens.css`.
fn agent_color(agent: Agent, dark: bool) -> &'static str {
    match (agent, dark) {
        (Agent::ClaudeCode, false) => "#c4572b",
        (Agent::Codex, false) => "#2a6fc9",
        (Agent::OpenCode, false) => "#178a5f",
        (Agent::Hermes, false) => "#8b4fc4",
        (Agent::Cursor, false) => "#52525b",
        (Agent::ClaudeCode, true) => "#d9744a",
        (Agent::Codex, true) => "#4c89d8",
        (Agent::OpenCode, true) => "#2ca474",
        (Agent::Hermes, true) => "#a072da",
        (Agent::Cursor, true) => "#a1a1aa",
    }
}

pub(crate) fn agent_name(agent: Agent) -> &'static str {
    match agent {
        Agent::ClaudeCode => "Claude Code",
        Agent::Codex => "Codex",
        Agent::OpenCode => "OpenCode",
        Agent::Hermes => "Hermes",
        Agent::Cursor => "Cursor",
    }
}

/// XML text: the five special characters escaped, characters XML 1.0 forbids dropped.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(' '),
            c if (c as u32) < 0x20 || matches!(c as u32, 0xFFFE | 0xFFFF) => {}
            c => out.push(c),
        }
    }
    out
}

/// At most `max` characters, cut with an ellipsis. Text is laid out by estimate, so long names must not run on.
fn fit(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>().trim_end())
}

fn n(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn text(x: f64, y: f64, size: f64, weight: u32, fill: &str, anchor: &str, content: &str) -> String {
    format!(
        r#"<text x="{}" y="{}" font-size="{}" font-weight="{weight}" fill="{fill}" text-anchor="{anchor}">{}</text>"#,
        n(x),
        n(y),
        n(size),
        esc(content)
    )
}

/// Money is unknown when tokens were used and none has a price: "$0" would be a lie.
fn priced(cost: f64, unpriced: u64) -> bool {
    !(cost == 0.0 && unpriced > 0)
}

fn percent(part: u64, total: u64) -> String {
    if part == 0 || total == 0 {
        return "0%".into();
    }
    let p = part as f64 / total as f64 * 100.0;
    if p < 1.0 { "<1%".into() } else { format!("{}%", p.round()) }
}

/// The card as SVG, 360 wide. Pure: the same data and request give the same text. Fonts are chosen by
/// `render_png`; here they are the generic sans-serif.
pub fn share_svg(data: &ShareData, request: &ShareRequest, lang: Lang) -> String {
    let tx = i18n::share(lang);
    let p = palette(request.dark);
    let hide = request.hide_cost;
    let title = request.agent.map_or(tx.title, agent_name);

    let mut rows: Vec<&AgentShare> = data.agents.iter().collect();
    if hide {
        // Shares are of tokens, so the list reads in the same order as its percentages.
        rows.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| stack_index(a.agent).cmp(&stack_index(b.agent))));
    }
    let count = rows.len() as f64;
    let legend_h = if count > 0.0 { count * ROW_PITCH - ROW_GAP } else { 0.0 };
    let block_h = DONUT.max(legend_h);
    let stats_top = BLOCK_TOP + block_h + 14.0;
    let height = stats_top + STATS_H + 13.0 + 15.0 + PAD;
    let (cx, cy) = (PAD + DONUT / 2.0, BLOCK_TOP + block_h / 2.0);

    let mut s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}" font-family="sans-serif">"#,
        n(WIDTH),
        n(height),
        n(WIDTH),
        n(height)
    );
    s.push_str(&format!(
        r#"<rect x="0.5" y="0.5" width="{}" height="{}" rx="14" fill="{}" stroke="{}"/>"#,
        n(WIDTH - 1.0),
        n(height - 1.0),
        p.bg,
        p.border
    ));

    // Header: title left, period pill right.
    s.push_str(&text(PAD, PAD + 16.0, 15.0, 600, p.fg, "start", title));
    let label = i18n::share_period(request.period, lang);
    let pill_w = label.chars().count() as f64 * 6.6 + 18.0;
    s.push_str(&format!(
        r#"<rect x="{}" y="{}" width="{}" height="20" rx="10" fill="{}"/>"#,
        n(WIDTH - PAD - pill_w),
        n(PAD + 1.0),
        n(pill_w),
        p.muted
    ));
    s.push_str(&text(WIDTH - PAD - pill_w / 2.0, PAD + 15.0, 12.0, 500, p.fg, "middle", label));

    // Donut: cost per agent, or tokens per agent when the cost is hidden.
    let values: Vec<(Agent, f64)> = rows
        .iter()
        .map(|r| (r.agent, if hide { r.tokens as f64 } else { r.cost_usd }))
        .filter(|(_, v)| *v > 0.0)
        .collect();
    let sum: f64 = values.iter().map(|(_, v)| v).sum();
    let ring = |color: &str, dash: &str, offset: f64| {
        format!(
            r#"<circle cx="{}" cy="{}" r="{}" fill="none" stroke="{color}" stroke-width="{}" stroke-dasharray="{dash}" stroke-dashoffset="{}" transform="rotate(-90 {} {})"/>"#,
            n(cx),
            n(cy),
            n(RING_R),
            n(RING_W),
            n(offset),
            n(cx),
            n(cy)
        )
    };
    if sum > 0.0 {
        let circumference = 2.0 * std::f64::consts::PI * RING_R;
        let gap = if values.len() > 1 { 2.0 } else { 0.0 };
        let mut offset = 0.0;
        for (agent, v) in &values {
            let len = v / sum * circumference;
            s.push_str(&ring(agent_color(*agent, request.dark), &format!("{} {}", n((len - gap).max(0.5)), n(circumference)), -offset));
            offset += len;
        }
    } else {
        s.push_str(&ring(p.muted, &format!("{0} {0}", n(2.0 * std::f64::consts::PI * RING_R)), 0.0));
    }

    if rows.is_empty() {
        s.push_str(&text(LEGEND_X, cy + 4.7, 13.5, 500, p.muted_fg, "start", tx.no_usage));
    } else {
        let (centre, label) = if hide {
            (fmt_id::tokens(data.tokens, lang), tx.tokens)
        } else if priced(data.cost_usd, data.unpriced_tokens) {
            (if data.cost_usd < 1000.0 { fmt_id::usd(data.cost_usd, lang) } else { fmt_id::usd0(data.cost_usd, lang) }, tx.api_equivalent)
        } else {
            ("–".to_string(), tx.api_equivalent)
        };
        let size = 15.7_f64.min(66.0 / (centre.chars().count().max(1) as f64 * 0.58));
        s.push_str(&text(cx, cy, size, 600, p.fg, "middle", &centre));
        s.push_str(&text(cx, cy + 13.9, 10.2, 400, p.muted_fg, "middle", label));
    }

    // Legend.
    let first = BLOCK_TOP + (block_h - legend_h) / 2.0;
    for (i, r) in rows.iter().enumerate() {
        let mid = first + i as f64 * ROW_PITCH + 8.5;
        let value = if hide {
            percent(r.tokens, data.tokens)
        } else if priced(r.cost_usd, r.unpriced_tokens) {
            fmt_id::usd(r.cost_usd, lang)
        } else {
            "–".to_string()
        };
        s.push_str(&format!(
            r#"<circle cx="{}" cy="{}" r="4.5" fill="{}"/>"#,
            n(LEGEND_X + 4.5),
            n(mid),
            agent_color(r.agent, request.dark)
        ));
        s.push_str(&text(LEGEND_X + 18.0, mid + 4.7, 13.5, 400, p.fg, "start", agent_name(r.agent)));
        s.push_str(&text(WIDTH - PAD, mid + 4.7, 13.5, 500, p.muted_fg, "end", &value));
    }

    // Three stats in one muted box.
    s.push_str(&format!(
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="10" fill="{}"/>"#,
        n(PAD),
        n(stats_top),
        n(WIDTH - 2.0 * PAD),
        n(STATS_H),
        p.muted
    ));
    let col = (WIDTH - 2.0 * PAD) / 3.0;
    let top_model = data.top_model.as_deref().map_or("–".to_string(), |m| fit(m, 16));
    let stats = [(tx.total_tokens, fmt_id::tokens(data.tokens, lang)), (tx.sessions, fmt_id::int(data.sessions, lang)), (tx.top_model, top_model)];
    for (i, (label, value)) in stats.iter().enumerate() {
        let left = PAD + i as f64 * col;
        if i > 0 {
            s.push_str(&format!(
                r#"<line x1="{0}" y1="{1}" x2="{0}" y2="{2}" stroke="{3}"/>"#,
                n(left),
                n(stats_top),
                n(stats_top + STATS_H),
                p.border
            ));
        }
        s.push_str(&text(left + 11.0, stats_top + 20.0, 11.5, 400, p.muted_fg, "start", label));
        // Shrink long values to the column (87 px of text) instead of cutting them.
        let size = 14.0_f64.min(((col - 22.0) / (value.chars().count().max(1) as f64 * 0.58)).max(9.0));
        s.push_str(&text(left + 11.0, stats_top + 38.0, size, 600, p.fg, "start", value));
    }

    s.push_str(&text(WIDTH / 2.0, stats_top + STATS_H + 13.0 + 11.5, 12.0, 400, p.muted_fg, "middle", tx.made_with));
    s.push_str("</svg>");
    s
}

// --- PNG and file ---------------------------------------------------------------------------

/// The tray's system font database with its family set as the generic sans-serif, so the card's
/// `font-family="sans-serif"` resolves. `None` (no font file): the card renders without text.
fn fonts() -> Option<&'static Arc<usvg::fontdb::Database>> {
    static FONTS: OnceLock<Option<Arc<usvg::fontdb::Database>>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let (db, family) = crate::tray::font()?;
            let mut db = (**db).clone();
            db.set_sans_serif_family(family.clone());
            Some(Arc::new(db))
        })
        .as_ref()
}

/// The card as a PNG at 3x (1080 px wide).
pub fn render_png(svg: &str) -> Result<Vec<u8>, String> {
    let mut opt = usvg::Options::default();
    if let Some(db) = fonts() {
        opt.fontdb = db.clone();
    }
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| format!("invalid svg: {e}"))?;
    let size = tree.size();
    let mut pixmap = tiny_skia::Pixmap::new((size.width() * SCALE).ceil() as u32, (size.height() * SCALE).ceil() as u32)
        .ok_or("empty image")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(SCALE, SCALE), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| format!("png: {e}"))
}

/// Writes `bytes`; when that fails the half-written file is deleted, so no broken PNG stays in Downloads.
fn write_or_remove(mut file: impl Write, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    file.write_all(bytes).and_then(|()| file.flush()).inspect_err(|_| {
        let _ = std::fs::remove_file(path);
    })
}

/// Writes `buku-token-YYYY-MM-DD.png` into `dir`, or `-2`, `-3`... when that name is taken; an existing
/// file is never touched. Returns the file name.
pub fn save_png(dir: &Path, day: NaiveDate, png: &[u8]) -> std::io::Result<String> {
    for attempt in 1..=1000 {
        let name = if attempt == 1 { format!("buku-token-{day}.png") } else { format!("buku-token-{day}-{attempt}.png") };
        match OpenOptions::new().write(true).create_new(true).open(dir.join(&name)) {
            Ok(file) => {
                write_or_remove(file, &dir.join(&name), png)?;
                return Ok(name);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(ErrorKind::AlreadyExists.into())
}

#[cfg(test)]
mod tests {
    /// `SHARE_PNG_PATH=/some/file.png cargo test --lib share::tests::write_example -- --ignored`
    #[test]
    #[ignore = "manual: writes the README example card"]
    fn write_example() {
        let path = std::env::var("SHARE_PNG_PATH").expect("set SHARE_PNG_PATH");
        let agent = |agent, cost_usd, tokens| AgentShare { agent, cost_usd, tokens, unpriced_tokens: 0 };
        let data = ShareData {
            agents: vec![
                agent(Agent::ClaudeCode, 411.4, 49_000_000),
                agent(Agent::Cursor, 105.0, 11_000_000),
                agent(Agent::Codex, 68.6, 7_000_000),
                agent(Agent::OpenCode, 8.6, 3_000_000),
                agent(Agent::Hermes, 6.4, 1_000_000),
            ],
            cost_usd: 600.0,
            tokens: 71_000_000,
            unpriced_tokens: 0,
            sessions: 527,
            top_model: Some("Opus 5.5".into()),
        };
        let request = ShareRequest { period: Period::Last30, agent: None, hide_cost: false, dark: true };
        std::fs::write(path, render_png(&share_svg(&data, &request, Lang::En)).unwrap()).unwrap();
    }

    use super::*;
    use crate::aggregate::testkit::*;
    use crate::model::Tokens;
    use crate::settings::Settings;

    const SECRET_DIR: &str = "/Users/zz/ProjectRahasia";

    fn req(period: Period, agent: Option<Agent>, hide_cost: bool, dark: bool) -> ShareRequest {
        ShareRequest { period, agent, hide_cost, dark }
    }

    fn model_ev(ts: chrono::DateTime<chrono::Utc>, agent: Agent, model: &str, session: &str, t: Tokens, cost: Option<f64>) -> crate::model::UsageEvent {
        ev(ts, agent, model, SECRET_DIR, session, t, cost)
    }

    /// Claude $15 (2 sessions, Opus 5.5 $12 and Sonnet 4 $3), Codex $3 (session "s1" again, a new session for Codex),
    /// OpenCode 550 tokens without a price.
    fn sample() -> crate::store::Store {
        let t = at(2026, 10, 30, 9, 0);
        store(vec![
            model_ev(t, Agent::ClaudeCode, "claude-opus-5-5", "s1", tok(1000, 100), Some(8.0)),
            model_ev(t, Agent::ClaudeCode, "claude-opus-5-5", "s1", tok(1000, 100), Some(4.0)),
            model_ev(t, Agent::ClaudeCode, "claude-sonnet-4-20250514", "s2", tok(500, 50), Some(3.0)),
            model_ev(t, Agent::Codex, "gpt-5.5", "s1", tok(2000, 10), Some(3.0)),
            model_ev(t, Agent::OpenCode, "big-pickle", "s3", tok(500, 50), None),
        ])
    }

    fn data_for(s: &crate::store::Store, r: &ShareRequest) -> ShareData {
        share_data(s, &Settings::default(), r, tz(), now()).unwrap()
    }

    fn sample_data() -> ShareData {
        data_for(&sample(), &req(Period::Last30, None, false, true))
    }

    #[test]
    fn data_sums_cost_tokens_sessions_and_sorts_agents_by_cost() {
        let d = sample_data();
        let rows: Vec<_> = d.agents.iter().map(|a| (a.agent, a.cost_usd, a.tokens, a.unpriced_tokens)).collect();
        assert_eq!(
            rows,
            vec![(Agent::ClaudeCode, 15.0, 2750, 0), (Agent::Codex, 3.0, 2010, 0), (Agent::OpenCode, 0.0, 550, 550)]
        );
        close(d.cost_usd, 18.0);
        assert_eq!((d.tokens, d.unpriced_tokens), (5310, 550));
        // Unique (agent, session): cc s1, cc s2, cx s1, oc s3. The two cc events of s1 count once.
        assert_eq!(d.sessions, 4);
        assert_eq!(d.top_model.as_deref(), Some("Opus 5.5"));
    }

    #[test]
    fn top_model_falls_back_to_tokens_without_any_cost() {
        let t = at(2026, 10, 30, 9, 0);
        let s = store(vec![
            model_ev(t, Agent::OpenCode, "claude-haiku-3", "a", tok(10, 1), None),
            model_ev(t, Agent::OpenCode, "gpt-5.5", "b", tok(900, 90), None),
        ]);
        let d = data_for(&s, &req(Period::Today, None, false, false));
        assert_eq!(d.top_model.as_deref(), Some("GPT-5.5"));
        assert_eq!(d.cost_usd, 0.0);
    }

    #[test]
    fn disabled_agents_one_agent_filter_and_period_window() {
        let t = at(2026, 10, 30, 9, 0);
        let old = at(2026, 10, 20, 9, 0);
        let s = store(vec![
            model_ev(t, Agent::ClaudeCode, "m", "a", tok(100, 10), Some(1.0)),
            model_ev(t, Agent::Codex, "m", "b", tok(100, 10), Some(2.0)),
            model_ev(old, Agent::Codex, "m", "c", tok(100, 10), Some(4.0)),
        ]);
        let agents = |d: &ShareData| d.agents.iter().map(|a| a.agent).collect::<Vec<_>>();

        let mut off = Settings::default();
        off.enabled_agents.insert(Agent::Codex, false);
        let d = share_data(&s, &off, &req(Period::Last30, None, false, false), tz(), now()).unwrap();
        assert_eq!(agents(&d), vec![Agent::ClaudeCode]);
        assert_eq!(d.sessions, 1);

        // A single agent that is switched off gives an empty share, not its numbers.
        let d = share_data(&s, &off, &req(Period::Last30, Some(Agent::Codex), false, false), tz(), now()).unwrap();
        assert!(d.agents.is_empty());

        let d = data_for(&s, &req(Period::Last30, Some(Agent::Codex), false, false));
        assert_eq!((agents(&d), d.cost_usd), (vec![Agent::Codex], 6.0));
        // 20 Okt is outside the last 7 days, inside the last 30.
        let d = data_for(&s, &req(Period::Last7, Some(Agent::Codex), false, false));
        assert_eq!(d.cost_usd, 2.0);
        let d = data_for(&s, &req(Period::Today, None, false, false));
        assert_eq!(d.cost_usd, 3.0);
    }

    #[test]
    fn share_data_carries_no_path_or_project() {
        let dump = format!("{:?}", sample_data());
        for leak in ["ProjectRahasia", "/Users", "zz"] {
            assert!(!dump.contains(leak), "{leak} in {dump}");
        }
    }

    #[test]
    fn model_label_matches_the_frontend() {
        // Same cases as `modelLabel` in src/lib/agents.test.ts.
        for (id, want) in [
            ("claude-opus-5-5", "Opus 5.5"),
            ("claude-sonnet-4-20250514", "Sonnet 4"),
            ("claude-3-5-haiku-20241022", "Haiku 3.5"),
            ("gpt-5.2-codex", "GPT-5.2 Codex"),
            ("gpt-5.5", "GPT-5.5"),
            ("gpt-5.1-codex-max", "GPT-5.1 Codex Max"),
            ("kimi-k2", "kimi-k2"),
            ("default", "Auto"),
            ("auto", "Auto"),
            ("composer-2.5", "Composer 2.5"),
            ("composer-2.5-fast", "Composer 2.5 Fast"),
            ("claude-opus-5-5-medium", "Opus 5.5"),
            ("claude-sonnet-4-20250514-high", "Sonnet 4"),
            ("gpt-5.5-high", "GPT-5.5"),
            ("kimi-k2-low", "kimi-k2-low"),
            ("gpt-5.1-codex-max-high", "GPT-5.1 Codex Max"),
            ("big-pickle", "big-pickle"),
        ] {
            assert_eq!(model_label(id), want, "{id}");
        }
    }

    fn svg(r: ShareRequest) -> String {
        share_svg(&data_for(&sample(), &r), &r, Lang::Id)
    }

    #[test]
    fn cost_mode_shows_the_card_from_the_mockup() {
        let r = req(Period::Last30, None, false, true);
        let s = svg(r);
        assert!(s.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg" width="360" height=""#));
        for want in [
            "Pemakaian AI-ku",
            "30 Hari Terakhir",
            "Setara API",
            "$18",
            "Claude Code",
            "$15,00",
            "$3,00",
            "Total Token",
            "5 rb",
            "Sesi",
            "Model Teratas",
            "Opus 5.5",
            "Dibuat dengan Buku Token",
        ] {
            assert!(s.contains(want), "missing {want}");
        }
        // Agents in cost order, OpenCode (no price) last with a dash.
        let (cc, cx, oc) = (s.find("Claude Code").unwrap(), s.find("Codex").unwrap(), s.find("OpenCode").unwrap());
        assert!(cc < cx && cx < oc);
        assert!(!s.contains("Rp"));
    }

    #[test]
    fn english_card_has_no_indonesian_and_english_numbers() {
        let r = req(Period::Last30, None, false, true);
        let s = share_svg(&data_for(&sample(), &r), &r, Lang::En);
        for want in ["My AI Usage", "Last 30 Days", "API Equivalent", "$18", "$15.00", "$3.00", "Total Tokens", "5K", "Sessions", "Top Model", "Made With Buku Token"] {
            assert!(s.contains(want), "missing {want}");
        }
        for banned in ["Pemakaian", "Terakhir", "Setara", "Sesi", "Teratas", "Dibuat", "$15,00", "5 rb"] {
            assert!(!s.contains(banned), "{banned} left in the English card");
        }
        let hidden = req(Period::Today, None, true, false);
        let h = share_svg(&data_for(&sample(), &hidden), &hidden, Lang::En);
        assert!(h.contains(">Tokens<") && h.contains(">Today<"));
        let empty = share_svg(&data_for(&store(vec![]), &r), &r, Lang::En);
        assert!(empty.contains("No Usage Yet"));
    }

    #[test]
    fn the_top_model_of_a_private_id_is_other_model_in_english() {
        let t = at(2026, 10, 30, 9, 0);
        let secret = store(vec![model_ev(t, Agent::ClaudeCode, "ProjectRahasia-v2", "s1", tok(1000, 0), Some(5.0))]);
        let r = req(Period::Last30, None, false, false);
        let en = Settings { language: Lang::En, ..Settings::default() };
        assert_eq!(share_data(&secret, &en, &r, tz(), now()).unwrap().top_model.as_deref(), Some("Other Model"));
        assert_eq!(data_for(&secret, &r).top_model.as_deref(), Some("Model Lain"));
        assert_eq!(public_model_label("gpt-projectrahasia", Lang::En), "Other Model");
        let card = share_svg(&share_data(&secret, &en, &r, tz(), now()).unwrap(), &r, Lang::En);
        assert!(card.contains(">Other Model<") && !card.contains("Rahasia"));
    }

    #[test]
    fn period_labels() {
        for (p, label) in [(Period::Today, "Hari Ini"), (Period::Last7, "7 Hari Terakhir"), (Period::Last30, "30 Hari Terakhir")] {
            assert!(svg(req(p, None, false, false)).contains(&format!(">{label}<")), "{label}");
        }
    }

    #[test]
    fn hide_cost_has_no_money_at_all() {
        let t = at(2026, 10, 30, 9, 0);
        let variants = [
            sample(),
            store(vec![model_ev(t, Agent::OpenCode, "big-pickle", "x", tok(500, 50), None)]),
            store(vec![model_ev(t, Agent::Codex, "gpt-5.5", "x", tok(500, 50), Some(1234.5))]),
            store(vec![]),
        ];
        for s in &variants {
            for agent in [None, Some(Agent::Codex)] {
                for dark in [false, true] {
                    for lang in [Lang::Id, Lang::En] {
                        let r = req(Period::Last30, agent, true, dark);
                        let out = share_svg(&data_for(s, &r), &r, lang);
                        for money in ["$", "Rp", "Setara API", "API Equivalent", "USD", "1.234", "1,234"] {
                            assert!(!out.contains(money), "{money} in hide_cost svg ({lang:?})");
                        }
                    }
                }
            }
        }
        let out = svg(req(Period::Last30, None, true, true));
        // Donut centre: total tokens with the label "Token"; legend: shares of tokens.
        assert!(out.contains(">5 rb<") && out.contains(">Token<"));
        assert!(out.contains(">52%<") && out.contains(">38%<") && out.contains(">10%<"), "{out}");
    }

    #[test]
    fn public_model_label_only_lets_known_model_families_through() {
        assert_eq!(public_model_label("claude-opus-5-5", Lang::Id), "Opus 5.5");
        assert_eq!(public_model_label("claude-opus-4-20250514", Lang::Id), "Opus 4");
        assert_eq!(public_model_label("GPT-5.2-Codex", Lang::Id), "GPT-5.2 Codex");
        assert_eq!(public_model_label("composer-2.5-fast", Lang::Id), "Composer 2.5 Fast");
        assert_eq!(public_model_label("default", Lang::Id), "Auto");
        assert_eq!(public_model_label("gpt-5.1-codex-max", Lang::Id), "GPT-5.1 Codex Max");
        for ok in ["o3-mini", "kimi-k2", "glm-4.6", "grok-4", "gemini-2.5-pro", "deepseek-v3", "qwen3-coder", "llama3.1", "minimax-m2", "minimax-m27"] {
            assert_eq!(public_model_label(ok, Lang::Id), ok, "{ok}");
        }
        for private in [
            "/Users/zz/Secret/m.gguf",
            "me@acme.com",
            "claude opus with spaces",
            "ProjectRahasia-v2",
            "secret",
            "autopilot-internal",
            "gpt-projectrahasia",
            "claude3-acme-internal",
            "gpt-5-projectrahasia",
            "kimi-k2-acme",
            "kimi-a8675309",
            "kimi-p123456",
            "gpt-5-p123456",
            "kimi-k2-a8675309",
            "kimi-k2-8675309",
            "kimi-8675309",
            "gpt-5-12345",
            "o3-mini-8675309",
            "deepseek-coder",
            "composer-2-5projectx",
            "composer-2.5secret",
            "claude-opus-5-5x",
            "gpt-5.2x",
            "claude-opus-5-5/../../etc",
            "claude\u{1}x",
            "",
        ] {
            assert_eq!(public_model_label(private, Lang::Id), "Model Lain", "{private:?}");
        }
    }

    #[test]
    fn hide_cost_ranks_the_top_model_by_tokens_only() {
        let t = at(2026, 10, 30, 9, 0);
        let s = store(vec![
            model_ev(t, Agent::ClaudeCode, "claude-opus-5-5", "a", tok(10, 1), Some(100.0)),
            model_ev(t, Agent::Codex, "gpt-5.5", "b", tok(5000, 10), Some(1.0)),
        ]);
        let shown = data_for(&s, &req(Period::Last30, None, false, false));
        let hidden = data_for(&s, &req(Period::Last30, None, true, false));
        assert_eq!(shown.top_model.as_deref(), Some("Opus 5.5"));
        assert_eq!(hidden.top_model.as_deref(), Some("GPT-5.5"));
    }

    #[test]
    fn the_hole_shows_cents_below_a_thousand_dollars() {
        let t = at(2026, 10, 30, 9, 0);
        let one = |cost: f64| {
            let s = store(vec![model_ev(t, Agent::Codex, "gpt-5.5", "x", tok(100, 10), Some(cost))]);
            let r = req(Period::Last30, None, false, false);
            share_svg(&data_for(&s, &r), &r, Lang::Id)
        };
        assert!(one(0.4).contains(">$0,40<"));
        assert!(one(999.99).contains(">$999,99<"));
        assert!(one(2741.4).contains(">$2.741<"));
    }

    #[test]
    fn a_long_known_model_label_shrinks_instead_of_being_cut() {
        let t = at(2026, 10, 30, 9, 0);
        let s = store(vec![model_ev(t, Agent::Codex, "gpt-5.2-codex", "x", tok(100, 10), Some(1.0))]);
        let r = req(Period::Last30, None, false, false);
        let out = share_svg(&data_for(&s, &r), &r, Lang::Id);
        assert!(out.contains(">GPT-5.2 Codex<") && !out.contains('…'));
        let tag = out.split(">GPT-5.2 Codex<").next().unwrap().rsplit("<text").next().unwrap();
        let size: f64 = tag.split("font-size=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
        // Column text width is 328/3 - 22 = 87; 13 characters at ~0.58 em.
        assert!(size < 14.0 && 13.0 * 0.58 * size <= 87.5, "{size}");
    }

    #[test]
    fn a_failed_write_leaves_no_partial_file() {
        struct Failing;
        impl Write for Failing {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(ErrorKind::StorageFull.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("half.png");
        std::fs::write(&path, b"").unwrap();
        assert!(write_or_remove(Failing, &path, b"png").is_err());
        assert!(!path.exists());
    }

    #[test]
    fn no_project_name_path_or_session_in_the_image() {
        let t = at(2026, 10, 30, 9, 0);
        let secrets = store(
            ["/Users/zz/Secret/m.gguf", "me@acme.com", "ProjectRahasia-v2"]
                .iter()
                .enumerate()
                .map(|(i, m)| model_ev(t, Agent::ClaudeCode, m, "s1", tok(1000 * (i as u64 + 1), 0), Some(50.0 + i as f64)))
                .collect(),
        );
        for hide in [false, true] {
            let r = req(Period::Last30, None, hide, false);
            let out = share_svg(&data_for(&secrets, &r), &r, Lang::Id);
            for leak in ["Secret", "m.gguf", "acme", "me@", "ProjectRahasia", "/Users"] {
                assert!(!out.contains(leak), "{leak}");
            }
            assert!(out.contains(">Model Lain<"));
        }

        for hide in [false, true] {
            let out = svg(req(Period::Last30, None, hide, false));
            for leak in ["ProjectRahasia", "/Users", "Documents", "cookie", "@", ">s1<", ">s2<", ">s3<", "session"] {
                assert!(!out.contains(leak), "{leak}");
            }
        }
    }

    #[test]
    fn single_agent_uses_its_name_as_title() {
        let out = svg(req(Period::Last30, Some(Agent::Codex), false, false));
        assert!(!out.contains("Pemakaian AI-ku"));
        assert_eq!(out.matches(">Codex<").count(), 2, "title and legend row");
        assert!(!out.contains("Claude Code"));
        assert!(out.contains("$3,00"));
    }

    #[test]
    fn unpriced_usage_shows_a_dash_never_zero_dollars() {
        let t = at(2026, 10, 30, 9, 0);
        let s = store(vec![model_ev(t, Agent::OpenCode, "big-pickle", "x", tok(500, 50), None)]);
        let r = req(Period::Last30, None, false, false);
        let out = share_svg(&data_for(&s, &r), &r, Lang::Id);
        assert!(!out.contains("$0") && !out.contains('$'), "{out}");
        assert!(out.contains(">–<"));
        assert!(out.contains("Setara API") && out.contains("OpenCode") && out.contains(">550<"));
    }

    #[test]
    fn no_usage_shows_an_empty_ring_and_a_message() {
        for hide in [false, true] {
            let r = req(Period::Today, None, hide, false);
            let out = share_svg(&data_for(&store(vec![]), &r), &r, Lang::Id);
            assert!(out.contains("Belum Ada Pemakaian"));
            assert!(!out.contains("Claude Code"));
            assert_eq!(out.matches("<circle").count(), 1, "one empty ring");
            assert!(out.contains("#f4f4f5"), "empty ring in the muted colour");
        }
    }

    #[test]
    fn colours_follow_the_theme() {
        let (light, dark) = (svg(req(Period::Last30, None, false, false)), svg(req(Period::Last30, None, false, true)));
        assert!(light.contains("#c4572b") && light.contains("#ffffff") && !light.contains("#d9744a"));
        assert!(dark.contains("#d9744a") && dark.contains("#141417") && !dark.contains("#c4572b"));
    }

    #[test]
    fn text_is_escaped_for_xml() {
        assert_eq!(esc("<b>&\"'x\u{1}"), "&lt;b&gt;&amp;&quot;&apos;x");
        let out = text(0.0, 0.0, 12.0, 400, "#000", "start", "<b>&");
        assert!(out.contains("&lt;b&gt;&amp;") && !out.contains("<b>"));
        // A hostile model id never gets as far as the text, and the card stays valid XML.
        let t = at(2026, 10, 30, 9, 0);
        let s = store(vec![model_ev(t, Agent::OpenCode, "<b>&\"'x", "x", tok(500, 50), Some(1.0))]);
        let r = req(Period::Last30, None, false, true);
        let card = share_svg(&data_for(&s, &r), &r, Lang::Id);
        assert!(card.contains(">Model Lain<") && !card.contains("<b>"));
        assert!(render_png(&card).is_ok());
    }

    #[test]
    fn png_is_three_times_the_card_and_has_text() {
        let r = req(Period::Last30, None, false, true);
        let out = share_svg(&data_for(&sample(), &r), &r, Lang::Id);
        let png = render_png(&out).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let px = resvg::tiny_skia::Pixmap::decode_png(&png).unwrap();
        let height: u32 = out.split("height=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
        assert_eq!((px.width(), px.height()), (1080, height * 3));
        // Near-white pixels on the dark card can only be text: the system font loaded.
        let text = px.pixels().iter().filter(|p| p.red() > 200 && p.green() > 200 && p.blue() > 200).count();
        assert!(text > 500, "only {text} text pixels");
        assert!(png.len() > 5_000);
    }

    #[test]
    fn save_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let day = date(2026, 10, 31);
        std::fs::write(dir.path().join("buku-token-2026-10-31.png"), b"mine").unwrap();
        assert_eq!(save_png(dir.path(), day, b"one").unwrap(), "buku-token-2026-10-31-2.png");
        assert_eq!(save_png(dir.path(), day, b"two").unwrap(), "buku-token-2026-10-31-3.png");
        assert_eq!(save_png(dir.path(), date(2026, 11, 1), b"x").unwrap(), "buku-token-2026-11-01.png");
        assert_eq!(std::fs::read(dir.path().join("buku-token-2026-10-31.png")).unwrap(), b"mine");
        assert_eq!(std::fs::read(dir.path().join("buku-token-2026-10-31-3.png")).unwrap(), b"two");
    }

    #[test]
    fn request_parses_from_the_ui() {
        let r: ShareRequest = serde_json::from_str(r#"{"period":"30d","agent":null,"hideCost":true,"dark":false}"#).unwrap();
        assert_eq!((r.period, r.agent, r.hide_cost, r.dark), (Period::Last30, None, true, false));
        let r: ShareRequest = serde_json::from_str(r#"{"period":"today","agent":"cr","hideCost":false,"dark":true}"#).unwrap();
        assert_eq!((r.agent, r.dark), (Some(Agent::Cursor), true));
    }
}
