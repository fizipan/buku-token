//! "Hide while the screen is shared": asks the window server whether something is watching the screen.
//!
//! `SLSIsScreenWatcherPresent` / `CGSIsScreenWatcherPresent` are private functions, so they are looked up
//! at run time with `dlsym` and never linked: a macOS that dropped them degrades to the public session
//! dictionary (`CGSessionCopyCurrentDictionary`, whose `CGSSessionScreenIsShared` key is undocumented) and never crashes.
//! `decide` and `Gate` are pure; only `probe` touches the window server.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// A C boolean as the private functions return it: anything but 0 counts as true, so an odd value is
/// never read as a Rust `bool` (which would be undefined behavior).
fn flag(v: u8) -> bool {
    v != 0
}

/// How often the window server is asked while the setting is on.
pub const POLL_EVERY: Duration = Duration::from_secs(3);

/// Screen is shared when any source that answered says so. `None` means the source is unavailable.
pub fn decide(watcher: Option<bool>, session_dictionary: Option<bool>) -> bool {
    watcher.unwrap_or(false) || session_dictionary.unwrap_or(false)
}

/// Whether a poll result differs from the last one, remembering it. Only a change redraws the tray.
pub fn changed(last: &AtomicBool, now: bool) -> bool {
    last.swap(now, Ordering::SeqCst) != now
}

/// Owns "is a poller running": starts one only while the setting is on, and stops it when it goes off.
#[derive(Default)]
pub struct Gate {
    stop: Mutex<Option<Arc<AtomicBool>>>,
}

impl Gate {
    /// Setting on: calls `spawn` with the poller's stop flag unless one already runs.
    /// Setting off: raises the stop flag. Returns true when a poller was started.
    pub fn sync(&self, on: bool, spawn: impl FnOnce(Arc<AtomicBool>)) -> bool {
        let mut slot = self.stop.lock().unwrap_or_else(|e| e.into_inner());
        match (on, slot.take()) {
            (true, Some(flag)) => {
                *slot = Some(flag);
                false
            }
            (true, None) => {
                let flag = Arc::new(AtomicBool::new(false));
                *slot = Some(flag.clone());
                spawn(flag);
                true
            }
            (false, Some(flag)) => {
                flag.store(true, Ordering::SeqCst);
                false
            }
            (false, None) => false,
        }
    }

    pub fn running(&self) -> bool {
        self.stop.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }
}

/// Asks the window server now. Everything unavailable counts as "not shared".
pub fn probe() -> bool {
    decide(private_watcher(), session_dictionary())
}

#[cfg(target_os = "macos")]
fn private_watcher() -> Option<bool> {
    use std::ffi::CStr;
    for name in [c"SLSIsScreenWatcherPresent", c"CGSIsScreenWatcherPresent"] {
        let name: &CStr = name;
        // SAFETY: dlsym only reads a symbol table. The function takes no arguments and returns a C bool, read as a byte.
        let sym = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
        if !sym.is_null() {
            let f: extern "C" fn() -> u8 = unsafe { std::mem::transmute(sym) };
            return Some(flag(f()));
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn session_dictionary() -> Option<bool> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;

    // SAFETY: a public CoreGraphics function, looked up the same way; it follows the Create rule.
    let sym = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"CGSessionCopyCurrentDictionary".as_ptr()) };
    if sym.is_null() {
        return None;
    }
    let copy: extern "C" fn() -> CFDictionaryRef = unsafe { std::mem::transmute(sym) };
    let raw = copy();
    if raw.is_null() {
        return None;
    }
    let dict: CFDictionary<CFString, CFType> = unsafe { CFDictionary::wrap_under_create_rule(raw) };
    let value = dict.find(CFString::new("CGSSessionScreenIsShared"))?;
    value
        .downcast::<CFBoolean>()
        .map(bool::from)
        .or_else(|| value.downcast::<CFNumber>().and_then(|n| n.to_i32()).map(|n| n != 0))
}

