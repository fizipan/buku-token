// Scroll motion. Everything is visible without this file, and it never runs under reduced motion.
import gsap from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { SplitText } from "gsap/SplitText";
import Lenis from "lenis";

const root = document.documentElement;

// Charts rise once they come into view, with or without smooth scroll.
function showWhenSeen(els) {
  const seen = new IntersectionObserver((list) => list.forEach((en) => { if (en.isIntersecting) { en.target.classList.add("in"); seen.unobserve(en.target); } }), { threshold: 0.2 });
  els.forEach((e) => seen.observe(e));
}

if (!root.classList.contains("motion")) {
  addEventListener("bt:top", () => document.querySelector(".desk").scrollIntoView());
} else {
  gsap.registerPlugin(ScrollTrigger, SplitText);
  const lenis = new Lenis({ lerp: 0.12, smoothWheel: true, syncTouch: false, anchors: true });
  lenis.on("scroll", ScrollTrigger.update);
  gsap.ticker.add((t) => lenis.raf(t * 1000));
  gsap.ticker.lagSmoothing(0);
  addEventListener("bt:top", () => lenis.scrollTo(0));

  const $$ = (s) => gsap.utils.toArray(s);
  const bar = document.querySelector(".bar");
  const afterSplash = root.dataset.splash === "on" ? 1.05 : 0.05;
  const start = async () => {
    await Promise.race([document.fonts.ready, new Promise((r) => setTimeout(r, 2500))]);

    // Hero entrance. Lift the first-paint guard first, so the tweens read the visible state as their end point.
    root.classList.add("ready");
    gsap.timeline({ delay: afterSplash })
      .from("#headline .ln > span", { yPercent: 112, duration: 1, ease: "power4.out", stagger: 0.1 })
      .from("[data-in]", { y: 18, autoAlpha: 0, duration: 0.7, ease: "power3.out", stagger: 0.06 }, 0.35)
      .fromTo(".pop-wrap", { y: -26, autoAlpha: 0 }, { y: 0, autoAlpha: 1, duration: 0.8, ease: "power3.out", clearProps: "all", onComplete: () => document.querySelector(".pop-wrap").classList.add("live") }, 0.25)
      .from(".wall-bars i", { scaleY: 0.03, transformOrigin: "bottom", duration: 1, ease: "power3.out", stagger: 0.03 }, 0.3);

    // Pins first, top to bottom, so every trigger below is measured with their extra scroll included.
    const mm = gsap.matchMedia();

    // Mosaic: columns of the app's own screens at uneven speeds, behind the wordmark.
    mm.add("(min-width: 860px) and (pointer: fine)", () => {
      $$(".m-col").forEach((col, i) => {
        gsap.to(col, { y: () => -innerHeight * [0.34, 0.12, 0.42, 0.2][i % 4], ease: "none", scrollTrigger: { trigger: ".mosaic", start: "top bottom", end: "bottom top", scrub: 0.4, invalidateOnRefresh: true } });
      });
    });

    // Three chapters, pinned. Scroll picks the chapter, and the next one wipes in from the right.
    mm.add("(min-width: 1041px) and (min-height: 700px)", () => {
      root.classList.add("pin-on");
      const panels = $$("[data-panel]"), last = panels.length - 1, STEP = 1.5;
      const at = (k) => (k === 0 ? 0 : (k - 1) * STEP + 0.75);
      const tl = gsap.timeline({ paused: true });
      panels.slice(1).forEach((p, i) => {
        gsap.set(p, { clipPath: "inset(0% 0% 0% 100%)" });
        tl.to(p, { clipPath: "inset(0% 0% 0% 0%)", duration: 0.75, ease: "power3.inOut" }, i * STEP);
      });
      let active = -1, tween;
      const show = (k) => {
        if (k === active) return;
        active = k;
        panels[k].querySelectorAll(".rise").forEach((e) => e.classList.add("in"));
        bar.classList.toggle("on-light", panels[k].dataset.tone === "light");
        tween?.kill();
        tween = tl.tweenTo(at(k), { duration: 0.8, ease: "power2.out" });
      };
      ScrollTrigger.create({ trigger: ".stage", start: "top 60%", once: true, onEnter: () => panels[0].querySelectorAll(".rise").forEach((e) => e.classList.add("in")) });
      const st = ScrollTrigger.create({
        trigger: ".stage", start: "top top", end: () => "+=" + innerHeight * last * 1.15, pin: true, invalidateOnRefresh: true,
        onUpdate: (self) => show(Math.round(self.progress * last)),
        onEnter: () => show(0), onEnterBack: (self) => show(Math.round(self.progress * last)),
        onLeave: () => bar.classList.remove("on-light"), onLeaveBack: () => bar.classList.remove("on-light"),
      });
      // Keyboard focus inside a chapter brings that chapter up, so focus is never on something hidden.
      const onFocus = (e) => { const k = panels.findIndex((p) => p.contains(e.target)); if (k >= 0) lenis.scrollTo(st.start + (k / last) * (st.end - st.start), { immediate: true }); };
      document.querySelector(".stage").addEventListener("focusin", onFocus);
      return () => { root.classList.remove("pin-on"); document.querySelector(".stage").removeEventListener("focusin", onFocus); gsap.set(panels, { clearProps: "clipPath" }); };
    });

    // Tools strip: vertical scroll moves it sideways.
    mm.add("(min-width: 1041px) and (min-height: 620px)", () => {
      root.classList.add("strip-on");
      const track = document.querySelector(".strip-track"), distance = () => Math.max(0, track.scrollWidth - innerWidth);
      gsap.to(track, { x: () => -distance(), ease: "none", scrollTrigger: { trigger: ".strip", start: "top top", end: () => "+=" + distance(), pin: true, scrub: 0.8, invalidateOnRefresh: true } });
      return () => root.classList.remove("strip-on");
    });

    // Closing: two walls of screens rise past the install command.
    mm.add("(min-width: 1200px) and (min-height: 620px)", () => {
      root.classList.add("walls-on");
      const tl = gsap.timeline({ scrollTrigger: { trigger: ".closer", start: "top top", end: () => "+=" + innerHeight * 1.7, pin: true, scrub: 0.6, invalidateOnRefresh: true } });
      tl.fromTo(".c-wall.l", { y: () => innerHeight }, { y: () => -document.querySelector(".c-wall.l").offsetHeight, ease: "none" }, 0)
        .fromTo(".c-wall.r", { y: () => innerHeight * 1.35 }, { y: () => -document.querySelector(".c-wall.r").offsetHeight * 0.9, ease: "none" }, 0);
      return () => root.classList.remove("walls-on");
    });

    // The outlined line fills in as you start to scroll.
    gsap.to(".ghost", { color: "#fafafa", ease: "none", scrollTrigger: { trigger: ".desk", start: "top top", end: "45% top", scrub: 0.4 } });

    // Section titles come up line by line.
    $$("[data-title]").forEach((el) => {
      const split = SplitText.create(el, { type: "lines", mask: "lines" });
      gsap.from(split.lines, { yPercent: 110, duration: 0.9, ease: "power4.out", stagger: 0.08, scrollTrigger: { trigger: el, start: "top 86%" }, onComplete: () => split.revert() });
    });

    // Marquee of tools that speeds up with the scroll
    const loop = gsap.to(".band-track", { xPercent: -50, duration: 34, ease: "none", repeat: -1 });
    lenis.on("scroll", ({ velocity }) => {
      gsap.to(loop, { timeScale: 1 + Math.min(Math.abs(velocity) * 0.25, 5), duration: 0.2, overwrite: true });
      gsap.to(loop, { timeScale: 1, duration: 1, delay: 0.2 });
    });

    // The menu bar takes the tone of what's under it, like macOS over a light wallpaper.
    $$("[data-tone=light]").forEach((section) => {
      if (section.closest(".stage")) return;
      ScrollTrigger.create({ trigger: section, start: "top 30px", end: "bottom 30px", toggleClass: { targets: bar, className: "on-light" } });
    });

    // Share card drifts against the scroll.
    gsap.fromTo("#card", { y: 50, rotate: 2.4 }, { y: -50, rotate: -1.6, ease: "none", scrollTrigger: { trigger: "#share", start: "top bottom", end: "bottom top", scrub: 0.5 } });

    // The wipe opens once, unless someone already took the handle.
    const wipe = document.querySelector("#wipe"), range = wipe.querySelector("input"), x = { v: 86 };
    ScrollTrigger.create({ trigger: wipe, start: "top 70%", once: true, onEnter: () => {
      if (wipe.dataset.touched) return;
      gsap.fromTo(x, { v: 86 }, { v: 50, duration: 1.5, ease: "power3.inOut", onUpdate: () => { if (wipe.dataset.touched) return; wipe.style.setProperty("--x", x.v + "%"); range.value = Math.round(x.v); } });
    } });

    showWhenSeen($$(".rise").filter((e) => !e.closest("[data-panel]") || !root.classList.contains("pin-on")));
    ScrollTrigger.refresh();
  };
  start();
}
if (!root.classList.contains("motion")) showWhenSeen([...document.querySelectorAll(".rise")]);
