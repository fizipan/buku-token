// Everything shown is the same invented account as the screenshots, so the figures reconcile:
// $18.40 today, $140 in 7 days, $600 in 30 days.
export const AGENTS = [
  { name: "Claude Code", id: "cc", total: 411.4 },
  { name: "Codex", id: "cx", total: 68.6 },
  { name: "OpenCode", id: "oc", total: 8.6 },
  { name: "Hermes", id: "hm", total: 6.4 },
  { name: "Cursor", id: "cr", total: 105 },
] as const;

// Per day, in the order of AGENTS, oldest first.
export const DAYS = [[4.63,.77,.1,.07,1.18],[18.83,3.14,.39,.29,4.81],[12.23,2.04,.26,.19,3.12],[14.75,2.46,.31,.23,3.77],[17.66,2.94,.37,.27,4.51],[7.77,1.3,.16,.12,1.98],[19.35,3.23,.4,.3,4.94],[9.33,1.56,.2,.15,2.38],[16.82,2.81,.35,.26,4.29],[15.87,2.65,.33,.25,4.05],[10.8,1.8,.23,.17,2.76],[19.18,3.2,.4,.3,4.9],[6.22,1.04,.13,.1,1.59],[18.32,3.06,.38,.28,4.68],[13.55,2.26,.28,.21,3.46],[13.54,2.26,.28,.21,3.46],[18.33,3.06,.38,.28,4.68],[6.2,1.03,.13,.1,1.58],[19.18,3.2,.4,.3,4.89],[10.81,1.8,.23,.17,2.76],[15.86,2.64,.33,.25,4.05],[16.83,2.81,.35,.26,4.3],[9.32,1.55,.2,.14,2.38],[17.55,2.4,.43,.15,6.14],[7.06,.97,.17,.06,2.47],[16.01,2.19,.39,.13,5.6],[13.39,1.83,.32,.11,4.69],[11.08,1.52,.27,.09,3.88],[18.1,4,0,.6,0],[12.8,3.1,.42,.36,1.72]];

// The Overview's $18.40 day, by hour.
export const HOURS: Record<number, number[]> = { 8: [0, 0, 0.42, 0, 0], 9: [1.4, 0, 0, 0, 0], 10: [1.6, 0, 0, 0, 0], 11: [0, 1.7, 0, 0, 0], 12: [0, 1.4, 0, 0, 0], 13: [2.6, 0, 0, 0, 0], 14: [4.2, 0, 0, 0, 0], 15: [3, 0, 0, 0, 0.5], 16: [0, 0, 0, 0.2, 0.8], 17: [0, 0, 0, 0.16, 0.42] };

export const PROJECTS = [
  { name: "web-store", cost: 42, by: [30, 6, 0, 0, 6], sessions: 34, share: 30 },
  { name: "admin-panel", cost: 32.5, by: [22, 4, 0, 0, 6.5], sessions: 28, share: 23 },
  { name: "mobile-api", cost: 22, by: [16, 2, 0, 0, 4], sessions: 19, share: 16 },
  { name: "landing-page", cost: 17, by: [10, 2, 2, 0, 3], sessions: 15, share: 12 },
  { name: "docs-site", cost: 11.5, by: [8, 1, 0, 0, 2.5], sessions: 11, share: 8 },
  { name: "billing-service", cost: 8.5, by: [6, 0.5, 0, 1.5, 0.5], sessions: 9, share: 6 },
  { name: null, cost: 6.5, by: [4, 0.5, 0, 0, 2], sessions: 7, share: 5 },
];

export const MODELS = [
  { name: "Opus 5.5", id: "cc", sym: "l-cc", cost: 78, share: 56 },
  { name: "Composer 1", id: "cr", sym: "l-cr", cost: 24.5, share: 18 },
  { name: "Sonnet 5.5", id: "cc", sym: "l-cc", cost: 18, share: 13 },
  { name: "GPT-5.2 Codex", id: "cx", sym: "l-cx", cost: 16, share: 11 },
  { name: "glm-4.7", id: "oc", sym: null, cost: 2, share: 1 },
  { name: "Haiku 4.5", id: "hm", sym: "l-cc", cost: 1.5, share: 1 },
  { name: "big-pickle", id: "oc", sym: null, cost: null, share: null },
];