#[cfg(not(target_os = "macos"))]
fn private_watcher() -> Option<bool> {
    None
}

#[cfg(not(target_os = "macos"))]
fn session_dictionary() -> Option<bool> {
    None
}

// --- Tauri wiring ---------------------------------------------------------------------------

use tauri::{AppHandle, Manager};

use crate::engine::Engine;

#[derive(Default)]
struct ScreenState {
    shared: AtomicBool,
    gate: Gate,
}

pub fn setup(app: &tauri::App) {
    app.manage(ScreenState::default());
    sync(app.handle());
}

/// Whether the last poll found the screen shared. Always false while the setting is off.
pub fn is_shared(app: &AppHandle) -> bool {
    // `try_state`: a redraw that runs before `setup` has registered the state must not panic.
    app.try_state::<ScreenState>().is_some_and(|s| s.shared.load(Ordering::SeqCst))
}

/// Starts or stops the poller to match the setting. Turning it off clears the state and redraws the tray.
pub fn sync(app: &AppHandle) {
    let Some(state) = app.try_state::<ScreenState>() else { return };
    let on = app.state::<Engine>().settings().hide_on_screen_share;
    let handle = app.clone();
    state.gate.sync(on, move |stop| {
        let spawned = std::thread::Builder::new().name("buku-token-screen".into()).spawn(move || poll(&handle, &stop));
        if let Err(e) = spawned {
            eprintln!("[buku-token] cannot watch the screen: {e}");
        }
    });
    if !on && state.shared.swap(false, Ordering::SeqCst) {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || crate::tray::update_icon(&app));
    }
}

fn poll(app: &AppHandle, stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) {
        let now = probe();
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if app.try_state::<ScreenState>().is_some_and(|s| changed(&s.shared, now)) {
            crate::tray::update_icon(app);
        }
        std::thread::sleep(POLL_EVERY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_private_function_result_is_read_as_a_byte() {
        assert!(!flag(0));
        assert!(flag(1));
        assert!(flag(2), "an odd value is true, not undefined");
        assert!(flag(255));
    }

    #[test]
    fn any_answering_source_that_says_shared_wins() {
        assert!(decide(Some(true), None));
        assert!(decide(None, Some(true)));
        assert!(decide(Some(false), Some(true)));
        assert!(!decide(Some(false), Some(false)));
    }

    #[test]
    fn missing_symbols_degrade_to_not_shared_without_failing() {
        assert!(!decide(None, None));
        assert!(!decide(None, Some(false)));
    }

    #[test]
    fn a_poll_only_reports_a_change() {
        let last = AtomicBool::new(false);
        assert!(!changed(&last, false));
        assert!(changed(&last, true));
        assert!(!changed(&last, true));
        assert!(changed(&last, false));
    }

    #[test]
    fn nothing_polls_while_the_setting_is_off() {
        let (gate, spawned) = (Gate::default(), Cell::new(0));
        assert!(!gate.sync(false, |_| spawned.set(spawned.get() + 1)));
        assert_eq!(spawned.get(), 0);
        assert!(!gate.running());
    }

    #[test]
    fn turning_on_starts_one_poller_and_off_stops_it() {
        let (gate, spawned) = (Gate::default(), Cell::new(0));
        let flag = Cell::new(None);
        assert!(gate.sync(true, |f| {
            spawned.set(spawned.get() + 1);
            flag.set(Some(f));
        }));
        assert!(!gate.sync(true, |_| spawned.set(spawned.get() + 1)), "already running");
        assert_eq!(spawned.get(), 1);
        let flag = flag.take().unwrap();
        assert!(!flag.load(Ordering::SeqCst));
        gate.sync(false, |_| unreachable!());
        assert!(flag.load(Ordering::SeqCst), "poller told to stop");
        assert!(!gate.running());
        assert!(gate.sync(true, |_| spawned.set(spawned.get() + 1)), "a new poller after off");
        assert_eq!(spawned.get(), 2);
    }
}
