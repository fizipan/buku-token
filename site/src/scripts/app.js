// Interactions that work with or without scroll motion.
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const S = JSON.parse(document.body.dataset.strings);

// Language: remember the choice, so "/" doesn't send someone back.
$("#lang").addEventListener("click", (e) => { try { localStorage.setItem("bt-lang", e.currentTarget.dataset.lang); } catch {} });

// App tabs in the panel
const cap = $("#cap"), shots = $$(".shot"), tabs = $$(".tabs button");
let touched = false;
function selectTab(tab, focus) {
  tabs.forEach((b) => { const on = b.dataset.tab === tab; b.setAttribute("aria-selected", on); b.tabIndex = on ? 0 : -1; if (on && focus) b.focus(); });
  shots.forEach((i) => { const on = i.dataset.tab === tab; i.classList.toggle("on", on); i.alt = on ? S.shotAlt.replace("%s", i.dataset.name) : ""; if (on) cap.innerHTML = `<b>${i.dataset.name}.</b> ${i.dataset.text}` + (touched ? "" : `<br>${S.hint}`); });
}
tabs.forEach((b, i) => {
  b.addEventListener("click", () => { touched = true; selectTab(b.dataset.tab); });
  b.addEventListener("keydown", (e) => {
    const d = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (d) { e.preventDefault(); touched = true; selectTab(tabs[(i + d + tabs.length) % tabs.length].dataset.tab, true); }
  });
});
// Load the other tabs early, so the first click doesn't fade to an empty panel.
addEventListener("load", () => { const go = () => shots.forEach((i) => { i.loading = "eager"; }); window.requestIdleCallback ? requestIdleCallback(go) : setTimeout(go, 400); });

// Tray item
const tray = $("#tray"), pop = $("#popover"), desk = $(".desk");
const setOpen = (open) => { pop.hidden = !open; tray.setAttribute("aria-expanded", open); };
tray.addEventListener("click", () => {
  // Past the hero the panel is out of sight, so the click takes you back to it instead of toggling it unseen.
  if (scrollY > desk.offsetHeight * 0.6) { setOpen(true); dispatchEvent(new CustomEvent("bt:top")); return; }
  setOpen(pop.hidden);
});

// Mobile menu
const menu = $("#menu"), navs = $("#navs");
const setMenu = (open) => { navs.classList.toggle("open", open); menu.setAttribute("aria-expanded", open); };
menu.addEventListener("click", () => setMenu(!navs.classList.contains("open")));
navs.addEventListener("click", (e) => { if (e.target.closest("a")) setMenu(false); });
document.addEventListener("click", (e) => { if (!e.target.closest("#menu, #navs")) setMenu(false); });
document.addEventListener("keydown", (e) => { if (e.key !== "Escape") return; if (navs.classList.contains("open")) { setMenu(false); menu.focus(); } else if (!pop.hidden && scrollY < desk.offsetHeight) { setOpen(false); tray.focus(); } });

// Copy buttons
$$("[data-copy]").forEach((btn) => btn.addEventListener("click", async () => {
  const target = $(btn.dataset.copy);
  try { await navigator.clipboard.writeText(target.textContent); btn.textContent = S.copied; setTimeout(() => { btn.textContent = S.copy; }, 1600); } catch { const r = document.createRange(); r.selectNodeContents(target); const s = getSelection(); s.removeAllRanges(); s.addRange(r); }
}));

// Clock, and the panel hanging centred under the tray item, like the app does.
function alignPopover() { const t = tray.getBoundingClientRect(); desk.style.setProperty("--pop-right", Math.max(12, Math.round(document.documentElement.clientWidth - (t.left + t.right) / 2 - 190)) + "px"); }
function clock() { $("#clock").textContent = new Date().toLocaleString(S.locale, { weekday: "short", hour: "numeric", minute: "2-digit" }).replace(",", ""); alignPopover(); }
clock(); setInterval(clock, 20000);
addEventListener("resize", alignPopover);
document.fonts?.ready.then(alignPopover);

// Day chart: pointing at a day rewrites the statement in place, so no tooltip can cover the chart.
const days = $("#days"), cols = $("#dayCols"), dayEls = [...cols.children], keyEls = $$("#key li");
const dayData = dayEls.map((c) => c.dataset.day.split(",").map(Number));
const usd = (v) => "$" + v.toFixed(2), sum = (a) => a.reduce((x, y) => x + y, 0);
let dayAt = -1;
const dayName = (i) => { const back = dayData.length - 1 - i; return back === 0 ? S.today : back === 1 ? S.yesterday : S.ago.replace("%s", back); };
function readDay() {
  const d = dayData[dayAt];
  $("#rLabel").textContent = d ? dayName(dayAt) : S.last30;
  $("#rCost").textContent = usd(d ? sum(d) : 600);
  keyEls.forEach((li, j) => { $("b", li).textContent = usd(d ? d[j] : +li.dataset.total); });
  days.classList.toggle("hovering", !!d);
  dayEls.forEach((c, i) => c.classList.toggle("on", i === dayAt));
}
days.addEventListener("pointermove", (e) => { const r = cols.getBoundingClientRect(); dayAt = Math.max(0, Math.min(dayData.length - 1, Math.floor(((e.clientX - r.left) / r.width) * dayData.length))); readDay(); });
days.addEventListener("pointerleave", (e) => { if (e.pointerType === "mouse") { dayAt = -1; readDay(); } });
days.addEventListener("keydown", (e) => {
  const last = dayData.length - 1, next = { ArrowLeft: Math.max(0, (dayAt < 0 ? last + 1 : dayAt) - 1), ArrowRight: Math.min(last, dayAt + 1), Home: 0, End: last, Escape: -1 }[e.key];
  if (next === undefined) return;
  e.preventDefault(); dayAt = next; readDay();
  $("#dayLive").textContent = dayAt < 0 ? "" : `${dayName(dayAt)}: ${usd(sum(dayData[dayAt]))}`;
});
days.addEventListener("blur", () => { dayAt = -1; readDay(); });

// Light and dark wipe
const wipe = $("#wipe"), range = $("input", wipe);
range.addEventListener("input", () => { wipe.dataset.touched = "1"; wipe.style.setProperty("--x", range.value + "%"); });