export const SHOTS = [
  { tab: "overview", en: "Overview", en2: "Today's cost by agent, and the quota behind it.", id2: "Biaya hari ini per agent, dan kuota di baliknya." },
  { tab: "sessions", en: "Sessions", en2: "Every session by day, listed like a bank statement.", id2: "Tiap sesi per hari, tersusun seperti mutasi rekening." },
  { tab: "models", en: "Models", en2: "Which model took the money, and what cache saved.", id2: "Model mana yang paling menghabiskan uang, dan seberapa banyak yang dihemat cache." },
  { tab: "projects", en: "Projects", en2: "What each project cost, and who ran it.", id2: "Biaya tiap project, dan agent yang menjalankannya." },
  { tab: "history", en: "History", en2: "Daily spending, and whether a subscription paid for itself.", id2: "Pengeluaran harian, dan apakah langganan sudah balik modal." },
];

const en = {
  htmlLang: "en", other: "/id/", otherLabel: "ID", otherAria: "Ganti ke Bahasa Indonesia", self: "/",
  title: "Buku Token: Quota and Cost of AI Coding Agents in the Mac Menu Bar",
  description: "Free, open source Mac menu bar app that shows the quota left and the cost of Claude Code, Codex, Cursor and more. Mostly from local logs, with no telemetry.",
  ogImage: "/assets/og-en.png", ogLocale: "en_US", ogLocaleAlt: "id_ID", ogAlt: "Buku Token. Know what's left, and where it went. The app's panel beside the Homebrew install command, with example data.",
  ogDescription: "The quota left and the cost of your AI coding agents, in the Mac menu bar. Open source, with no telemetry.",
  skip: "Skip to content", nav: { features: "Features", tools: "Tools", privacy: "Privacy", share: "Share", install: "Install" }, menu: "Menu",
  trayLabel: "Show or hide the panel. Example quota: Claude 62% left, Codex 55%, Cursor 27%", tabsLabel: "App tabs",
  h1: ["Know what's left,", "and where it went."],
  lede: "Buku Token sits in your Mac's menu bar and shows the quota and cost of Claude Code, Codex, Cursor and more, mostly from logs they already write, with no new account and no telemetry.",
  cta: "View on GitHub", note: 'Free and MIT licensed. For Apple Silicon Macs. <a href="#notarized">Not notarized by Apple</a>.', copy: "Copy", copied: "Copied",
  hint: "Try the tabs in the panel. Example data.", shotAlt: "Buku Token popover, %s tab, with example data", wallCap: "Today's cost per hour. Example data.",
  leftH: "What's left.", leftSub: "Every quota in one list, with the time it resets. For Claude it also projects where you'll land at the reset, so you can tell early whether the next big task fits.",
  burnLabel: "Claude Code 5-hour session in example data: 38% used now, projected to about 71% at the reset.", q5h: "5h Session", qWeek: "Weekly", qMonth: "Monthly Total", qLeft: "Left", limit: "Limit", burnNow: "38% used", burnEnd: "About 71% at reset", now: "Now",
  leftFine: "Bars show what's used. Codex figures are as of your last session. Example data.",
  wentH: "Where it went.", wentSub: "Cost per day, split by agent, then by project and by model. Point at a day to read its line.", avg: "Avg $20.00/day. Example data.",
  daysLabel: "Daily cost over the last 30 days, example data. Use the arrow keys to read each day.", ago29: "29 days ago", today: "Today", yesterday: "Yesterday", ago: "%s days ago", last30: "Last 30 days",
  byProject: "By project", byModel: "By model", last7: "Last 7 days, $140.00", sessions: "sessions", noPrice: "No price yet", otherProjects: "2 other projects",
  breakH: "Then down to each project and model.",
  cacheNote: "Cache read takes 70% of tokens but only 22% of cost. Without cache, this period would be about $500. Example data.",
  worthH: "Was it worth it?", worthSub: "It puts what you pay next to what the same usage would cost at API prices, then projects the rest of the cycle. That's the number you want before you upgrade or downgrade.", worthFine: "API equivalent is an estimate, not a bill. Example data.",
  sum: ["Paid", "API equivalent", "payback"], planCc: "Day 19 of 31", planCx: "Day 10 of 30", paid1: "Paid $100", api1: "API equivalent $260", proj1: "Projected by cycle end ≈ $424 (4.2×)", paid2: "Paid $20", api2: "API equivalent $23", proj2: "Projected by cycle end ≈ $69 (3.5×)",
  toolsH: "What it can read from each tool.", toolsSub: "Some tools keep detailed logs, and some keep almost nothing. This is what Buku Token can get from each one.",
  thCost: "Tokens and cost", thQuota: "Quota", thSource: "Read from", grpIn: "Counted in your totals", grpOut: "Shown separately", none: "–",
  tools: [
    { name: "Claude Code", id: "cc", sym: "l-cc", cost: "Yes", quota: "Yes", src: "~/.claude/projects + Anthropic usage API" },
    { name: "Codex", id: "cx", sym: "l-cx", cost: "Yes", quota: "Yes, as of your last session", src: "~/.codex/sessions and archived_sessions" },
    { name: "Cursor", id: "cr", sym: "l-cr", cost: "After you connect it", quota: "After you connect it", src: "cursor.com + your session cookie" },
    { name: "OpenCode", id: "oc", sym: null, short: "OC", cost: "Yes", quota: null, src: "~/.local/share/opencode/opencode.db" },
    { name: "Hermes", id: "hm", sym: "l-hm", cost: "Yes", quota: null, src: "~/.hermes/state.db" },
    { name: "OpenRouter", id: "ne", sym: "l-or", out: true, cost: "Spend and balance", quota: "Key limit, if set", src: "openrouter.ai + your API key" },
    { name: "Antigravity", id: "ag", sym: "l-ag", out: true, cost: "Activity only", quota: null, src: "~/.gemini: file names, dates, step counts" },
    { name: "RTK", id: "ne", sym: null, short: "RTK", out: true, cost: "Tokens saved", quota: null, src: "the rtk command" },
  ],
  toolsFoot: "OpenRouter stays out of your totals, because OpenCode or Hermes may have counted the same spend already. Costs are estimates at API prices unless a tool reports its own, and a model with no known price shows a dash, never $0.",
  privH: "Your usage stays on your Mac.", privSub: "This app reads sensitive folders and holds credentials, so here is exactly what it touches.",
  readH: "What it reads", reads: [["Usage figures from agent logs and databases", "Opened read-only. Hermes and OpenCode are read from a temporary copy."], ["Your Claude login, from Keychain", "Only to read your quota. Never stored, logged or refreshed."], ["Your Cursor cookie and OpenRouter key", "Only if you paste them. Kept in Buku Token's own Keychain items until you disconnect. Those items survive uninstalling, so disconnect first. The cookie is a full account session, so treat it like a password."], ["Project folder names", "It walks folder names from / and checks for .git, so macOS may ask for folder access."]],
  neverH: "What it leaves alone", nevers: [["Project files", ""], ["Your conversations", "Logs are parsed for usage fields only."], ["Cursor's own database or Keychain", ""], ["Antigravity conversations, titles or your Google token", ""]],
  netH: "The only network calls", nets: [["api.anthropic.com", "Claude quota"], ["cursor.com", "Once you connect it"], ["openrouter.ai", "Once you connect it"], ["raw.githubusercontent.com", "Price list, when you press Update"]],
  shareH: "Share the number, not the details.", shareSub: "Turn a day, a week or a month into a card you can post. It's drawn on your Mac, with no upload and no link.", points: ["Choose all agents or just one.", "Hide the cost, and the card shows token shares instead.", "Project names, paths and emails never reach the image. Unknown model names show as “Other Model”."],
  shareAlt: "Example share card: $600 API equivalent over 30 days, split across Claude Code, Cursor, Codex, OpenCode and Hermes, with 71M tokens, 527 sessions and Opus 5.5 as top model",
  fitH: "Light or dark, English or Indonesian.", fitSub: "It follows your Mac's appearance, and you pick the language in Settings. Turn on notifications to hear when a quota drops below 15%, and it tries to hide the menu bar figures while you share your screen.",
  wipeLabel: "Compare light and dark mode", wipeHint: "Drag to compare.", darkAlt: "The Overview tab in dark mode, with example data", lightAlt: "The Overview tab in light mode, with example data",
  instH: "Install it with one command.", instSub: 'You need Homebrew and an Apple Silicon Mac. <a href="#notarized">Not notarized by Apple</a>.',
  in2: "Open it from Applications", in2p: "It stays in the menu bar, with no Dock icon. When macOS asks about <code>Claude Code-credentials</code>, choose Always Allow to see your Claude quota. Everything else works if you refuse.", in3: "Update it the same way",
  notar: "The app isn't notarized by Apple. It's signed with a self-signed certificate, so the cask removes the download quarantine flag to let macOS open it. If you'd rather not allow that, build it from source.",
  limits: "A few limits: Cursor's endpoints are undocumented and can change, Codex quota is only as fresh as your last session, and Antigravity shows activity without tokens or cost. The README lists the rest.",
  srcSum: "Other ways to install", dmg: 'There\'s a DMG on the <a href="https://github.com/fizipan/buku-token/releases">Releases page</a>. macOS blocks its first open, so go to System Settings, then Privacy &amp; Security, and choose Open Anyway.', srcNeeds: "To build from source, which Intel Macs also need, you'll want Node.js, pnpm, Rust and the Xcode Command Line Tools.",
  st1: "Create a signing certificate, once", st1p: "In Keychain Access, open Certificate Assistant and create a certificate named <code>Buku Token Local</code>, with Self-Signed Root and Code Signing. It lets macOS remember your Keychain answer after a rebuild.", st2: "Build the app", st3: "Move it to Applications and open it", st3p: "You'll find it at <code>src-tauri/<wbr>target/<wbr>release/<wbr>bundle/<wbr>macos/<wbr>Buku Token.app</code>. When macOS asks about <code>Claude Code-credentials</code>, choose Always Allow to see your Claude quota. Everything else works if you refuse.",
  license: "MIT License", security: "Security",
  foot1: "Screenshots use example data. Product names and logos belong to their owners and only identify each tool. Buku Token is not affiliated with any of them.",
  foot2: 'Prices come from <a href="https://github.com/BerriAI/litellm">LiteLLM</a>, most logos from <a href="https://github.com/lobehub/lobe-icons">LobeHub Icons</a>. <a href="https://github.com/robinebers/openusage">OpenUsage</a> inspired the share feature and the Settings layout.',
};

export type Strings = typeof en;

const id: Strings = {
  ...en,
  htmlLang: "id", other: "/", otherLabel: "EN", otherAria: "Switch to English", self: "/id/",
  title: "Buku Token: Kuota dan Biaya Agent AI Coding di Menu Bar Mac",
  description: "Aplikasi menu bar Mac gratis dan open source: sisa kuota dan biaya Claude Code, Codex, Cursor, dan lainnya. Sebagian besar dari log lokal, tanpa telemetri.",
  ogImage: "/assets/og-id.png", ogLocale: "id_ID", ogLocaleAlt: "en_US", ogAlt: "Buku Token. Tahu sisanya, dan ke mana perginya. Panel aplikasi di samping perintah instal Homebrew, dengan data contoh.",
  ogDescription: "Sisa kuota dan biaya agent AI coding kamu, di menu bar Mac. Open source, tanpa telemetri.",
  skip: "Lewati ke isi", nav: { features: "Fitur", tools: "Tool", privacy: "Privasi", share: "Bagikan", install: "Instal" },
  trayLabel: "Tampilkan atau sembunyikan panel. Contoh kuota: Claude sisa 62%, Codex 55%, Cursor 27%", tabsLabel: "Tab aplikasi",
  h1: ["Tahu sisanya,", "dan ke mana perginya."],
  lede: "Buku Token ada di menu bar Mac kamu dan menampilkan kuota serta biaya Claude Code, Codex, Cursor, dan lainnya, sebagian besar dari log yang sudah mereka tulis, tanpa akun baru dan tanpa telemetri.",
  cta: "Lihat di GitHub", note: 'Gratis dan berlisensi MIT. Untuk Mac Apple Silicon. <a href="#notarized">Belum dinotarisasi Apple</a>.', copy: "Salin", copied: "Tersalin",
  hint: "Coba tab di panel. Data contoh.", shotAlt: "Popover Buku Token, tab %s, dengan data contoh", wallCap: "Biaya per jam hari ini. Data contoh.",
  leftH: "Sisanya.", leftSub: "Semua kuota dalam satu daftar, lengkap dengan waktu reset-nya. Untuk Claude ada juga proyeksi posisimu saat reset, jadi kamu tahu lebih awal apakah task besar berikutnya masih muat.",
  burnLabel: "Sesi 5 jam Claude Code dalam data contoh: 38% terpakai sekarang, diproyeksikan sekitar 71% saat reset.", q5h: "Sesi 5 Jam", qWeek: "Mingguan", qMonth: "Total Bulanan", qLeft: "Sisa", burnNow: "38% terpakai", burnEnd: "Sekitar 71% saat reset", now: "Sekarang",
  leftFine: "Bar menunjukkan yang sudah terpakai. Angka Codex per sesi terakhirmu. Data contoh.",
  wentH: "Ke mana perginya.", wentSub: "Biaya per hari, dipecah per agent, lalu per project dan per model. Arahkan ke satu hari untuk membaca barisnya.", avg: "Rata-rata $20.00/hari. Data contoh.",
  daysLabel: "Biaya harian 30 hari terakhir, data contoh. Pakai tombol panah untuk membaca tiap hari.", ago29: "29 hari lalu", today: "Hari ini", yesterday: "Kemarin", ago: "%s hari lalu", last30: "30 hari terakhir",
  byProject: "Per project", byModel: "Per model", last7: "7 hari terakhir, $140.00", sessions: "sesi", noPrice: "Belum ada harga", otherProjects: "2 project lain",
  breakH: "Lalu turun ke tiap project dan model.",
  cacheNote: "Cache read memakan 70% token tapi hanya 22% biaya. Tanpa cache, periode ini sekitar $500. Data contoh.",
  worthH: "Sepadan atau tidak?", worthSub: "Yang kamu bayar disandingkan dengan biaya pemakaian yang sama di harga API, lalu sisa siklusnya diproyeksikan. Angka ini yang kamu perlukan sebelum naik atau turun paket.", worthFine: "Setara API adalah perkiraan, bukan tagihan. Data contoh.",
  sum: ["Bayar", "setara API", "balik modal"], planCc: "Hari ke-19 dari 31", planCx: "Hari ke-10 dari 30", paid1: "Bayar $100", api1: "Setara API $260", proj1: "Proyeksi akhir siklus ≈ $424 (4.2×)", paid2: "Bayar $20", api2: "Setara API $23", proj2: "Proyeksi akhir siklus ≈ $69 (3.5×)",
  toolsH: "Apa yang terbaca dari tiap tool.", toolsSub: "Sebagian tool menyimpan log lengkap, sebagian nyaris tidak. Ini yang bisa didapat Buku Token dari tiap tool.",
  thCost: "Token dan biaya", thQuota: "Kuota", thSource: "Dibaca dari", grpIn: "Masuk total kamu", grpOut: "Ditampilkan terpisah",
  tools: [
    { name: "Claude Code", id: "cc", sym: "l-cc", cost: "Ya", quota: "Ya", src: "~/.claude/projects + Anthropic usage API" },
    { name: "Codex", id: "cx", sym: "l-cx", cost: "Ya", quota: "Ya, per sesi terakhirmu", src: "~/.codex/sessions dan archived_sessions" },
    { name: "Cursor", id: "cr", sym: "l-cr", cost: "Setelah dihubungkan", quota: "Setelah dihubungkan", src: "cursor.com + cookie sesi kamu" },
    { name: "OpenCode", id: "oc", sym: null, short: "OC", cost: "Ya", quota: null, src: "~/.local/share/opencode/opencode.db" },
    { name: "Hermes", id: "hm", sym: "l-hm", cost: "Ya", quota: null, src: "~/.hermes/state.db" },
    { name: "OpenRouter", id: "ne", sym: "l-or", out: true, cost: "Pengeluaran dan saldo", quota: "Limit key, kalau ada", src: "openrouter.ai + API key kamu" },
    { name: "Antigravity", id: "ag", sym: "l-ag", out: true, cost: "Aktivitas saja", quota: null, src: "~/.gemini: nama file, tanggal, jumlah langkah" },
    { name: "RTK", id: "ne", sym: null, short: "RTK", out: true, cost: "Token yang dihemat", quota: null, src: "perintah rtk" },
  ],
  toolsFoot: "OpenRouter tidak masuk total, karena pengeluaran yang sama bisa saja sudah dihitung OpenCode atau Hermes. Biaya adalah perkiraan harga API, kecuali tool melaporkan biayanya sendiri, dan model tanpa harga tampil sebagai tanda strip, bukan $0.",
  privH: "Data pemakaianmu tetap di Mac kamu.", privSub: "Aplikasi ini membaca folder sensitif dan memegang kredensial, jadi inilah persisnya yang disentuh.",
  readH: "Yang dibaca", reads: [["Angka pemakaian dari log dan database agent", "Dibuka read-only. Hermes dan OpenCode dibaca dari salinan sementara."], ["Login Claude kamu, dari Keychain", "Hanya untuk membaca kuota. Tidak pernah disimpan, dicatat, atau diperbarui."], ["Cookie Cursor dan key OpenRouter", "Hanya kalau kamu tempel. Disimpan di item Keychain milik Buku Token sampai kamu putuskan. Item itu tetap ada setelah uninstall, jadi putuskan dulu. Cookie itu adalah sesi akun penuh, jadi perlakukan seperti kata sandi."], ["Nama folder project", "Menelusuri nama folder dari / dan memeriksa .git, jadi macOS bisa meminta izin folder."]],
  neverH: "Yang tidak disentuh", nevers: [["Isi file project", ""], ["Percakapan kamu", "Log hanya diurai untuk kolom pemakaian."], ["Database atau Keychain milik Cursor", ""], ["Percakapan dan judul Antigravity, atau token Google kamu", ""]],
  netH: "Satu-satunya koneksi jaringan", nets: [["api.anthropic.com", "Kuota Claude"], ["cursor.com", "Setelah dihubungkan"], ["openrouter.ai", "Setelah dihubungkan"], ["raw.githubusercontent.com", "Daftar harga, saat kamu menekan Perbarui"]],
  shareH: "Bagikan angkanya, bukan detailnya.", shareSub: "Ubah sehari, seminggu, atau sebulan jadi kartu yang bisa kamu posting. Dibuat di Mac kamu, tanpa unggahan dan tanpa tautan.", points: ["Pilih semua agent atau satu saja.", "Sembunyikan biaya, dan kartu menampilkan porsi token.", "Nama project, path, dan email tidak pernah masuk gambar. Nama model yang tidak dikenal tampil sebagai “Model Lain”."],
  shareAlt: "Contoh kartu bagikan: setara API $600 selama 30 hari, terbagi ke Claude Code, Cursor, Codex, OpenCode, dan Hermes, dengan 71M token, 527 sesi, dan Opus 5.5 sebagai model teratas",
  fitH: "Terang atau gelap, Indonesia atau Inggris.", fitSub: "Tampilannya mengikuti Mac kamu, dan bahasanya dipilih di Pengaturan. Nyalakan notifikasi supaya diberi tahu saat kuota di bawah 15%, dan aplikasi berusaha menyembunyikan angka di menu bar saat kamu berbagi layar.",
  wipeLabel: "Bandingkan mode terang dan gelap", wipeHint: "Geser untuk membandingkan.", darkAlt: "Tab Overview dalam mode gelap, dengan data contoh", lightAlt: "Tab Overview dalam mode terang, dengan data contoh",
  instH: "Instal dengan satu perintah.", instSub: 'Kamu butuh Homebrew dan Mac Apple Silicon. <a href="#notarized">Belum dinotarisasi Apple</a>.',
  in2: "Buka dari Applications", in2p: "Aplikasinya ada di menu bar, tanpa ikon di Dock. Saat macOS bertanya soal <code>Claude Code-credentials</code>, pilih Always Allow supaya kuota Claude terbaca. Bagian lain tetap jalan kalau ditolak.", in3: "Perbarui dengan cara yang sama",
  notar: "Aplikasi ini belum dinotarisasi Apple. Tanda tangannya memakai sertifikat self-signed, jadi cask menghapus tanda karantina unduhan supaya macOS mau membukanya. Kalau kamu tidak mau itu, build dari source saja.",
  limits: "Beberapa keterbatasan: endpoint Cursor tidak terdokumentasi dan bisa berubah, kuota Codex hanya sesegar sesi terakhirmu, dan Antigravity menampilkan aktivitas tanpa token atau biaya. Selengkapnya ada di README.",
  srcSum: "Cara instal lain", dmg: 'Ada DMG di <a href="https://github.com/fizipan/buku-token/releases">halaman Releases</a>. macOS memblokir saat pertama dibuka, jadi buka System Settings, lalu Privacy &amp; Security, dan pilih Open Anyway.', srcNeeds: "Untuk build dari source, yang juga dibutuhkan Mac Intel, kamu perlu Node.js, pnpm, Rust, dan Xcode Command Line Tools.",
  st1: "Buat sertifikat signing, sekali saja", st1p: "Di Keychain Access, buka Certificate Assistant dan buat sertifikat bernama <code>Buku Token Local</code>, dengan Self-Signed Root dan Code Signing. Dengan ini macOS mengingat jawaban Keychain kamu setelah build ulang.", st2: "Build aplikasinya", st3: "Pindahkan ke Applications dan buka", st3p: "Hasilnya ada di <code>src-tauri/<wbr>target/<wbr>release/<wbr>bundle/<wbr>macos/<wbr>Buku Token.app</code>. Saat macOS bertanya soal <code>Claude Code-credentials</code>, pilih Always Allow supaya kuota Claude terbaca. Bagian lain tetap jalan kalau ditolak.",
  license: "Lisensi MIT", security: "Keamanan",
  foot1: "Screenshot memakai data contoh, dalam bahasa Inggris. Nama produk dan logo milik pemiliknya masing-masing dan hanya menandai tiap tool. Buku Token tidak berafiliasi dengan mereka.",
  foot2: 'Harga dari <a href="https://github.com/BerriAI/litellm">LiteLLM</a>, sebagian besar logo dari <a href="https://github.com/lobehub/lobe-icons">LobeHub Icons</a>. <a href="https://github.com/robinebers/openusage">OpenUsage</a> menjadi inspirasi fitur bagikan dan tata letak Pengaturan.',
};

export const STRINGS = { en, id };
