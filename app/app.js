(() => {
  "use strict";

  // --- Session token bootstrapping (scrubbed from URL immediately) ---
  let SESSION_TOKEN = "";
  const hashMatch = window.location.hash.match(/token=([0-9a-fA-F]+)/);
  if (hashMatch) {
    SESSION_TOKEN = hashMatch[1];
    try {
      window.history.replaceState(null, "", window.location.pathname);
    } catch (_) {}
  }

  // --- Crisp Monochrome Vector Icon Library (Zero Emojis in Chrome) ---
  const ICO = {
    copy: `<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="13" height="13" x="9" y="9" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`,
    check: `<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`,
    trendUp: `<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="22 7 13.5 15.5 8.5 10.5 2 17"/><polyline points="16 7 22 7 22 13"/></svg>`,
    trendDown: `<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="22 17 13.5 8.5 8.5 13.5 2 7"/><polyline points="16 17 22 17 22 11"/></svg>`,
    shield: `<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10"/></svg>`,
    receipt: `<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 2v20l2-1 2 1 2-1 2 1 2-1 2 1 2-1 2 1V2l-2 1-2-1-2 1-2-1-2 1-2-1-2 1Z"/><path d="M8 8h8M8 12h6"/></svg>`,
    key: `<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><circle cx="7.5" cy="15.5" r="4.5"/><path d="m21 2-9.6 9.6"/><path d="m15.5 7.5 3 3L22 7l-3-3"/></svg>`,
    inbox: `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><polyline points="22 12 16 12 14 15 10 15 8 12 2 12"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/></svg>`,
    sparkle: `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3Z"/></svg>`,
    plus: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>`,
    calendar: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><rect width="18" height="18" x="3" y="4" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/></svg>`,
    chart: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M3 3v18h18"/><path d="m19 9-5 5-4-4-3 3"/></svg>`,
    bolt: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/></svg>`,
    eye: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z"/><circle cx="12" cy="12" r="3"/></svg>`,
    download: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/></svg>`,
    lock: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><rect width="18" height="11" x="3" y="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>`,
    sun: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2m0 16v2M4.93 4.93l1.41 1.41m11.32 11.32 1.41 1.41M2 12h2m16 0h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/></svg>`,
    laptop: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><rect width="16" height="12" x="4" y="4" rx="2"/><path d="M2 20h20"/></svg>`,
    refresh: `<svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16"/><path d="M16 16h5v5"/></svg>`,
  };

  const THEMES = [
    { id: "daylight", label: "纯净白昼" },
    { id: "porcelain", label: "暖玉极简" },
    { id: "graphite", label: "静谧深色" },
  ];

  const CHART_PALETTE = [
    "#2563eb",
    "#0d9488",
    "#7c3aed",
    "#f59e0b",
    "#e11d48",
    "#0284c7",
    "#10b981",
    "#6366f1",
  ];

  const BRAND_PRESETS = {
    openai: { bg: "#10a37f", mark: "AI" },
    anthropic: { bg: "#b85d3b", mark: "CL" },
    macbook: { bg: "#18181b", mark: "MB" },
    apple: { bg: "#18181b", mark: "AP" },
    github: { bg: "#24292f", mark: "GH" },
    jetbrains: { bg: "#6d28d9", mark: "JB" },
    netflix: { bg: "#dc2626", mark: "NF" },
    spotify: { bg: "#15803d", mark: "SP" },
    notion: { bg: "#334155", mark: "NT" },
    figma: { bg: "#7c3aed", mark: "FG" },
    raycast: { bg: "#e11d48", mark: "RC" },
    wps: { bg: "#2563eb", mark: "WP" },
    金山: { bg: "#2563eb", mark: "金" },
    支付宝: { bg: "#0284c7", mark: "支" },
    微信: { bg: "#16a34a", mark: "微" },
    哔哩: { bg: "#db2777", mark: "B" },
    腾讯: { bg: "#0284c7", mark: "腾" },
    阿里: { bg: "#ea580c", mark: "阿" },
    京东: { bg: "#dc2626", mark: "京" },
    百度: { bg: "#2563eb", mark: "百" },
  };

  const CURRENCY_SYMBOLS = {
    CNY: "¥",
    USD: "$",
    EUR: "€",
    GBP: "£",
    JPY: "JP¥",
    HKD: "HK$",
    CAD: "CA$",
    AUD: "AU$",
    SGD: "S$",
    TWD: "NT$",
    KRW: "₩",
    CHF: "CHF ",
  };

  const CURRENCY_NAMES = {
    CNY: "人民币",
    USD: "美元",
    EUR: "欧元",
    GBP: "英镑",
    JPY: "日元",
    HKD: "港币",
    CAD: "加元",
    AUD: "澳元",
    SGD: "新元",
    TWD: "新台币",
    KRW: "韩元",
    CHF: "瑞士法郎",
  };

  const CYCLE_LABELS = {
    monthly: "月付",
    yearly: "年付",
    quarterly: "季付",
    weekly: "周付",
    one_time: "一次性",
    unknown: "未标注",
  };

  const ASSET_KIND_META = {
    subscription: { label: "订阅", full: "周期订阅", pillClass: "pill-green" },
    license: { label: "买断", full: "买断授权", pillClass: "pill-blue" },
    hardware: { label: "硬件", full: "硬件保修", pillClass: "pill-iris" },
    reimbursable: { label: "报销", full: "报销凭证", pillClass: "pill-amber" },
    general: { label: "账单", full: "普通账单", pillClass: "pill-slate" },
  };

  const REIMBURSE_META = {
    pending: { label: "待报销", next: "submitted", cls: "pending" },
    submitted: { label: "已提交", next: "reimbursed", cls: "submitted" },
    reimbursed: { label: "已到账", next: "pending", cls: "reimbursed" },
  };

  const COLLECTION_TITLES = {
    all: "全部资产",
    subscription: "周期订阅",
    license: "买断与密钥",
    hardware: "硬件与保修",
    reimbursable: "报销跟进",
    csv: "CSV 流水挑拣",
    needs: "待补录与核对",
  };

  const state = {
    bootstrap: null,
    workspace: null,
    theme: "daylight",
    privacyBlur: false,
    lobbyTab: "unlock",
    unlockMode: "passphrase",
    activeNav: "assets",       // "assets" | "calendar" | "insights" | "triage" | "security"
    collection: "all",         // "all" | "subscription" | "license" | "hardware" | "reimbursable" | "csv" | "needs"
    viewMode: "cards",         // "cards" | "table"
    accountFilter: "",
    currencyFilter: "",
    categoryFilter: "",
    sortOrder: "recent",
    searchQuery: "",
    calendarYear: new Date().getFullYear(),
    calendarMonth: new Date().getMonth(),
    insightsCurrency: "",
    simCutIds: new Set(),
    triageSkippedIds: new Set(),
    selectedSourceId: null,
    activeDrawerTab: "edit",
    pendingRecoveryKey: null,
    textViewActive: false,
    lastRedactedText: "",
    manualKind: "subscription",
    manualCycle: "monthly",
    editKind: "general",
    cmdOpen: false,
    cmdIndex: 0,
    cmdActions: [],
    dragDepth: 0,
  };

  const $ = (id) => document.getElementById(id);

  function esc(str) {
    return String(str ?? "")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
  }

  function brandStyleFor(name) {
    const raw = (name || "账").trim();
    const low = raw.toLowerCase();
    for (const [key, preset] of Object.entries(BRAND_PRESETS)) {
      if (low.includes(key)) return preset;
    }
    let h = 0;
    for (let i = 0; i < raw.length; i++) h = (h * 31 + raw.charCodeAt(i)) >>> 0;
    const bg = CHART_PALETTE[h % CHART_PALETTE.length];
    const first = Array.from(raw)[0] || "S";
    const mark = /[\u4e00-\u9fff]/.test(first) ? first : raw.slice(0, 2).toUpperCase();
    return { bg, mark };
  }

  function formatWithCommas(intStr) {
    return String(intStr).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  }

  // Editorial typographic numeral: crisp bold integer + softer decimal
  function renderEditorialMoney(currency, amountText, showCode = true) {
    if (!amountText) return "";
    const sym = CURRENCY_SYMBOLS[currency] || "";
    const [whole, frac] = String(amountText).split(".");
    const formattedInt = formatWithCommas(whole || "0");
    const decPart = frac !== undefined ? `<span class="money-dec">.${esc(frac)}</span>` : "";
    const codePart = showCode ? `<span class="money-code">${esc(currency)}</span>` : "";
    return `${esc(sym)}${esc(formattedInt)}${decPart}${codePart}`;
  }

  function formatMoney(currency, amountText) {
    if (!amountText) return "";
    const sym = CURRENCY_SYMBOLS[currency] || `${currency} `;
    const [whole, frac] = String(amountText).split(".");
    return `${sym}${formatWithCommas(whole || "0")}${frac !== undefined ? `.${frac}` : ""} ${currency}`;
  }

  function formatMoneyShort(currency, amountText) {
    if (!amountText) return "";
    const sym = CURRENCY_SYMBOLS[currency] || "";
    const [whole, frac] = String(amountText).split(".");
    return `${sym}${formatWithCommas(whole || "0")}${frac !== undefined ? `.${frac}` : ""}`;
  }

  function amountToCents(text) {
    if (!text) return 0;
    const parts = String(text).split(".");
    const w = parseInt(parts[0], 10) || 0;
    const f = parseInt((parts[1] || "00").slice(0, 2).padEnd(2, "0"), 10) || 0;
    return w * 100 + f;
  }

  function centsToText(cents) {
    const abs = Math.abs(Math.round(cents));
    const sign = cents < 0 ? "-" : "";
    return `${sign}${Math.floor(abs / 100)}.${String(abs % 100).padStart(2, "0")}`;
  }

  function formatBytes(n) {
    if (!Number.isFinite(n)) return "";
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / (1024 * 1024)).toFixed(2)} MB`;
  }

  function formatTime(ms) {
    if (!ms) return "";
    try {
      return new Date(ms).toLocaleString("zh-CN", { hour12: false });
    } catch (_) {
      return String(ms);
    }
  }

  let toastTimer = null;
  function showToast(msg, kind = "info") {
    const el = $("toast");
    $("toast-msg").textContent = msg;
    el.className = `toast ${kind === "error" ? "toast-error" : kind === "ok" ? "toast-ok" : ""}`;
    el.classList.remove("hidden");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => el.classList.add("hidden"), 4200);
  }

  async function api(method, path, bodyObj) {
    const headers = { "X-SubVault-Token": SESSION_TOKEN };
    const opts = { method, headers };
    if (bodyObj !== undefined) {
      headers["Content-Type"] = "application/json";
      opts.body = JSON.stringify(bodyObj);
    }
    const resp = await fetch(path, opts);
    const data = await resp.json().catch(() => ({ ok: false, message: "响应解析失败" }));
    if (!resp.ok || data.ok === false) {
      const err = new Error(data.message || data.error || `请求失败 (${resp.status})`);
      err.code = data.error || "request_failed";
      err.status = resp.status;
      throw err;
    }
    return data;
  }

  function tokenUrl(path) {
    const sep = path.includes("?") ? "&" : "?";
    return `${path}${sep}token=${encodeURIComponent(SESSION_TOKEN)}`;
  }

  function arrayBufferToBase64(buffer) {
    const bytes = new Uint8Array(buffer);
    let binary = "";
    const chunk = 0x8000;
    for (let i = 0; i < bytes.length; i += chunk) {
      binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
    }
    return btoa(binary);
  }

  function downloadTextFile(filename, content) {
    const blob = new Blob([content], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  // --- Theme & Privacy ---
  function applyTheme(themeId) {
    const found = THEMES.find((t) => t.id === themeId) || THEMES[0];
    state.theme = found.id;
    document.documentElement.setAttribute("data-theme", found.id);
    if ($("lobby-theme-label")) $("lobby-theme-label").textContent = found.label;
    try {
      localStorage.setItem("subvault_ui_theme_v2", found.id);
    } catch (_) {}
  }

  function cycleTheme() {
    const idx = THEMES.findIndex((t) => t.id === state.theme);
    const next = THEMES[(idx + 1) % THEMES.length];
    applyTheme(next.id);
    showToast(`已切换至「${next.label}」外观`);
  }

  function togglePrivacy(forceVal) {
    state.privacyBlur = forceVal !== undefined ? forceVal : !state.privacyBlur;
    document.body.classList.toggle("privacy-blur", state.privacyBlur);
    $("btn-privacy-toggle").classList.toggle("active", state.privacyBlur);
    if (state.privacyBlur) {
      showToast("防窥模式已开启：金额、账号与密钥已模糊（悬停单条可临时查看）");
    } else if (forceVal === undefined) {
      showToast("防窥模式已关闭");
    }
  }

  // --- Celebration Confetti ---
  function launchConfetti() {
    const canvas = $("confetti-canvas");
    if (!canvas) return;
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    canvas.classList.remove("hidden");
    const ctx = canvas.getContext("2d");
    const pieces = Array.from({ length: 70 }, () => ({
      x: Math.random() * canvas.width,
      y: -20 - Math.random() * 120,
      r: 4 + Math.random() * 4,
      dx: (Math.random() - 0.5) * 4,
      dy: 2.6 + Math.random() * 4.2,
      rot: Math.random() * 360,
      drot: (Math.random() - 0.5) * 10,
      color: CHART_PALETTE[Math.floor(Math.random() * CHART_PALETTE.length)],
    }));
    let frame = 0;
    function tick() {
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      pieces.forEach((p) => {
        p.x += p.dx;
        p.y += p.dy;
        p.dy += 0.08;
        p.rot += p.drot;
        ctx.save();
        ctx.translate(p.x, p.y);
        ctx.rotate((p.rot * Math.PI) / 180);
        ctx.fillStyle = p.color;
        ctx.fillRect(-p.r, -p.r * 0.5, p.r * 2, p.r);
        ctx.restore();
      });
      frame += 1;
      if (frame < 105) requestAnimationFrame(tick);
      else {
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        canvas.classList.add("hidden");
      }
    }
    requestAnimationFrame(tick);
  }

  // --- Asset Metric Helpers ---
  function maskLicenseKey(key) {
    const s = String(key || "").trim();
    if (!s) return "";
    if (s.length <= 4) return "••••";
    return `•••• •••• ${s.slice(-4)}`;
  }

  function periodProgressInfo(perVal) {
    if (!perVal || !perVal.period_start || !perVal.period_end) return null;
    const s = new Date(`${perVal.period_start}T00:00:00`);
    const e = new Date(`${perVal.period_end}T00:00:00`);
    if (isNaN(s.getTime()) || isNaN(e.getTime()) || e <= s) return null;
    const now = new Date();
    const pct = Math.max(0, Math.min(100, Math.round(((now - s) / (e - s)) * 100)));
    const daysLeft = Math.ceil((e - now) / (1000 * 60 * 60 * 24));
    const statusLabel = daysLeft > 0 ? `还剩 ${daysLeft} 天` : `已于 ${perVal.period_end} 届满`;
    return { pct, expired: daysLeft <= 0, range: `${perVal.period_start} — ${perVal.period_end}`, statusLabel };
  }

  function warrantyInfo(buyDate, warrantyEnd) {
    if (!warrantyEnd || !/^\d{4}-\d{2}-\d{2}$/.test(warrantyEnd)) return null;
    const end = new Date(`${warrantyEnd}T00:00:00`);
    if (isNaN(end.getTime())) return null;
    const now = new Date();
    const daysLeft = Math.ceil((end - now) / (1000 * 60 * 60 * 24));
    let pct = daysLeft <= 0 ? 100 : 50;
    if (buyDate && /^\d{4}-\d{2}-\d{2}$/.test(buyDate)) {
      const start = new Date(`${buyDate}T00:00:00`);
      if (!isNaN(start.getTime()) && end > start) {
        pct = Math.max(0, Math.min(100, Math.round(((now - start) / (end - start)) * 100)));
      }
    }
    const level = daysLeft < 0 ? "slate" : daysLeft <= 60 ? "amber" : "iris";
    const statusLabel = daysLeft > 0
      ? `保修剩 ${daysLeft} 天`
      : daysLeft === 0
        ? "今日保修到期"
        : `已过保 ${Math.abs(daysLeft)} 天`;
    return { daysLeft, pct, level, statusLabel, warrantyEnd };
  }

  function costPerDayInfo(buyDate, currency, amountText) {
    if (!buyDate || !/^\d{4}-\d{2}-\d{2}$/.test(buyDate) || !amountText) return null;
    const start = new Date(`${buyDate}T00:00:00`);
    const now = new Date();
    if (isNaN(start.getTime()) || start > now) return null;
    const daysUsed = Math.max(1, Math.floor((now - start) / (1000 * 60 * 60 * 24)));
    const cents = amountToCents(amountText);
    if (cents <= 0) return null;
    const perDayCents = Math.max(1, Math.round(cents / daysUsed));
    return { daysUsed, perDayText: formatMoneyShort(currency, centsToText(perDayCents)) };
  }

  async function copyLicenseKey(vendor, key) {
    try {
      await navigator.clipboard.writeText(key);
      showToast(`「${vendor || "该授权"}」激活密钥已复制`, "ok");
    } catch (_) {
      showToast("剪贴板被限制，请点开详情查看密钥", "error");
    }
  }

  async function cycleReimburseStatus(sourceId, currentStatus) {
    const cur = REIMBURSE_META[currentStatus] ? currentStatus : "pending";
    const next = REIMBURSE_META[cur].next;
    try {
      const snap = await api("POST", "/api/sources/reimburse", {
        source_id: sourceId,
        reimburse_status: next,
      });
      state.workspace = snap;
      renderWorkspace();
      showToast(`报销进度已更新为「${REIMBURSE_META[next].label}」`, "ok");
    } catch (err) {
      showToast(err.message, "error");
    }
  }

  // --- Bootstrap & Lobby ---
  async function boot() {
    try {
      const savedTheme = localStorage.getItem("subvault_ui_theme_v2");
      applyTheme(savedTheme || "daylight");
    } catch (_) {
      applyTheme("daylight");
    }
    bindEvents();
    if (!SESSION_TOKEN) {
      showToast("缺少本地访问凭证，请通过双击 subvault-app 打开。", "error");
      return;
    }
    try {
      const info = await api("GET", "/api/bootstrap");
      state.bootstrap = info;
      const curs = info.currencies || ["CNY", "USD", "EUR", "GBP", "JPY", "HKD"];
      populateCurrencySelect($("ed-currency"), curs);
      populateCurrencySelect($("man-currency"), curs);
      $("create-path").value = info.default_vault_path || "";

      const recents = info.recent_vaults || [];
      renderRecentVaults(recents);
      if (recents.length > 0) {
        selectRecentVault(recents[0]);
        setLobbyTab("unlock");
      } else {
        $("unlock-path").value = info.default_vault_path || "";
        setLobbyTab("create");
      }

      if (info.active_vault) {
        await refreshWorkspace();
        showWorkspaceScreen();
      } else {
        showLobbyScreen();
      }
    } catch (err) {
      showToast(err.message, "error");
    }
  }

  function populateCurrencySelect(sel, list) {
    if (!sel) return;
    sel.innerHTML = list
      .map((code) => `<option value="${esc(code)}">${esc(code)} ${esc(CURRENCY_SYMBOLS[code] || "")}</option>`)
      .join("");
  }

  function setLobbyTab(tab) {
    state.lobbyTab = tab;
    document.querySelectorAll("[data-lobby-tab]").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-lobby-tab") === tab);
    });
    $("lobby-pane-unlock").classList.toggle("hidden", tab !== "unlock");
    $("lobby-pane-create").classList.toggle("hidden", tab !== "create");
  }

  function renderRecentVaults(recent) {
    const container = $("recent-vaults-list");
    const emptyEl = $("recent-vaults-empty");
    if (!recent.length) {
      container.innerHTML = "";
      emptyEl.classList.remove("hidden");
      return;
    }
    emptyEl.classList.add("hidden");
    container.innerHTML = recent
      .map((v, idx) => {
        const tag = v.needs_passphrase
          ? `<span class="pill-tag pill-green">已加密</span>`
          : `<span class="pill-tag pill-slate">系统盘</span>`;
        return `
          <button type="button" class="recent-item" data-recent-idx="${idx}">
            <div style="min-width:0">
              <div class="recent-item-name">${esc(v.name)}</div>
              <div class="recent-item-path">${esc(v.path)}</div>
            </div>
            ${tag}
          </button>
        `;
      })
      .join("");
    container.querySelectorAll("[data-recent-idx]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const target = recent[Number(btn.getAttribute("data-recent-idx"))];
        if (target) {
          setLobbyTab("unlock");
          selectRecentVault(target);
        }
      });
    });
  }

  function selectRecentVault(v) {
    $("unlock-path").value = v.path || "";
    setUnlockMode(v.needs_passphrase ? "passphrase" : "os-disk");
    if (v.needs_passphrase) $("unlock-passphrase").focus();
  }

  function setUnlockMode(mode) {
    state.unlockMode = mode;
    document.querySelectorAll("[data-unlock-mode]").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-unlock-mode") === mode);
    });
    $("unlock-passphrase-wrap").classList.toggle("hidden", mode !== "passphrase");
    $("unlock-recovery-wrap").classList.toggle("hidden", mode !== "recovery");
  }

  function showLobbyScreen() {
    $("screen-lobby").classList.remove("hidden");
    $("screen-workspace").classList.add("hidden");
    closeDrawer();
  }

  function showWorkspaceScreen() {
    $("screen-lobby").classList.add("hidden");
    $("screen-workspace").classList.remove("hidden");
    renderWorkspace();
  }

  async function refreshWorkspace() {
    const snap = await api("GET", "/api/workspace");
    state.workspace = snap;
    renderWorkspace();
    if (state.selectedSourceId) {
      const stillThere = (snap.items || []).find((i) => i.source_id === state.selectedSourceId);
      if (stillThere) renderDrawer(stillThere);
      else closeDrawer();
    }
  }

  // --- Navigation & Collections ---
  function navigateTo(navKey, collectionKey) {
    state.activeNav = navKey;
    if (navKey === "assets" && collectionKey !== undefined) {
      state.collection = collectionKey;
    }

    document.querySelectorAll("[data-nav]").forEach((b) => {
      const isMatch = b.getAttribute("data-nav") === navKey && (navKey !== "assets" || state.collection === "all");
      b.classList.toggle("active", isMatch);
    });
    document.querySelectorAll("[data-collection]").forEach((b) => {
      const isMatch = navKey === "assets" && b.getAttribute("data-collection") === state.collection;
      b.classList.toggle("active", isMatch);
    });

    ["assets", "calendar", "insights", "triage", "security"].forEach((name) => {
      $(`pane-${name}`).classList.toggle("hidden", name !== navKey);
    });
    $("stage-assets-controls").classList.toggle("hidden", navKey !== "assets");

    if (state.workspace) {
      renderStageHeader(state.workspace);
      if (navKey === "assets") renderAssetsPane(state.workspace);
      else if (navKey === "calendar") renderCalendarPane(state.workspace);
      else if (navKey === "insights") renderInsightsPane(state.workspace);
      else if (navKey === "triage") renderTriagePane(state.workspace);
      else if (navKey === "security") renderSecurityAttention(state.workspace);
    }
  }

  // --- Workspace Rendering ---
  function renderWorkspace() {
    const ws = state.workspace;
    if (!ws) return;
    const v = ws.vault;
    const ac = ws.asset_counts || {};
    const triageCount = getTriageQueue(ws).length;
    const attentionCount = (v.duplicate_count || 0) + (v.conflict_count || 0) + (v.needs_entry_count || 0);

    $("sb-vault-name").textContent = v.name || "我的账本";
    $("sb-vault-status").textContent = v.encrypted ? "已加密 · 本机离线" : "系统盘模式 · 本机";
    $("sb-vault-dot").className = `status-dot ${v.encrypted ? "green" : "slate"}`;
    $("sb-vault-path").textContent = v.path || "";

    $("zip-ack-row").classList.toggle("hidden", !v.encrypted);
    $("dir-ack-row").classList.toggle("hidden", !v.encrypted);
    $("card-security-rotate").classList.toggle("hidden", !v.encrypted);

    $("cnt-all").textContent = String((ws.items || []).length);
    $("cnt-sub").textContent = ac.subscription > 0 ? String(ac.subscription) : "";
    $("cnt-lic").textContent = ac.license > 0 ? String(ac.license) : "";
    $("cnt-hw").textContent = ac.hardware > 0 ? String(ac.hardware) : "";
    $("cnt-reimb").textContent = ac.reimbursable > 0 ? String(ac.reimbursable) : "";
    $("cnt-csv").textContent = v.csv_statement_count > 0 ? String(v.csv_statement_count) : "";

    const triEl = $("cnt-triage");
    triEl.textContent = String(triageCount);
    triEl.classList.toggle("hidden", triageCount === 0);

    const attnEl = $("cnt-attention");
    attnEl.textContent = String(attentionCount);
    attnEl.classList.toggle("hidden", attentionCount === 0);

    syncFilterDropdowns(ws);
    renderStageHeader(ws);
    renderAssetsPane(ws);

    if (state.activeNav === "calendar") renderCalendarPane(ws);
    if (state.activeNav === "insights") renderInsightsPane(ws);
    if (state.activeNav === "triage") renderTriagePane(ws);
    if (state.activeNav === "security") renderSecurityAttention(ws);
  }

  function renderStageHeader(ws) {
    const titles = {
      assets: COLLECTION_TITLES[state.collection] || "全部资产",
      calendar: "续费与保修日历",
      insights: "洞察与省钱沙盘",
      triage: "极速整理",
      security: "核对与备份",
    };
    $("stage-title").textContent = titles[state.activeNav] || "全部资产";

    if (state.activeNav === "assets") {
      $("stage-count").textContent = `${filteredItems(ws).length} 项`;
    } else if (state.activeNav === "triage") {
      $("stage-count").textContent = `剩余 ${getTriageQueue(ws).length} 项`;
    } else {
      $("stage-count").textContent = "";
    }

    const chips = [];
    if (state.categoryFilter) {
      chips.push(`<span class="active-chip">分类：${esc(state.categoryFilter)} <button type="button" data-clear-filter="category" aria-label="清除">×</button></span>`);
    }
    if (state.accountFilter) {
      chips.push(`<span class="active-chip">账号：${esc(state.accountFilter)} <button type="button" data-clear-filter="account" aria-label="清除">×</button></span>`);
    }
    if (state.currencyFilter) {
      chips.push(`<span class="active-chip">币种：${esc(state.currencyFilter)} <button type="button" data-clear-filter="currency" aria-label="清除">×</button></span>`);
    }
    const chipBox = $("stage-active-chips");
    chipBox.innerHTML = chips.join("");
    chipBox.querySelectorAll("[data-clear-filter]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const key = btn.getAttribute("data-clear-filter");
        if (key === "category") state.categoryFilter = "";
        if (key === "account") state.accountFilter = "";
        if (key === "currency") state.currencyFilter = "";
        renderWorkspace();
      });
    });
  }

  function syncFilterDropdowns(ws) {
    const accSel = $("filter-account");
    accSel.innerHTML = `<option value="">所有账号</option>` +
      (ws.accounts || []).map((a) => `<option value="${esc(a)}">${esc(a)}</option>`).join("");
    accSel.value = state.accountFilter;

    const curSel = $("filter-currency");
    const curs = (ws.summary_by_currency || []).map((c) => c.currency);
    curSel.innerHTML = `<option value="">所有币种</option>` +
      curs.map((c) => `<option value="${esc(c)}">${esc(c)} · ${esc(CURRENCY_NAMES[c] || c)}</option>`).join("");
    curSel.value = state.currencyFilter;
  }

  function filteredItems(ws) {
    const q = state.searchQuery.trim().toLowerCase();
    const list = (ws.items || []).filter((item) => {
      const ann = item.annotation || {};
      const amtVal = item.amount_fact ? item.amount_fact.value : null;
      const flags = item.status_flags || {};
      const kind = ann.asset_kind || "general";

      if (state.collection === "subscription" && kind !== "subscription") return false;
      if (state.collection === "license" && kind !== "license") return false;
      if (state.collection === "hardware" && kind !== "hardware") return false;
      if (state.collection === "reimbursable") {
        if (kind !== "reimbursable" && (!ann.reimburse_status || ann.reimburse_status === "none")) return false;
      }
      if (state.collection === "csv" && !flags.is_csv_statement) return false;
      if (state.collection === "needs") {
        if (!flags.needs_manual_entry && !flags.has_conflict && !flags.is_credit_or_order_warning) return false;
      }

      if (state.accountFilter && (ann.account_label || "") !== state.accountFilter) return false;
      if (state.currencyFilter && (!amtVal || amtVal.currency !== state.currencyFilter)) return false;
      if (state.categoryFilter && (ann.category || "未分类") !== state.categoryFilter) return false;

      if (q) {
        const hay = [
          ann.vendor,
          ann.plan_name,
          ann.account_label,
          ann.category,
          ann.notes,
          ann.license_scope,
          ann.warranty_end,
          item.display_name,
          amtVal ? `${amtVal.currency} ${amtVal.amount_text}` : "",
        ]
          .filter(Boolean)
          .join(" ")
          .toLowerCase();
        if (!hay.includes(q)) return false;
      }
      return true;
    });

    if (state.sortOrder === "amount_desc") {
      list.sort((a, b) => amountToCents(b.amount_fact?.value?.amount_text) - amountToCents(a.amount_fact?.value?.amount_text));
    } else if (state.sortOrder === "date_desc") {
      list.sort((a, b) => {
        const da = a.annotation?.bill_date || a.period_fact?.value?.period_start || "";
        const db = b.annotation?.bill_date || b.period_fact?.value?.period_start || "";
        return db.localeCompare(da);
      });
    }
    return list;
  }

  // --- View 1: Assets Canvas (Hero Bento + Cards / List) ---
  function renderAssetsPane(ws) {
    const demoCount = ws.vault?.demo_count || 0;
    const demoBar = $("demo-notice-bar");
    if (demoCount > 0) {
      $("demo-notice-msg").textContent = `当前账本包含 ${demoCount} 项体验示例数据，随时可一键清除且不影响你的真实账单。`;
      demoBar.classList.remove("hidden");
    } else {
      demoBar.classList.add("hidden");
    }

    renderHeroBento(ws);

    const hasItems = (ws.items || []).length > 0;
    $("assets-subbar").classList.toggle("hidden", !hasItems);

    document.querySelectorAll("[data-view-mode]").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-view-mode") === state.viewMode);
    });

    const stage = $("ledger-stage");
    const list = filteredItems(ws);

    if (list.length === 0) {
      if (!hasItems) {
        stage.innerHTML = `
          <div class="empty-welcome">
            <div class="empty-icon-wrap">${ICO.inbox}</div>
            <h3>开始整理你的第一份资产或账单</h3>
            <p>直接把 PDF 发票、邮件 HTML、支付宝 / 微信 CSV 账单或收据截图拖进窗口，或点下方按钮立刻开始。</p>
            <div class="btn-row" style="justify-content:center">
              <button type="button" id="btn-empty-import" class="btn btn-primary btn-sm">选择本地文件导入</button>
              <button type="button" id="btn-empty-manual" class="btn btn-secondary btn-sm">手动记一笔</button>
              <button type="button" id="btn-load-demo-pack" class="btn btn-ghost btn-sm">载入体验示例数据（随时可一键清空）</button>
            </div>
          </div>
        `;
        $("btn-empty-import").addEventListener("click", () => $("file-picker-input").click());
        $("btn-empty-manual").addEventListener("click", openManualModal);
        const demoBtn = $("btn-load-demo-pack");
        demoBtn.addEventListener("click", async () => {
          demoBtn.disabled = true;
          demoBtn.textContent = "正在载入示例资产…";
          try {
            const snap = await api("POST", "/api/import-demo-pack", {});
            state.workspace = snap;
            renderWorkspace();
            showToast("示例资产包已就绪，上方随时可一键清空", "ok");
          } catch (err) {
            showToast(err.message, "error");
            demoBtn.disabled = false;
            demoBtn.textContent = "载入体验示例数据（随时可一键清空）";
          }
        });
      } else {
        stage.innerHTML = `
          <div class="empty-welcome">
            <h3>当前筛选下没有匹配的资产</h3>
            <p>可以清空搜索词或切换左侧分类查看全部已归档记录。</p>
            <button type="button" id="btn-reset-filters" class="btn btn-secondary btn-sm">重置筛选条件</button>
          </div>
        `;
        $("btn-reset-filters").addEventListener("click", () => {
          state.searchQuery = "";
          $("search-box").value = "";
          state.accountFilter = "";
          state.currencyFilter = "";
          state.categoryFilter = "";
          navigateTo("assets", "all");
        });
      }
      return;
    }

    if (state.viewMode === "cards") renderCardsGallery(list, stage);
    else renderTableView(list, stage);
  }

  function renderHeroBento(ws) {
    const hero = $("hero-bento");
    const v = ws.vault;
    const curList = ws.summary_by_currency || [];
    const ac = ws.asset_counts || {};
    const radar = ws.asset_radar || [];

    if ((ws.items || []).length === 0) {
      hero.classList.add("hidden");
      return;
    }
    hero.classList.remove("hidden");

    const pillars = curList.map((c) => {
      const recurText = c.recurring_count > 0
        ? `周期订阅约 <strong class="sensitive num">${esc(formatMoneyShort(c.currency, c.monthly_equiv_recurring))}/月</strong> · 年合 <span class="sensitive num">${esc(formatMoneyShort(c.currency, c.yearly_equiv_recurring))}</span>`
        : `共 ${c.bill_count} 笔已入账记录`;
      return `
        <div class="hero-pillar">
          <div class="pillar-head">
            <span>${esc(c.currency)} · ${esc(CURRENCY_NAMES[c.currency] || "账单汇总")}</span>
            <span class="pill-tag pill-slate">${c.bill_count} 笔</span>
          </div>
          <div class="pillar-amount sensitive">${renderEditorialMoney(c.currency, c.total_recorded, false)}</div>
          <div class="pillar-sub">${recurText}</div>
        </div>
      `;
    });

    const parts = [];
    if (ac.subscription > 0) parts.push(`订阅 ${ac.subscription}`);
    if (ac.license > 0) parts.push(`买断 ${ac.license}`);
    if (ac.hardware > 0) parts.push(`硬件 ${ac.hardware}`);
    if (v.duplicate_count > 0) parts.push(`已防重 ${v.duplicate_count}`);

    pillars.push(`
      <div class="hero-pillar">
        <div class="pillar-head">
          <span>原件加密档案</span>
          <span class="pill-tag pill-green">${ICO.check} 完整</span>
        </div>
        <div class="pillar-amount">${v.source_count}<span class="money-code">份原件</span></div>
        <div class="pillar-sub">${parts.join(" · ") || "原件字节防篡改存证"}</div>
      </div>
    `);

    const radarHtml = radar.length > 0
      ? `
        <div class="hero-radar">
          <span class="radar-eyebrow">需要关注</span>
          ${radar
            .map((r, idx) => {
              const icon = r.kind === "price_hike"
                ? ICO.trendUp
                : r.kind === "warranty_expiring"
                  ? ICO.shield
                  : ICO.receipt;
              const tone = r.kind === "price_hike"
                ? "rose"
                : r.kind === "warranty_expiring"
                  ? (r.days_left <= 14 ? "rose" : "iris")
                  : r.kind === "reimburse_pending"
                    ? "amber"
                    : "slate";
              return `
                <button type="button" class="radar-btn ${tone}" data-radar-idx="${idx}" title="${esc(r.detail || "")}">
                  ${icon}
                  <span class="sensitive">${esc(r.label)}</span>
                </button>
              `;
            })
            .join("")}
        </div>
      `
      : "";

    hero.innerHTML = `<div class="hero-pillars">${pillars.join("")}</div>${radarHtml}`;
    hero.querySelectorAll("[data-radar-idx]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const r = radar[Number(btn.getAttribute("data-radar-idx"))];
        if (!r) return;
        if (r.source_id) {
          const found = (ws.items || []).find((i) => i.source_id === r.source_id);
          if (found) openDrawer(found);
        } else if (r.kind === "reimburse_pending" || r.kind === "reimburse_submitted") {
          navigateTo("assets", "reimbursable");
        }
      });
    });
  }

  function renderCardsGallery(list, stage) {
    const html = list
      .map((item) => {
        const ann = item.annotation || {};
        const amtVal = item.amount_fact ? item.amount_fact.value : null;
        const perVal = item.period_fact ? item.period_fact.value : null;
        const flags = item.status_flags || {};
        const kind = ann.asset_kind || "general";
        const kindMeta = ASSET_KIND_META[kind] || ASSET_KIND_META.general;
        const title = ann.vendor || item.display_name;
        const sub = ann.plan_name || (ann.vendor ? item.display_name : item.media_type);
        const brand = brandStyleFor(title);
        const drift = item.price_drift || null;

        let driftTag = "";
        if (drift) {
          const isUp = drift.direction === "up";
          const pct = Math.abs(drift.pct_change || 0);
          driftTag = `<span class="pill-tag ${isUp ? "pill-rose" : "pill-green"}" title="对比上期 ${esc(drift.prev_text)} 变动 ${esc(drift.diff_text)}">${isUp ? ICO.trendUp : ICO.trendDown} ${isUp ? `+${pct}%` : `-${pct}%`}</span>`;
        }

        let amountRow = "";
        if (amtVal && amtVal.amount_text) {
          const cycleLabel = kind === "hardware"
            ? "一次购入"
            : kind === "license"
              ? "买断授权"
              : (CYCLE_LABELS[ann.billing_cycle] || "未标注");
          amountRow = `
            <div class="ac-price-group">
              <div class="ac-amount sensitive">${renderEditorialMoney(amtVal.currency, amtVal.amount_text, true)}</div>
              ${driftTag}
            </div>
            <span class="ac-cycle">${esc(cycleLabel)}</span>
          `;
        } else if (flags.is_csv_statement) {
          amountRow = `<span class="pill-tag pill-slate">CSV 流水 · ${item.csv_row_count} 条可挑拣</span><span class="ac-cycle">展开挑拣</span>`;
        } else if (flags.is_credit_or_order_warning) {
          amountRow = `<span class="pill-tag pill-amber">退款或未付订单</span><span class="ac-cycle">未计入</span>`;
        } else {
          amountRow = `<span class="ac-missing">待补录金额</span><span class="ac-cycle">点击完善</span>`;
        }

        // Context micro-module tailored to asset_kind
        let contextModule = "";
        if (kind === "hardware") {
          const w = warrantyInfo(ann.bill_date, ann.warranty_end);
          const cpd = amtVal ? costPerDayInfo(ann.bill_date, amtVal.currency, amtVal.amount_text) : null;
          const segs = [];
          if (w) {
            segs.push(`
              <div class="progress-module">
                <div class="progress-meta">
                  <span>保修至 ${esc(w.warrantyEnd)}</span>
                  <span>${esc(w.statusLabel)}</span>
                </div>
                <div class="progress-track"><div class="progress-fill ${w.level}" style="width:${w.pct}%"></div></div>
              </div>
            `);
          }
          if (cpd) {
            segs.push(`
              <div class="hw-row">
                <span>已陪伴 ${cpd.daysUsed} 天</span>
                <span class="cpd-chip sensitive">日均 ${esc(cpd.perDayText)}</span>
              </div>
            `);
          }
          contextModule = segs.join("");
        } else if (kind === "license") {
          const segs = [];
          if (ann.license_scope) {
            segs.push(`<div class="hw-row"><span>${esc(ann.license_scope)}</span></div>`);
          }
          if (ann.license_key) {
            segs.push(`
              <div class="key-capsule">
                <span class="sensitive">${esc(maskLicenseKey(ann.license_key))}</span>
                <button type="button" class="key-copy-btn" data-copy-key-sid="${esc(item.source_id)}">${ICO.copy} 复制</button>
              </div>
            `);
          }
          contextModule = segs.join("");
        } else {
          const prog = periodProgressInfo(perVal);
          if (prog) {
            contextModule = `
              <div class="progress-module">
                <div class="progress-meta">
                  <span>${esc(prog.range)}</span>
                  <span>${esc(prog.statusLabel)}</span>
                </div>
                <div class="progress-track"><div class="progress-fill ${prog.expired ? "slate" : ""}" style="width:${prog.pct}%"></div></div>
              </div>
            `;
          } else if (ann.bill_date) {
            contextModule = `<div class="hw-row"><span>日期 ${esc(ann.bill_date)}</span></div>`;
          }
        }

        const reimbStatus = ann.reimburse_status && ann.reimburse_status !== "none"
          ? ann.reimburse_status
          : kind === "reimbursable"
            ? "pending"
            : null;
        const reimbAction = reimbStatus && REIMBURSE_META[reimbStatus]
          ? `<button type="button" class="reimb-pill ${REIMBURSE_META[reimbStatus].cls}" data-reimb-sid="${esc(item.source_id)}" data-reimb-cur="${esc(reimbStatus)}" title="点击切换下一状态">${REIMBURSE_META[reimbStatus].label} ${ICO.refresh}</button>`
          : "";

        const metaText = [ann.account_label, ann.category, flags.has_duplicate ? `防重 ×${item.occurrence_count}` : ""]
          .filter(Boolean)
          .join(" · ") || "未分组";

        const topRightTag = flags.has_conflict
          ? `<span class="pill-tag pill-amber">金额冲突</span>`
          : `<span class="pill-tag ${kindMeta.pillClass}">${esc(kindMeta.label)}</span>`;

        return `
          <article class="asset-card" data-source-id="${esc(item.source_id)}">
            <div class="ac-top">
              <div class="ac-id">
                <div class="monogram" style="background:${brand.bg}">${esc(brand.mark)}</div>
                <div style="min-width:0">
                  <div class="ac-title">${esc(title)}</div>
                  <div class="ac-sub">${esc(sub)}</div>
                </div>
              </div>
              ${topRightTag}
            </div>
            <div class="ac-amount-row">${amountRow}</div>
            ${contextModule}
            <div class="ac-foot">
              <span class="ac-meta-dots sensitive">${esc(metaText)}</span>
              ${reimbAction || (item.object_intact ? `<span class="ac-verified" title="原件字节完整无篡改">${ICO.check}</span>` : "")}
            </div>
          </article>
        `;
      })
      .join("");

    stage.innerHTML = `<div class="asset-grid">${html}</div>`;
    bindLedgerInteractiveButtons(stage);
  }

  function renderTableView(list, stage) {
    const rowsHtml = list
      .map((item) => {
        const ann = item.annotation || {};
        const amtVal = item.amount_fact ? item.amount_fact.value : null;
        const perVal = item.period_fact ? item.period_fact.value : null;
        const flags = item.status_flags || {};
        const kind = ann.asset_kind || "general";
        const kindMeta = ASSET_KIND_META[kind] || ASSET_KIND_META.general;
        const title = ann.vendor || item.display_name;
        const brand = brandStyleFor(title);
        const sub = [ann.plan_name, ann.account_label].filter(Boolean).join(" · ");

        let amtCell = "";
        if (amtVal && amtVal.amount_text) {
          const drift = item.price_drift;
          const dTag = drift
            ? ` <span class="pill-tag ${drift.direction === "up" ? "pill-rose" : "pill-green"}">${drift.direction === "up" ? `+${Math.abs(drift.pct_change)}%` : `-${Math.abs(drift.pct_change)}%`}</span>`
            : "";
          amtCell = `<span class="sensitive num" style="font-weight:700">${esc(formatMoney(amtVal.currency, amtVal.amount_text))}</span>${dTag}`;
        } else if (flags.is_csv_statement) {
          amtCell = `<span class="pill-tag pill-slate">${item.csv_row_count} 条流水</span>`;
        } else {
          amtCell = `<span class="ac-missing">待补录</span>`;
        }

        let dateCell = perVal?.period_start
          ? `${perVal.period_start} — ${perVal.period_end}`
          : ann.bill_date || "—";
        if (kind === "hardware" && ann.warranty_end) dateCell = `保修至 ${ann.warranty_end}`;

        const actions = [];
        if (kind === "license" && ann.license_key) {
          actions.push(`<button type="button" class="key-copy-btn" data-copy-key-sid="${esc(item.source_id)}">${ICO.copy} 密钥</button>`);
        }
        const reimbStatus = ann.reimburse_status && ann.reimburse_status !== "none"
          ? ann.reimburse_status
          : kind === "reimbursable" ? "pending" : null;
        if (reimbStatus && REIMBURSE_META[reimbStatus]) {
          actions.push(`<button type="button" class="reimb-pill ${REIMBURSE_META[reimbStatus].cls}" data-reimb-sid="${esc(item.source_id)}" data-reimb-cur="${esc(reimbStatus)}">${REIMBURSE_META[reimbStatus].label} ${ICO.refresh}</button>`);
        }

        return `
          <tr data-source-id="${esc(item.source_id)}">
            <td>
              <div class="tbl-vendor">
                <div class="monogram monogram-sm" style="background:${brand.bg}">${esc(brand.mark)}</div>
                <div>
                  <strong>${esc(title)}</strong>
                  ${sub ? `<small class="sensitive">${esc(sub)}</small>` : ""}
                </div>
              </div>
            </td>
            <td><span class="pill-tag ${kindMeta.pillClass}">${esc(kindMeta.label)}</span></td>
            <td style="color:var(--ink-secondary)">${esc(CYCLE_LABELS[ann.billing_cycle] || "—")}</td>
            <td class="right">${amtCell}</td>
            <td style="font-size:12px;color:var(--ink-muted);white-space:nowrap">${esc(dateCell)}</td>
            <td class="right"><div class="btn-row" style="justify-content:flex-end">${actions.join("") || (item.object_intact ? `<span class="ac-verified">${ICO.check} 原件</span>` : "")}</div></td>
          </tr>
        `;
      })
      .join("");

    stage.innerHTML = `
      <div class="table-sheet">
        <table class="clean-table">
          <thead>
            <tr>
              <th>资产与归属</th>
              <th>类型</th>
              <th>周期</th>
              <th class="right">金额</th>
              <th>日期 / 保修</th>
              <th class="right">快捷操作</th>
            </tr>
          </thead>
          <tbody>${rowsHtml}</tbody>
        </table>
      </div>
    `;
    bindLedgerInteractiveButtons(stage);
  }

  function bindLedgerInteractiveButtons(stage) {
    stage.querySelectorAll("[data-copy-key-sid]").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        const found = (state.workspace.items || []).find((i) => i.source_id === btn.getAttribute("data-copy-key-sid"));
        if (found?.annotation?.license_key) {
          copyLicenseKey(found.annotation.vendor || found.display_name, found.annotation.license_key);
        }
      });
    });
    stage.querySelectorAll("[data-reimb-sid]").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        cycleReimburseStatus(btn.getAttribute("data-reimb-sid"), btn.getAttribute("data-reimb-cur"));
      });
    });
    stage.querySelectorAll("[data-source-id]").forEach((row) => {
      row.addEventListener("click", () => {
        const found = (state.workspace.items || []).find((i) => i.source_id === row.getAttribute("data-source-id"));
        if (found) openDrawer(found);
      });
    });
  }

  // --- View 2: Dedicated Calendar ---
  function renderCalendarPane(ws) {
    const pane = $("pane-calendar");
    const year = state.calendarYear;
    const month = state.calendarMonth;
    const firstDay = new Date(year, month, 1);
    const startWeekday = firstDay.getDay();
    const daysInMonth = new Date(year, month + 1, 0).getDate();
    const prevMonthDays = new Date(year, month, 0).getDate();
    const today = new Date();

    const eventsByDay = {};
    let eventCount = 0;
    const addEvt = (day, evt) => {
      if (!eventsByDay[day]) eventsByDay[day] = [];
      eventsByDay[day].push(evt);
      eventCount += 1;
    };

    // Identify the latest bill per recurring (vendor, currency, billing_cycle) so superseded past bills don't project duplicates
    const latestRecurringKey = new Map();
    for (const item of ws.items || []) {
      const ann = item.annotation || {};
      const amtVal = item.amount_fact ? item.amount_fact.value : null;
      if (!amtVal || !amtVal.amount_text) continue;
      if (!["monthly", "quarterly", "yearly"].includes(ann.billing_cycle)) continue;
      const key = `${(ann.vendor || item.display_name).trim().toLowerCase()}|${amtVal.currency}|${ann.billing_cycle}`;
      const dStr = ann.bill_date || item.period_fact?.value?.period_start || "";
      const prev = latestRecurringKey.get(key);
      if (!prev || dStr >= prev.dStr) {
        latestRecurringKey.set(key, { sid: item.source_id, dStr });
      }
    }
    const projectableIds = new Set([...latestRecurringKey.values()].map((v) => v.sid));

    for (const item of ws.items || []) {
      const ann = item.annotation || {};
      const amtVal = item.amount_fact ? item.amount_fact.value : null;
      const perVal = item.period_fact ? item.period_fact.value : null;
      const title = ann.vendor || item.display_name;

      if (ann.warranty_end && /^\d{4}-\d{2}-\d{2}$/.test(ann.warranty_end)) {
        const [wy, wm, wd] = ann.warranty_end.split("-").map(Number);
        if (wy === year && wm === month + 1) {
          addEvt(wd, { kind: "warranty", source_id: item.source_id, title, money: "保修截止" });
        }
      }

      if (!amtVal || !amtVal.amount_text) continue;
      const money = formatMoneyShort(amtVal.currency, amtVal.amount_text);
      const realDateStr = ann.bill_date || (perVal && perVal.period_start);
      let matchedRealDay = null;

      if (realDateStr && /^\d{4}-\d{2}-\d{2}$/.test(realDateStr)) {
        const [ry, rm, rd] = realDateStr.split("-").map(Number);
        if (ry === year && rm === month + 1) {
          matchedRealDay = rd;
          addEvt(rd, { kind: "real", source_id: item.source_id, title, money });
        }
      }

      if (!projectableIds.has(item.source_id)) continue;
      const anchorStr = (perVal && perVal.period_end) || realDateStr;
      if (anchorStr && /^\d{4}-\d{2}-\d{2}$/.test(anchorStr)) {
        const [ay, am, ad] = anchorStr.split("-").map(Number);
        const diff = (year * 12 + month) - (ay * 12 + (am - 1));
        const clampedDay = Math.min(ad, daysInMonth);
        if (diff >= 0 && clampedDay !== matchedRealDay) {
          let hits = false;
          if (ann.billing_cycle === "monthly") hits = true;
          else if (ann.billing_cycle === "quarterly" && diff % 3 === 0) hits = true;
          else if (ann.billing_cycle === "yearly" && diff % 12 === 0) hits = true;
          if (hits && !(diff === 0 && perVal?.period_end && matchedRealDay === clampedDay)) {
            addEvt(clampedDay, { kind: "projected", source_id: item.source_id, title, money });
          }
        }
      }
    }

    const weekdays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
    let cells = weekdays.map((w) => `<div class="cal-wk">${w}</div>`).join("");

    for (let i = startWeekday - 1; i >= 0; i--) {
      cells += `<div class="cal-cell other"><div class="cal-day">${prevMonthDays - i}</div></div>`;
    }

    for (let d = 1; d <= daysInMonth; d++) {
      const isToday = today.getFullYear() === year && today.getMonth() === month && today.getDate() === d;
      const evts = eventsByDay[d] || [];
      const pills = evts
        .map((ev) => `
          <button type="button" class="cal-evt ${ev.kind}" data-cal-sid="${esc(ev.source_id)}" title="${esc(ev.title)} ${esc(ev.money)}">
            <span class="cal-evt-name">${esc(ev.title)}</span>
            <span class="${ev.kind === "warranty" ? "" : "sensitive num"}">${esc(ev.money)}</span>
          </button>
        `)
        .join("");
      cells += `
        <div class="cal-cell ${isToday ? "today" : ""}">
          <div class="cal-day">${d}${isToday ? " · 今天" : ""}</div>
          ${pills}
        </div>
      `;
    }

    const trailing = (7 - ((startWeekday + daysInMonth) % 7)) % 7;
    for (let nextD = 1; nextD <= trailing; nextD++) {
      cells += `<div class="cal-cell other"><div class="cal-day">${nextD}</div></div>`;
    }

    pane.innerHTML = `
      <div class="cal-sheet">
        <div class="cal-top">
          <div class="cal-nav-group">
            <button type="button" id="cal-prev" class="btn btn-secondary btn-sm">上月</button>
            <h3 class="cal-title">${year} 年 ${month + 1} 月</h3>
            <button type="button" id="cal-next" class="btn btn-secondary btn-sm">下月</button>
            <button type="button" id="cal-today" class="btn btn-ghost btn-sm">本月</button>
            <span class="pill-tag pill-slate" style="margin-left:4px">本月 ${eventCount} 项日程</span>
          </div>
          <div class="cal-legend">
            <span><i class="legend-dot real"></i>真实账单日</span>
            <span><i class="legend-dot proj"></i>推算续费日</span>
            <span><i class="legend-dot war"></i>保修截止</span>
          </div>
        </div>
        <div class="cal-grid">${cells}</div>
      </div>
    `;

    $("cal-prev").addEventListener("click", () => {
      state.calendarMonth -= 1;
      if (state.calendarMonth < 0) { state.calendarMonth = 11; state.calendarYear -= 1; }
      renderCalendarPane(ws);
    });
    $("cal-next").addEventListener("click", () => {
      state.calendarMonth += 1;
      if (state.calendarMonth > 11) { state.calendarMonth = 0; state.calendarYear += 1; }
      renderCalendarPane(ws);
    });
    $("cal-today").addEventListener("click", () => {
      const n = new Date();
      state.calendarYear = n.getFullYear();
      state.calendarMonth = n.getMonth();
      renderCalendarPane(ws);
    });
    pane.querySelectorAll("[data-cal-sid]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const found = (ws.items || []).find((i) => i.source_id === btn.getAttribute("data-cal-sid"));
        if (found) openDrawer(found);
      });
    });
  }

  // --- View 3: Insights, Vendor Portfolio & What-If Savings Sandbox ---
  function buildMiniSparkline(history, strokeColor) {
    if (!history || history.length < 2) {
      return `<span style="font-size:11px;color:var(--ink-muted)">单期记录</span>`;
    }
    const vals = history.slice(-8).map((h) => h.cents || 0);
    const min = Math.min(...vals);
    const max = Math.max(...vals);
    const span = max - min || 1;
    const w = 64;
    const h = 22;
    const pts = vals
      .map((v, i) => {
        const x = (i / (vals.length - 1)) * w;
        const y = h - 3 - ((v - min) / span) * (h - 6);
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
    const last = pts.split(" ").pop().split(",");
    return `
      <svg width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" style="overflow:visible">
        <polyline fill="none" stroke="${strokeColor}" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" points="${pts}" />
        <circle cx="${last[0]}" cy="${last[1]}" r="2.8" fill="${strokeColor}" />
      </svg>
    `;
  }

  function buildFunEquivalence(cur, cents) {
    if (cents <= 0) return "拨动下方任意订阅开关，实时测算停掉后一年能省下多少预算";
    const val = cents / 100;
    if (cur === "CNY") {
      const coffees = Math.max(1, Math.round(val / 30));
      const hotpots = Math.round(val / 180);
      return hotpots >= 2
        ? `相当于省下约 ${coffees} 杯精品咖啡，或 ${hotpots} 顿双人火锅`
        : `相当于省下约 ${coffees} 杯精品咖啡`;
    }
    if (cur === "USD" || cur === "EUR" || cur === "GBP") {
      const coffees = Math.max(1, Math.round(val / 5));
      const games = Math.round(val / 60);
      return games >= 1
        ? `大约相当于 ${coffees} 杯咖啡，或 ${games} 款 3A 大作`
        : `大约相当于 ${coffees} 杯精品咖啡`;
    }
    return `一年可减少 ${formatMoney(cur, centsToText(cents))} 的周期固定支出`;
  }

  function renderInsightsPane(ws) {
    const pane = $("pane-insights");
    const curList = (ws.summary_by_currency || []).map((c) => c.currency);
    if (curList.length === 0) {
      pane.innerHTML = `
        <div class="empty-welcome">
          <h3>导入或录入几笔资产后，这里会自动绘出分析图表与省钱沙盘</h3>
          <p>按币种独立分析分类占比、月度节奏、商家长期累计花费与涨价轨迹，并支持模拟精简订阅。</p>
        </div>
      `;
      return;
    }

    if (!curList.includes(state.insightsCurrency)) state.insightsCurrency = curList[0];
    const cur = state.insightsCurrency;
    const analytics = ws.analytics || { categories: {}, accounts: {}, monthly_trend: {} };
    const catRows = (analytics.categories && analytics.categories[cur]) || [];
    const accRows = (analytics.accounts && analytics.accounts[cur]) || [];
    const monthRows = (analytics.monthly_trend && analytics.monthly_trend[cur]) || [];

    // Thin-ring Donut
    const totalCatCents = catRows.reduce((s, r) => s + (r.cents || 0), 0) || 1;
    const r = 68;
    const circ = 2 * Math.PI * r;
    let offset = 0;
    const slicesSvg = catRows
      .map((row, idx) => {
        const frac = row.cents / totalCatCents;
        const dash = Math.max(2, frac * circ);
        const color = CHART_PALETTE[idx % CHART_PALETTE.length];
        const seg = `<circle class="donut-seg" cx="88" cy="88" r="${r}" fill="none" stroke="${color}" stroke-width="16" stroke-dasharray="${dash.toFixed(2)} ${(circ - dash).toFixed(2)}" stroke-dashoffset="${(-offset).toFixed(2)}" transform="rotate(-90 88 88)" data-cat-slice="${esc(row.name)}" data-cat-amt="${esc(row.amount_text)}" data-cat-pct="${Math.round(frac * 100)}" />`;
        offset += dash;
        return seg;
      })
      .join("");

    const legendHtml = catRows
      .map((row, idx) => {
        const pct = Math.round((row.cents / totalCatCents) * 100);
        const color = CHART_PALETTE[idx % CHART_PALETTE.length];
        return `
          <div class="legend-item" data-jump-cat="${esc(row.name)}">
            <div class="legend-left">
              <span class="swatch" style="background:${color}"></span>
              <span>${esc(row.name)}</span>
            </div>
            <strong class="sensitive num">${esc(formatMoneyShort(cur, row.amount_text))} · ${pct}%</strong>
          </div>
        `;
      })
      .join("");

    // Monthly Bars + Account Allocation
    const maxMonth = Math.max(...monthRows.map((m) => m.cents || 0), 1);
    const barsHtml = monthRows.length === 0
      ? `<p class="muted-note" style="margin-top:12px">补充账单日期后会自动生成按月节奏图。</p>`
      : `<div class="bars-row">
          ${monthRows.slice(-10).map((m) => {
            const h = Math.max(8, Math.round((m.cents / maxMonth) * 106));
            return `
              <div class="bar-col">
                <span class="bar-num sensitive">${esc(formatMoneyShort(cur, m.amount_text))}</span>
                <div class="bar-fill" style="height:${h}px"></div>
                <span class="bar-lbl">${esc(m.month)}</span>
              </div>
            `;
          }).join("")}
        </div>`;

    const totalAcc = accRows.reduce((s, r) => s + (r.cents || 0), 0) || 1;
    const allocSegs = accRows
      .map((row, idx) => `<div class="alloc-seg" style="width:${Math.max(4, Math.round((row.cents / totalAcc) * 100))}%;background:${CHART_PALETTE[(idx + 1) % CHART_PALETTE.length]}"></div>`)
      .join("");
    const accLegend = accRows
      .map((row, idx) => `
        <div class="legend-item" data-jump-acc="${esc(row.name)}">
          <div class="legend-left">
            <span class="swatch" style="background:${CHART_PALETTE[(idx + 1) % CHART_PALETTE.length]}"></span>
            <span class="sensitive">${esc(row.name)}（${row.count} 笔）</span>
          </div>
          <strong class="sensitive num">${esc(formatMoneyShort(cur, row.amount_text))} · ${Math.round((row.cents / totalAcc) * 100)}%</strong>
        </div>
      `)
      .join("");

    // Vendor Asset Portfolio
    const portfolioRows = (ws.vendor_portfolio || []).filter((p) => p.currency === cur);
    const portfolioHtml = portfolioRows.length === 0
      ? ""
      : `
        <div class="surface-card pad-20">
          <div class="card-kicker">商家资产档案</div>
          <h3 class="card-title" style="margin-top:2px">累计投入与价格轨迹</h3>
          <p class="card-desc" style="margin-top:2px">点击任意商家卡片，直接筛选该商家的全部历史凭证</p>
          <div class="vendor-grid">
            ${portfolioRows.map((vp) => {
              const brand = brandStyleFor(vp.vendor);
              const kindMeta = ASSET_KIND_META[vp.asset_kind] || ASSET_KIND_META.general;
              const drift = vp.price_drift;
              const badge = drift
                ? `<span class="pill-tag ${drift.direction === "up" ? "pill-rose" : "pill-green"}">${drift.direction === "up" ? ICO.trendUp : ICO.trendDown} ${drift.direction === "up" ? `+${Math.abs(drift.pct_change)}%` : `-${Math.abs(drift.pct_change)}%`}</span>`
                : `<span class="pill-tag ${kindMeta.pillClass}">${esc(kindMeta.label)}</span>`;
              const spark = buildMiniSparkline(vp.history || [], drift?.direction === "up" ? "#dc2626" : "#0f172a");
              const accStr = (vp.accounts || []).length ? ` · ${vp.accounts.join(" / ")}` : "";
              return `
                <div class="vendor-card" data-jump-vendor="${esc(vp.vendor)}">
                  <div class="vc-head">
                    <div class="vc-id">
                      <div class="monogram monogram-sm" style="background:${brand.bg}">${esc(brand.mark)}</div>
                      <div style="min-width:0">
                        <div class="vc-name">${esc(vp.vendor)}</div>
                        <div class="vc-sub sensitive">${vp.bill_count} 笔${esc(accStr)}</div>
                      </div>
                    </div>
                    ${badge}
                  </div>
                  <div class="vc-foot">
                    <div>
                      <div style="font-size:11px;color:var(--ink-muted)">累计投入</div>
                      <div class="vc-ltv sensitive">${renderEditorialMoney(cur, vp.total_text, false)}</div>
                    </div>
                    ${spark}
                  </div>
                </div>
              `;
            }).join("")}
          </div>
        </div>
      `;

    // What-If Simulator
    const recurring = (ws.items || []).filter((item) => {
      const amt = item.amount_fact?.value;
      const cyc = item.annotation?.billing_cycle;
      return amt && amt.currency === cur && ["monthly", "yearly", "quarterly", "weekly"].includes(cyc);
    });
    let yearlySave = 0;
    const cutList = [];
    const simRows = recurring
      .map((item) => {
        const ann = item.annotation || {};
        const amt = item.amount_fact.value;
        const mult = { monthly: 12, yearly: 1, quarterly: 4, weekly: 52 }[ann.billing_cycle] || 0;
        const yrCents = amountToCents(amt.amount_text) * mult;
        const isCut = state.simCutIds.has(item.source_id);
        if (isCut) {
          yearlySave += yrCents;
          cutList.push(`${ann.vendor || item.display_name}（${formatMoney(cur, amt.amount_text)} / ${CYCLE_LABELS[ann.billing_cycle]}）`);
        }
        return `
          <div class="sim-row ${isCut ? "cut" : ""}">
            <div style="min-width:0">
              <div class="sim-title" style="font-weight:600;font-size:13.5px">${esc(ann.vendor || item.display_name)}</div>
              <div style="font-size:12px;color:var(--ink-muted)" class="sensitive num">${esc(formatMoneyShort(cur, amt.amount_text))} / ${esc(CYCLE_LABELS[ann.billing_cycle])} · 年合 ${esc(formatMoneyShort(cur, centsToText(yrCents)))}</div>
            </div>
            <label class="ios-switch" title="模拟取消此订阅">
              <input type="checkbox" data-sim-sid="${esc(item.source_id)}" ${isCut ? "checked" : ""}>
              <span class="ios-slider"></span>
            </label>
          </div>
        `;
      })
      .join("");

    const curSegs = curList
      .map((c) => `<button type="button" class="seg-tab ${c === cur ? "active" : ""}" data-ins-cur="${esc(c)}">${esc(c)} · ${esc(CURRENCY_NAMES[c] || c)}</button>`)
      .join("");

    pane.innerHTML = `
      <div class="insights-stack">
        <div class="insights-bar">
          <div class="segmented-bar compact" style="max-width:420px">${curSegs}</div>
          <span class="subbar-caption">按原件币种独立分析，绝不做虚假汇率折算</span>
        </div>

        <div class="bento-2">
          <div class="surface-card pad-20">
            <div class="card-kicker">分类构成</div>
            <h3 class="card-title" style="margin-top:2px">花费占比（点击扇区筛选）</h3>
            <div class="donut-wrap">
              <div class="donut-box">
                <svg viewBox="0 0 176 176" width="176" height="176">${slicesSvg}</svg>
                <div class="donut-center">
                  <div id="donut-center-label" class="donut-center-k">${esc(cur)} 总额</div>
                  <div id="donut-center-val" class="donut-center-v sensitive">${esc(formatMoneyShort(cur, centsToText(totalCatCents)))}</div>
                </div>
              </div>
              <div class="legend-col">${legendHtml}</div>
            </div>
          </div>

          <div class="surface-card pad-20">
            <div class="card-kicker">节奏与分账</div>
            <h3 class="card-title" style="margin-top:2px">月度账单与账号分布</h3>
            ${barsHtml}
            <div class="alloc-bar">${allocSegs}</div>
            <div class="legend-col">${accLegend}</div>
          </div>
        </div>

        ${portfolioHtml}

        <div class="sim-sheet">
          <div class="sim-head">
            <div>
              <div style="display:flex;align-items:center;gap:8px">
                <span class="card-kicker">退订省钱沙盘</span>
                <span class="pill-tag pill-slate">纯模拟 · 不改真实账本</span>
              </div>
              <div class="sim-big-num sensitive">每年可省 ${renderEditorialMoney(cur, centsToText(yearlySave), true)}</div>
              <div class="sim-sub">${esc(buildFunEquivalence(cur, yearlySave))}</div>
            </div>
            <div class="btn-row">
              <button type="button" id="btn-sim-copy" class="btn btn-primary btn-sm" ${cutList.length === 0 ? "disabled" : ""}>复制精简清单 (${cutList.length})</button>
              <button type="button" id="btn-sim-reset" class="btn btn-secondary btn-sm" ${cutList.length === 0 ? "disabled" : ""}>重置</button>
            </div>
          </div>
          ${recurring.length > 0 ? `<div class="sim-grid">${simRows}</div>` : `<p class="muted-note" style="margin:0">当前 ${esc(cur)} 下暂无周期订阅，标记月付或年付即可开启省钱模拟。</p>`}
        </div>
      </div>
    `;

    pane.querySelectorAll("[data-ins-cur]").forEach((btn) => {
      btn.addEventListener("click", () => {
        state.insightsCurrency = btn.getAttribute("data-ins-cur");
        renderInsightsPane(ws);
      });
    });
    pane.querySelectorAll("[data-cat-slice]").forEach((circle) => {
      circle.addEventListener("mouseenter", () => {
        $("donut-center-label").textContent = `${circle.getAttribute("data-cat-slice")} · ${circle.getAttribute("data-cat-pct")}%`;
        $("donut-center-val").textContent = formatMoneyShort(cur, circle.getAttribute("data-cat-amt"));
      });
      circle.addEventListener("mouseleave", () => {
        $("donut-center-label").textContent = `${cur} 总额`;
        $("donut-center-val").textContent = formatMoneyShort(cur, centsToText(totalCatCents));
      });
      circle.addEventListener("click", () => {
        state.categoryFilter = circle.getAttribute("data-cat-slice");
        state.currencyFilter = cur;
        navigateTo("assets", "all");
      });
    });
    pane.querySelectorAll("[data-jump-cat]").forEach((row) => {
      row.addEventListener("click", () => {
        state.categoryFilter = row.getAttribute("data-jump-cat");
        state.currencyFilter = cur;
        navigateTo("assets", "all");
      });
    });
    pane.querySelectorAll("[data-jump-acc]").forEach((row) => {
      row.addEventListener("click", () => {
        const acc = row.getAttribute("data-jump-acc");
        state.accountFilter = acc === "未分组" ? "" : acc;
        navigateTo("assets", "all");
      });
    });
    pane.querySelectorAll("[data-jump-vendor]").forEach((tile) => {
      tile.addEventListener("click", () => {
        const v = tile.getAttribute("data-jump-vendor") || "";
        state.searchQuery = v;
        $("search-box").value = v;
        navigateTo("assets", "all");
      });
    });
    pane.querySelectorAll("[data-sim-sid]").forEach((chk) => {
      chk.addEventListener("change", () => {
        const sid = chk.getAttribute("data-sim-sid");
        if (chk.checked) state.simCutIds.add(sid);
        else state.simCutIds.delete(sid);
        renderInsightsPane(ws);
      });
    });
    if ($("btn-sim-reset")) {
      $("btn-sim-reset").addEventListener("click", () => {
        state.simCutIds.clear();
        renderInsightsPane(ws);
      });
    }
    if ($("btn-sim-copy")) {
      $("btn-sim-copy").addEventListener("click", async () => {
        const text = `【SubVault 待精简订阅清单（${cur}）】\n预计每年可省：${formatMoney(cur, centsToText(yearlySave))}\n` +
          cutList.map((n, i) => `${i + 1}. ${n}`).join("\n");
        try {
          await navigator.clipboard.writeText(text);
          showToast("待精简清单已复制", "ok");
        } catch (_) {
          downloadTextFile("subvault-cancel-plan.txt", text);
        }
      });
    }
  }

  // --- View 4: Quick Triage ---
  function getTriageQueue(ws) {
    return (ws.items || []).filter(
      (i) => (i.status_flags.needs_manual_entry || i.status_flags.has_conflict) && !state.triageSkippedIds.has(i.source_id)
    );
  }

  function renderTriagePane(ws) {
    const pane = $("pane-triage");
    const queue = getTriageQueue(ws);
    if (queue.length === 0) {
      const skipped = state.triageSkippedIds.size;
      pane.innerHTML = `
        <div class="empty-welcome">
          <div class="empty-icon-wrap" style="background:var(--green-soft);color:var(--green-ink)">${ICO.sparkle}</div>
          <h3>所有待补录与冲突账单都已处理干净</h3>
          <p>下次拖入扫描件发票、收据截图或出现多金额候选的文件时，可以随时来这里左右对照极速登记。</p>
          <div class="btn-row" style="justify-content:center">
            ${skipped > 0
              ? `<button type="button" id="btn-reset-skip" class="btn btn-secondary btn-sm">重新查看跳过的 ${skipped} 项</button>`
              : `<button type="button" id="btn-back-assets" class="btn btn-primary btn-sm">回到全部资产</button>`}
          </div>
        </div>
      `;
      if ($("btn-reset-skip")) {
        $("btn-reset-skip").addEventListener("click", () => {
          state.triageSkippedIds.clear();
          renderTriagePane(ws);
        });
      }
      if ($("btn-back-assets")) {
        $("btn-back-assets").addEventListener("click", () => navigateTo("assets", "all"));
      }
      return;
    }

    const item = queue[0];
    const ann = item.annotation || {};
    const rawUrl = tokenUrl(`/api/sources/${encodeURIComponent(item.source_id)}/raw`);
    let previewHtml = "";
    if (item.media_type === "application/pdf") {
      previewHtml = `<iframe src="${esc(rawUrl)}" title="PDF 原件"></iframe>`;
    } else if (item.media_type.startsWith("image/")) {
      previewHtml = `<img src="${esc(rawUrl)}" alt="${esc(item.display_name)}">`;
    } else if (item.media_type.startsWith("text/html")) {
      previewHtml = `<iframe sandbox="" src="${esc(rawUrl)}" title="静态 HTML"></iframe>`;
    } else {
      previewHtml = `<pre id="triage-text-preview" class="code-sheet" style="margin:0">加载原件文本中…</pre>`;
    }

    const curs = (state.bootstrap?.currencies || ["CNY", "USD", "EUR", "GBP", "JPY", "HKD"])
      .map((c) => `<option value="${esc(c)}">${esc(c)} ${esc(CURRENCY_SYMBOLS[c] || "")}</option>`)
      .join("");

    pane.innerHTML = `
      <div class="surface-card triage-bar">
        <div style="display:flex;align-items:center;gap:10px">
          <span class="pill-tag pill-amber">剩余 ${queue.length} 份</span>
          <span style="font-weight:600">${esc(item.display_name)}</span>
          <span style="font-size:12px;color:var(--ink-muted)">填完按 Enter 自动切下一张</span>
        </div>
        <button type="button" id="btn-triage-skip" class="btn btn-ghost btn-sm">暂时跳过</button>
      </div>
      <div class="triage-split">
        <div class="surface-card pad-20">
          <div class="card-kicker" style="margin-bottom:10px">原始凭证对照 · ${formatBytes(item.byte_length)}</div>
          <div class="preview-frame" style="margin-bottom:0">${previewHtml}</div>
        </div>
        <div class="surface-card pad-20">
          <h3 class="card-title" style="margin-bottom:14px">快速补录信息</h3>
          ${item.status_flags.has_conflict ? `<div class="warning-note" style="margin-bottom:12px">检测到多个金额候选，请对照左侧原件填入真实金额。</div>` : ""}
          <form id="form-triage" class="form-stack">
            <label class="field">
              <span>商家 / 服务名</span>
              <input id="tr-vendor" type="text" value="${esc(ann.vendor || "")}" placeholder="如 JetBrains、Apple、电信宽带" required>
            </label>
            <div class="form-row-2">
              <label class="field">
                <span>币种</span>
                <select id="tr-currency">${curs}</select>
              </label>
              <label class="field">
                <span>金额</span>
                <input id="tr-amount" type="text" placeholder="如 98.00" required>
              </label>
            </div>
            <div class="chip-line">
              <span class="chip-line-label">周期</span>
              ${["monthly", "yearly", "quarterly", "one_time"]
                .map((c) => `<button type="button" class="choice-pill ${ann.billing_cycle === c ? "active" : ""}" data-tr-cycle="${c}">${CYCLE_LABELS[c]}</button>`)
                .join("")}
            </div>
            <div class="chip-line">
              <span class="chip-line-label">账号</span>
              ${["个人", "公司工作", "家庭共享"]
                .map((a) => `<button type="button" class="choice-pill ${ann.account_label === a ? "active" : ""}" data-tr-acc="${a}">${a}</button>`)
                .join("")}
            </div>
            <div class="chip-line">
              <span class="chip-line-label">分类</span>
              ${["AI 工具", "数码硬件", "云存储", "开发工具", "办公效率"]
                .map((k) => `<button type="button" class="choice-pill ${ann.category === k ? "active" : ""}" data-tr-cat="${k}">${k}</button>`)
                .join("")}
            </div>
            <div class="form-row-2">
              <label class="field">
                <span>归属账号</span>
                <input id="tr-account" type="text" value="${esc(ann.account_label || "个人")}">
              </label>
              <label class="field">
                <span>分类</span>
                <input id="tr-category" type="text" value="${esc(ann.category || "")}">
              </label>
            </div>
            <label class="field">
              <span>账单日期（可选）</span>
              <input id="tr-date" type="date" value="${esc(ann.bill_date || "")}">
            </label>
            <button id="btn-triage-save" class="btn btn-primary btn-lg btn-block" type="submit">保存并进入下一张</button>
          </form>
        </div>
      </div>
    `;

    if ($("triage-text-preview")) {
      api("GET", `/api/sources/${encodeURIComponent(item.source_id)}/text`)
        .then((res) => { if ($("triage-text-preview")) $("triage-text-preview").textContent = res.text || ""; })
        .catch(() => {});
    }

    let chosenCycle = ann.billing_cycle || "monthly";
    pane.querySelectorAll("[data-tr-cycle]").forEach((b) => {
      b.addEventListener("click", () => {
        chosenCycle = b.getAttribute("data-tr-cycle");
        pane.querySelectorAll("[data-tr-cycle]").forEach((x) => x.classList.toggle("active", x === b));
      });
    });
    pane.querySelectorAll("[data-tr-acc]").forEach((b) => {
      b.addEventListener("click", () => {
        $("tr-account").value = b.getAttribute("data-tr-acc");
        pane.querySelectorAll("[data-tr-acc]").forEach((x) => x.classList.toggle("active", x === b));
      });
    });
    pane.querySelectorAll("[data-tr-cat]").forEach((b) => {
      b.addEventListener("click", () => {
        $("tr-category").value = b.getAttribute("data-tr-cat");
        pane.querySelectorAll("[data-tr-cat]").forEach((x) => x.classList.toggle("active", x === b));
      });
    });
    $("btn-triage-skip").addEventListener("click", () => {
      state.triageSkippedIds.add(item.source_id);
      renderTriagePane(ws);
    });
    $("form-triage").addEventListener("submit", async (e) => {
      e.preventDefault();
      const btn = $("btn-triage-save");
      btn.disabled = true;
      btn.textContent = "保存中…";
      try {
        const snap = await api("POST", "/api/sources/save", {
          source_id: item.source_id,
          vendor: $("tr-vendor").value,
          account_label: $("tr-account").value,
          billing_cycle: chosenCycle,
          currency: $("tr-currency").value,
          amount_text: $("tr-amount").value,
          category: $("tr-category").value,
          bill_date: $("tr-date").value,
        });
        state.workspace = snap;
        renderWorkspace();
        if (getTriageQueue(snap).length === 0) {
          launchConfetti();
          showToast("全部待整理账单已清零", "ok");
        } else {
          showToast("已保存，切入下一张", "ok");
        }
      } catch (err) {
        showToast(err.message, "error");
        btn.disabled = false;
        btn.textContent = "保存并进入下一张";
      }
    });
    $("tr-vendor").focus();
  }

  // --- View 5: Security & Dedup / Conflicts ---
  function renderSecurityAttention(ws) {
    const wrap = $("security-attention-wrap");
    const items = ws.items || [];
    const needs = items.filter(
      (i) => i.status_flags.needs_manual_entry || i.status_flags.has_conflict || i.status_flags.is_credit_or_order_warning
    );
    const dups = items.filter((i) => i.status_flags.has_duplicate);

    const needsHtml = needs.length === 0
      ? `<p class="muted-note" style="margin:0">目前没有待补录或存在金额冲突的文件。</p>`
      : `<div class="stack-8">${needs.map((item) => {
          const reason = item.status_flags.has_conflict
            ? "同一文件检测到多个候选金额，等待你核对确认"
            : item.status_flags.is_credit_or_order_warning
              ? "疑似退款单或未支付订单，未计入花费"
              : "未提取到文字金额（扫描件或截图），点击补录";
          return `
            <div class="attn-row">
              <div>
                <div style="font-weight:600">${esc(item.annotation?.vendor || item.display_name)}</div>
                <div style="font-size:12px;color:var(--ink-muted)">${esc(reason)}</div>
              </div>
              <button type="button" class="btn btn-secondary btn-sm" data-open-sid="${esc(item.source_id)}">核对</button>
            </div>
          `;
        }).join("")}</div>`;

    const dupsHtml = dups.length === 0
      ? `<p class="muted-note" style="margin:0">暂未遇到重复导入的文件；以后即便改名拖入同一份发票，系统也会按原件指纹自动防重。</p>`
      : `<div class="stack-8">${dups.map((item) => {
          const names = (item.occurrences || []).map((o) => o.display_name).join("、");
          return `
            <div class="attn-row">
              <div>
                <div style="font-weight:600">${esc(item.annotation?.vendor || item.display_name)}</div>
                <div style="font-size:12px;color:var(--ink-muted)">共导入 ${item.occurrence_count} 次，拦截重复 ${item.duplicate_hits} 次（金额仅算一次）· ${esc(names)}</div>
              </div>
              <button type="button" class="btn btn-secondary btn-sm" data-open-sid="${esc(item.source_id)}">查看</button>
            </div>
          `;
        }).join("")}</div>`;

    wrap.innerHTML = `
      <div class="bento-2">
        <div class="surface-card pad-20 stack-12">
          <div style="display:flex;align-items:center;justify-content:space-between">
            <h3 class="card-title">待补录与冲突核对</h3>
            <span class="pill-tag ${needs.length ? "pill-amber" : "pill-slate"}">${needs.length}</span>
          </div>
          ${needsHtml}
        </div>
        <div class="surface-card pad-20 stack-12">
          <div style="display:flex;align-items:center;justify-content:space-between">
            <h3 class="card-title">已自动拦截的重复账单</h3>
            <span class="pill-tag ${dups.length ? "pill-green" : "pill-slate"}">${dups.length}</span>
          </div>
          ${dupsHtml}
        </div>
      </div>
    `;
    wrap.querySelectorAll("[data-open-sid]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const found = items.find((i) => i.source_id === btn.getAttribute("data-open-sid"));
        if (found) openDrawer(found);
      });
    });
  }

  // --- Detail Inspector Drawer ---
  function openDrawer(item, defaultTab) {
    state.selectedSourceId = item.source_id;
    state.textViewActive = false;
    state.lastRedactedText = "";
    $("redact-result-wrap").classList.add("hidden");
    $("redact-custom-terms").value = "";
    $("delete-confirm-bar").classList.add("hidden");
    $("btn-ask-delete-item").classList.remove("hidden");
    $("drawer-backdrop").classList.remove("hidden");
    $("detail-drawer").classList.remove("hidden");

    const isCsv = item.status_flags && item.status_flags.is_csv_statement;
    $("dtab-csv-btn").classList.toggle("hidden", !isCsv);
    renderDrawer(item);
    switchDrawerTab(defaultTab || (isCsv ? "csv" : "edit"));
  }

  function closeDrawer() {
    state.selectedSourceId = null;
    $("drawer-backdrop").classList.add("hidden");
    $("detail-drawer").classList.add("hidden");
  }

  function switchDrawerTab(tab) {
    state.activeDrawerTab = tab;
    document.querySelectorAll(".dtab").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-dtab") === tab);
    });
    ["edit", "evidence", "csv", "history", "redact"].forEach((name) => {
      $(`dpane-${name}`).classList.toggle("hidden", name !== tab);
    });
    if (tab === "evidence" && state.selectedSourceId) loadEvidencePreview(state.selectedSourceId);
  }

  function setEditAssetKind(kind, userClicked) {
    state.editKind = kind || "general";
    document.querySelectorAll("[data-ed-kind]").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-ed-kind") === state.editKind);
    });
    const curReimb = $("ed-reimburse-status").value || "none";
    $("ed-panel-license").classList.toggle("hidden", state.editKind !== "license");
    $("ed-panel-hardware").classList.toggle("hidden", state.editKind !== "hardware");
    $("ed-panel-reimb").classList.toggle("hidden", state.editKind !== "reimbursable" && curReimb === "none");

    const hasPeriod = Boolean($("ed-period-start").value || $("ed-period-end").value);
    $("ed-period-row").classList.toggle(
      "hidden",
      (state.editKind === "hardware" || state.editKind === "license") && !hasPeriod
    );

    if (userClicked) {
      if (state.editKind === "hardware" || state.editKind === "license") $("ed-cycle").value = "one_time";
      else if (state.editKind === "subscription" && ["unknown", "one_time"].includes($("ed-cycle").value)) $("ed-cycle").value = "monthly";
      else if (state.editKind === "reimbursable" && $("ed-reimburse-status").value === "none") $("ed-reimburse-status").value = "pending";
      updateEditHardwareMetrics();
    }
  }

  function updateEditHardwareMetrics() {
    const box = $("ed-hardware-metrics");
    if (!box) return;
    if (state.editKind !== "hardware") {
      box.classList.add("hidden");
      return;
    }
    const buy = $("ed-bill-date").value;
    const wend = $("ed-warranty-end").value;
    const cur = $("ed-currency").value || "CNY";
    const amt = $("ed-amount").value;
    const pills = [];
    const w = warrantyInfo(buy, wend);
    if (w) pills.push(`<span class="pill-tag ${w.level === "amber" ? "pill-amber" : w.level === "slate" ? "pill-slate" : "pill-iris"}">${esc(w.statusLabel)}</span>`);
    const cpd = costPerDayInfo(buy, cur, amt);
    if (cpd) pills.push(`<span class="cpd-chip sensitive">已陪伴 ${cpd.daysUsed} 天 · 日均 ${esc(cpd.perDayText)}</span>`);
    if (pills.length) {
      box.innerHTML = pills.join("");
      box.classList.remove("hidden");
    } else {
      box.classList.add("hidden");
    }
  }

  function renderDrawer(item) {
    const ann = item.annotation || {};
    const amtVal = item.amount_fact ? item.amount_fact.value : null;
    const perVal = item.period_fact ? item.period_fact.value : null;
    const title = ann.vendor || item.display_name;
    const brand = brandStyleFor(title);

    $("drawer-avatar").textContent = brand.mark;
    $("drawer-avatar").style.background = brand.bg;
    $("drawer-title").textContent = title;
    $("drawer-subtitle").textContent = `${item.display_name} · ${formatBytes(item.byte_length)}`;
    $("drawer-stamp").textContent = item.object_intact ? "原件完整" : "校验异常";
    $("drawer-stamp").className = `pill-tag ${item.object_intact ? "pill-green" : "pill-rose"}`;

    const warns = [];
    if (item.status_flags.has_conflict) warns.push("该文件包含多个不同的金额候选，系统未自动采纳，请核对原件后在下方保存。");
    if (item.status_flags.is_credit_or_order_warning) warns.push("疑似退款单或未支付订单，系统未计入花费统计。");
    if (item.status_flags.needs_manual_entry) warns.push("尚未记录金额（常见于扫描件或截图），原件已安全加密存档，补填信息即可。");
    $("drawer-warnings").innerHTML = warns.map((w) => `<div class="warning-note">${esc(w)}</div>`).join("");

    $("ed-vendor").value = ann.vendor || "";
    $("ed-plan").value = ann.plan_name || "";
    $("ed-account").value = ann.account_label || "";
    $("ed-cycle").value = ann.billing_cycle || "unknown";
    $("ed-currency").value = (amtVal && amtVal.currency) || "CNY";
    $("ed-amount").value = (amtVal && amtVal.amount_text) || "";
    $("ed-category").value = ann.category || "";
    $("ed-bill-date").value = ann.bill_date || "";
    $("ed-period-start").value = (perVal && perVal.period_start) || "";
    $("ed-period-end").value = (perVal && perVal.period_end) || "";
    $("ed-notes").value = ann.notes || "";
    $("ed-warranty-end").value = ann.warranty_end || "";
    $("ed-license-key").value = ann.license_key || "";
    $("ed-license-key").type = "password";
    $("btn-ed-toggle-key").textContent = "显示";
    $("ed-license-scope").value = ann.license_scope || "";
    $("ed-reimburse-status").value = ann.reimburse_status || (ann.asset_kind === "reimbursable" ? "pending" : "none");
    setEditAssetKind(ann.asset_kind || "general", false);
    updateEditHardwareMetrics();

    $("evidence-meta").innerHTML = `
      <div><strong>文件名：</strong>${esc(item.display_name)}（${formatBytes(item.byte_length)}）</div>
      <div><strong>原件指纹：</strong><code>${esc(item.content_sha256)}</code></div>
      <div><strong>完整性：</strong>${item.object_intact ? "已实时核验一致，原件零改动" : "校验异常，请检查账本"}</div>
    `;
    $("btn-download-raw").href = tokenUrl(`/api/sources/${encodeURIComponent(item.source_id)}/raw?download=1`);

    $("evidence-occurrences").innerHTML = `
      <div class="section-eyebrow" style="margin:6px 0 4px">导入记录（共 ${item.occurrence_count} 次）</div>
      ${(item.occurrences || []).map((o) => `
        <div class="occ-row">
          <strong>${esc(o.display_name)}</strong> · ${formatTime(o.recorded_at)}
          <span class="pill-tag ${o.storage_reused ? "pill-slate" : "pill-green"}" style="margin-left:6px">${o.storage_reused ? "重复自动复用" : "首次入库"}</span>
        </div>
      `).join("")}
    `;

    if (item.status_flags.is_csv_statement) renderCsvRows(item);
    renderHistoryTimeline(item);
  }

  async function loadEvidencePreview(sourceId) {
    const item = (state.workspace?.items || []).find((i) => i.source_id === sourceId);
    if (!item) return;
    const box = $("evidence-preview-box");
    const textBox = $("evidence-text-box");
    const rawUrl = tokenUrl(`/api/sources/${encodeURIComponent(sourceId)}/raw`);

    if (item.media_type === "application/pdf") {
      box.innerHTML = `<iframe src="${esc(rawUrl)}" title="PDF 原件"></iframe>`;
      box.classList.remove("hidden");
    } else if (item.media_type.startsWith("image/")) {
      box.innerHTML = `<img src="${esc(rawUrl)}" alt="${esc(item.display_name)}">`;
      box.classList.remove("hidden");
    } else if (item.media_type.startsWith("text/html")) {
      box.innerHTML = `<iframe sandbox="" src="${esc(rawUrl)}" title="静态 HTML 原件"></iframe>`;
      box.classList.remove("hidden");
    } else {
      box.classList.add("hidden");
      state.textViewActive = true;
    }

    textBox.classList.toggle("hidden", !state.textViewActive);
    try {
      const res = await api("GET", `/api/sources/${encodeURIComponent(sourceId)}/text`);
      textBox.textContent = res.text || "（无纯文本预览）";
    } catch (err) {
      textBox.textContent = `文本加载失败：${err.message}`;
    }
  }

  function renderCsvRows(item) {
    const container = $("csv-rows-container");
    const q = ($("csv-search").value || "").trim().toLowerCase();
    const expenseOnly = $("chk-csv-expense-only").checked;
    const rows = (item.candidates || [])
      .filter((c) => c.kind === "csv_transaction")
      .map((c) => ({ ...c.value, raw_line: c.excerpt }))
      .filter((r) => {
        if (expenseOnly && r.direction !== "expense") return false;
        if (q) {
          const hay = `${r.merchant || ""} ${r.description || ""} ${r.date || ""} ${r.amount_text || ""}`.toLowerCase();
          if (!hay.includes(q)) return false;
        }
        return true;
      });

    if (rows.length === 0) {
      container.innerHTML = `<p class="muted-note">没有符合条件的流水记录。</p>`;
      return;
    }

    container.innerHTML = rows.slice(0, 150).map((r, idx) => {
      const dirTag = r.direction === "expense"
        ? `<span class="pill-tag pill-slate">支出</span>`
        : r.direction === "refund"
          ? `<span class="pill-tag pill-amber">退款</span>`
          : `<span class="pill-tag pill-green">收入</span>`;
      return `
        <div class="csv-row">
          <div>
            <strong>${esc(r.merchant || "未标注商户")} · <span class="sensitive num">${esc(formatMoney(r.currency || "CNY", r.amount_text))}</span></strong>
            <small>${esc(r.date || "")} · ${esc(r.description || "")} ${dirTag}</small>
          </div>
          <button type="button" class="btn btn-secondary btn-sm" data-csv-promote="${idx}">登记为资产</button>
        </div>
      `;
    }).join("");

    container.querySelectorAll("[data-csv-promote]").forEach((btn) => {
      btn.addEventListener("click", async () => {
        const r = rows[Number(btn.getAttribute("data-csv-promote"))];
        if (!r) return;
        btn.disabled = true;
        btn.textContent = "登记中…";
        try {
          const res = await api("POST", "/api/csv/promote", {
            parent_source_id: item.source_id,
            row_number: r.row_number,
            merchant: r.merchant || "",
            description: r.description || "",
            date: r.date || "",
            currency: r.currency || "CNY",
            amount_text: r.amount_text || "",
            billing_cycle: "unknown",
            account_label: r.account_hint || undefined,
            raw_line: r.raw_line || "",
          });
          showToast(res.storage_reused ? "这条流水之前已登记过，已自动去重" : `已将「${r.merchant || "该笔流水"}」收入资产库`, "ok");
          await refreshWorkspace();
        } catch (err) {
          showToast(err.message, "error");
        } finally {
          btn.disabled = false;
          btn.textContent = "登记为资产";
        }
      });
    });
  }

  function renderHistoryTimeline(item) {
    const el = $("history-timeline");
    const blocks = [];
    for (const fact of [item.amount_fact, item.period_fact].filter(Boolean)) {
      const decisions = fact.decisions || [];
      const factLabel = fact.fact_type === "stated_amount_paid" ? "金额" : "服务周期";
      decisions.forEach((dec, idx) => {
        const isLatest = idx === decisions.length - 1;
        const kindLabel = {
          rule_adopt: "自动提取",
          human_record: "手动补录",
          human_correct: "人工修正",
          undo: "撤销变更",
        }[dec.kind] || dec.kind;
        const val = dec.new_value || {};
        const valText = fact.fact_type === "stated_amount_paid"
          ? (val.amount_text ? formatMoney(val.currency || "USD", val.amount_text) : "已清空")
          : (val.period_start ? `${val.period_start} — ${val.period_end}` : "已清空");
        const canUndo = isLatest && dec.kind !== "rule_adopt";
        blocks.push(`
          <div class="timeline-row">
            <div>
              <div><strong>${esc(factLabel)} · ${esc(kindLabel)}</strong> → <span class="sensitive num">${esc(valText)}</span></div>
              <div style="font-size:11.5px;color:var(--ink-muted)">版本 #${idx + 1} · ${formatTime(dec.recorded_at)}</div>
            </div>
            ${canUndo ? `<button type="button" class="btn btn-secondary btn-sm" data-undo-dec="${esc(dec.decision_id)}" data-undo-rev="${fact.revision}">撤销</button>` : ""}
          </div>
        `);
      });
    }
    el.innerHTML = blocks.length ? blocks.join("") : `<p class="muted-note">暂无金额或周期修订记录。</p>`;
    el.querySelectorAll("[data-undo-dec]").forEach((btn) => {
      btn.addEventListener("click", async () => {
        btn.disabled = true;
        try {
          const snap = await api("POST", "/api/facts/undo", {
            decision_id: btn.getAttribute("data-undo-dec"),
            expected_revision: Number(btn.getAttribute("data-undo-rev")),
          });
          state.workspace = snap;
          renderWorkspace();
          const refreshed = (snap.items || []).find((i) => i.source_id === state.selectedSourceId);
          if (refreshed) renderDrawer(refreshed);
          showToast("已撤销上一步修改", "ok");
        } catch (err) {
          showToast(err.message, "error");
        }
      });
    });
  }

  // --- Spotlight Command Palette (Ctrl+K) ---
  function openCmdPalette() {
    if (!state.workspace) return;
    state.cmdOpen = true;
    state.cmdIndex = 0;
    $("modal-cmd").classList.remove("hidden");
    $("cmd-input").value = "";
    renderCmdList("");
    $("cmd-input").focus();
  }

  function closeCmdPalette() {
    state.cmdOpen = false;
    $("modal-cmd").classList.add("hidden");
  }

  function renderCmdList(query) {
    const q = query.trim().toLowerCase();
    const baseActions = [
      { ico: ICO.plus, label: "手动记一笔资产或账单", sub: "订阅 / 密钥 / 保修 / 报销", run: () => openManualModal() },
      { ico: ICO.download, label: "导入本地账单文件", sub: "PDF / HTML / CSV / 截图", run: () => $("file-picker-input").click() },
      { ico: ICO.key, label: "查看买断授权与密钥库", sub: "一键复制加密激活码", run: () => navigateTo("assets", "license") },
      { ico: ICO.laptop, label: "查看硬件保修与日均成本", sub: "保修倒计时 + 每日成本", run: () => navigateTo("assets", "hardware") },
      { ico: ICO.receipt, label: "查看报销凭证与进度", sub: "一键切换待报销状态", run: () => navigateTo("assets", "reimbursable") },
      { ico: ICO.calendar, label: "打开续费与保修日历", sub: "按月查看账单与到期日", run: () => navigateTo("calendar") },
      { ico: ICO.chart, label: "打开洞察图表与省钱沙盘", sub: "分类占比 / 商家档案 / 模拟器", run: () => navigateTo("insights") },
      { ico: ICO.bolt, label: "进入极速整理工作台", sub: "左右分屏补录扫描件", run: () => navigateTo("triage") },
      { ico: ICO.sun, label: "切换界面外观主题", sub: "纯净白昼 / 暖玉极简 / 静谧深色", run: () => cycleTheme() },
      { ico: ICO.eye, label: "开关隐私防窥打码", sub: "快捷键 P", run: () => togglePrivacy() },
      { ico: ICO.lock, label: "立即锁定当前账本", sub: "清空内存密钥", run: () => $("btn-lock-vault").click() },
    ];

    const assetActions = (state.workspace?.items || []).map((item) => {
      const ann = item.annotation || {};
      const amt = item.amount_fact?.value;
      return {
        ico: ICO.receipt,
        label: ann.vendor || item.display_name,
        sub: [ann.plan_name, amt ? formatMoney(amt.currency, amt.amount_text) : "待补录", ann.account_label].filter(Boolean).join(" · "),
        run: () => openDrawer(item),
      };
    });

    const filtered = [...baseActions, ...assetActions].filter(
      (a) => !q || a.label.toLowerCase().includes(q) || a.sub.toLowerCase().includes(q)
    );
    state.cmdActions = filtered.slice(0, 12);
    state.cmdIndex = Math.min(state.cmdIndex, Math.max(0, state.cmdActions.length - 1));

    const el = $("cmd-list");
    if (!state.cmdActions.length) {
      el.innerHTML = `<div style="padding:20px;text-align:center;color:var(--ink-muted)">没有匹配的资产或指令</div>`;
      return;
    }
    el.innerHTML = state.cmdActions.map((act, idx) => `
      <button type="button" class="cmd-item ${idx === state.cmdIndex ? "selected" : ""}" data-cmd-idx="${idx}">
        <span class="cmd-item-left"><span style="color:var(--ink-muted);display:inline-flex">${act.ico}</span><span>${esc(act.label)}</span></span>
        <span class="cmd-item-sub">${esc(act.sub)}</span>
      </button>
    `).join("");
    el.querySelectorAll("[data-cmd-idx]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const act = state.cmdActions[Number(btn.getAttribute("data-cmd-idx"))];
        closeCmdPalette();
        if (act) act.run();
      });
    });
  }

  // --- Manual Entry Modal ---
  function setManualAssetKind(kind) {
    state.manualKind = kind || "subscription";
    document.querySelectorAll("[data-man-kind]").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-man-kind") === state.manualKind);
    });
    $("man-panel-license").classList.toggle("hidden", state.manualKind !== "license");
    $("man-panel-hardware").classList.toggle("hidden", state.manualKind !== "hardware");
    $("man-panel-reimb").classList.toggle("hidden", state.manualKind !== "reimbursable");
    $("man-cycle-row").classList.toggle("hidden", state.manualKind === "hardware" || state.manualKind === "license");

    if (state.manualKind === "hardware" || state.manualKind === "license") state.manualCycle = "one_time";
    else if (state.manualKind === "subscription" && state.manualCycle === "one_time") state.manualCycle = "monthly";

    document.querySelectorAll("[data-man-cycle]").forEach((x) => {
      x.classList.toggle("active", x.getAttribute("data-man-cycle") === state.manualCycle);
    });
  }

  function openManualModal() {
    $("modal-manual").classList.remove("hidden");
    $("man-vendor").value = "";
    $("man-plan").value = "";
    $("man-amount").value = "";
    $("man-notes").value = "";
    $("man-license-key").value = "";
    $("man-license-key").type = "password";
    $("btn-man-toggle-key").textContent = "显示";
    $("man-license-scope").value = "";
    $("man-warranty-end").value = "";
    $("man-reimburse-status").value = "pending";
    $("man-bill-date").value = new Date().toISOString().slice(0, 10);
    setManualAssetKind("subscription");
    $("man-vendor").focus();
  }

  function closeManualModal() {
    $("modal-manual").classList.add("hidden");
  }

  // --- Batch File & Folder Upload ---
  async function importFileList(files) {
    const valid = Array.from(files).filter((f) => f && f.size > 0);
    if (!valid.length) return;
    const prog = $("import-progress");
    prog.classList.remove("hidden");

    const batchId = `b_${Date.now().toString(36)}`;
    let added = 0;
    let reused = 0;
    let failed = 0;
    let lastError = "";

    for (let i = 0; i < valid.length; i++) {
      const f = valid[i];
      prog.textContent = `加密入库 ${i + 1} / ${valid.length} · ${f.name}`;
      try {
        const buf = await f.arrayBuffer();
        const res = await api("POST", "/api/import", {
          filename: f.name,
          content_base64: arrayBufferToBase64(buf),
          batch_id: batchId,
        });
        if (res.reused) reused += 1;
        else added += 1;
      } catch (err) {
        failed += 1;
        lastError = `${f.name}: ${err.message}`;
      }
    }

    prog.classList.add("hidden");
    await refreshWorkspace();
    const parts = [];
    if (added > 0) parts.push(`新入库 ${added} 份`);
    if (reused > 0) parts.push(`防重跳过 ${reused} 份`);
    if (failed > 0) parts.push(`失败 ${failed} 份（${lastError}）`);
    showToast(parts.join("，") || "导入完成", failed > 0 && added === 0 ? "error" : "ok");
  }

  async function collectDroppedFiles(dataTransfer) {
    const out = [];
    const items = dataTransfer.items ? Array.from(dataTransfer.items) : [];
    if (!items.some((it) => typeof it.webkitGetAsEntry === "function")) {
      return Array.from(dataTransfer.files || []);
    }
    async function walkEntry(entry) {
      if (!entry) return;
      if (entry.isFile) {
        const file = await new Promise((resolve) => entry.file(resolve, () => resolve(null)));
        if (file) out.push(file);
      } else if (entry.isDirectory) {
        const reader = entry.createReader();
        while (true) {
          const batch = await new Promise((resolve) => reader.readEntries(resolve, () => resolve([])));
          if (!batch.length) break;
          for (const child of batch) await walkEntry(child);
        }
      }
    }
    for (const item of items) {
      const entry = item.webkitGetAsEntry();
      if (entry) await walkEntry(entry);
    }
    return out;
  }

  function openRecoveryModal(recoveryKey, onDone) {
    state.pendingRecoveryKey = recoveryKey;
    $("modal-recovery-key").textContent = recoveryKey;
    $("chk-recovery-saved").checked = false;
    $("btn-confirm-recovery").disabled = true;
    $("modal-recovery").classList.remove("hidden");
    $("btn-confirm-recovery").onclick = () => {
      $("modal-recovery").classList.add("hidden");
      state.pendingRecoveryKey = null;
      if (onDone) onDone();
    };
  }

  // --- Event Binding ---
  function bindEvents() {
    $("toast-close").addEventListener("click", () => $("toast").classList.add("hidden"));
    $("lobby-theme-btn").addEventListener("click", cycleTheme);
    $("btn-theme-cycle").addEventListener("click", cycleTheme);
    $("btn-privacy-toggle").addEventListener("click", () => togglePrivacy());
    $("btn-cmd-open").addEventListener("click", openCmdPalette);
    $("btn-open-manual").addEventListener("click", openManualModal);
    $("btn-close-manual").addEventListener("click", closeManualModal);
    $("btn-cancel-manual").addEventListener("click", closeManualModal);
    $("btn-import-files").addEventListener("click", () => $("file-picker-input").click());
    $("btn-import-folder").addEventListener("click", () => $("folder-picker-input").click());
    $("sb-drop-hint").addEventListener("click", () => $("file-picker-input").click());

    // Keyboard shortcuts
    document.addEventListener("keydown", (e) => {
      const inInput = ["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName);
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        if (state.cmdOpen) closeCmdPalette();
        else openCmdPalette();
        return;
      }
      if (state.cmdOpen) {
        if (e.key === "Escape") { closeCmdPalette(); return; }
        if (e.key === "ArrowDown") {
          e.preventDefault();
          state.cmdIndex = (state.cmdIndex + 1) % Math.max(1, state.cmdActions.length);
          renderCmdList($("cmd-input").value);
          return;
        }
        if (e.key === "ArrowUp") {
          e.preventDefault();
          state.cmdIndex = (state.cmdIndex - 1 + state.cmdActions.length) % Math.max(1, state.cmdActions.length);
          renderCmdList($("cmd-input").value);
          return;
        }
        if (e.key === "Enter" && state.cmdActions[state.cmdIndex]) {
          e.preventDefault();
          const act = state.cmdActions[state.cmdIndex];
          closeCmdPalette();
          act.run();
          return;
        }
        return;
      }
      if (e.key === "Escape") {
        if (!$("modal-manual").classList.contains("hidden")) { closeManualModal(); return; }
        if (state.selectedSourceId) { closeDrawer(); return; }
      }
      if (!inInput && !$("screen-workspace").classList.contains("hidden")) {
        if (e.key === "/") {
          e.preventDefault();
          navigateTo("assets");
          $("search-box").focus();
        } else if (e.key.toLowerCase() === "p") {
          togglePrivacy();
        }
      }
    });

    $("cmd-input").addEventListener("input", (e) => { state.cmdIndex = 0; renderCmdList(e.target.value); });
    $("modal-cmd").addEventListener("click", (e) => { if (e.target === $("modal-cmd")) closeCmdPalette(); });
    $("modal-manual").addEventListener("click", (e) => { if (e.target === $("modal-manual")) closeManualModal(); });

    // Folder pickers
    document.querySelectorAll("[data-pick-target]").forEach((btn) => {
      btn.addEventListener("click", async () => {
        const targetId = btn.getAttribute("data-pick-target");
        const title = btn.getAttribute("data-pick-title") || "选择文件夹";
        const suffix = btn.getAttribute("data-pick-suffix") || "";
        btn.disabled = true;
        try {
          const res = await api("POST", "/api/pick-folder", { title });
          if (!res.cancelled && res.path) {
            const sep = res.path.includes("\\") ? "\\" : "/";
            const trimmed = res.path.replace(/[\\/]+$/, "");
            $(targetId).value = suffix ? `${trimmed}${sep}${suffix}` : res.path;
          }
        } catch (err) {
          showToast(err.message, "error");
        } finally {
          btn.disabled = false;
        }
      });
    });

    // Lobby tabs & unlock modes
    document.querySelectorAll("[data-lobby-tab]").forEach((b) => {
      b.addEventListener("click", () => setLobbyTab(b.getAttribute("data-lobby-tab")));
    });
    document.querySelectorAll("[data-unlock-mode]").forEach((b) => {
      b.addEventListener("click", () => setUnlockMode(b.getAttribute("data-unlock-mode")));
    });
    document.querySelectorAll('input[name="create-protection"]').forEach((radio) => {
      radio.addEventListener("change", () => {
        const isPass = radio.value === "passphrase";
        $("opt-passphrase").classList.toggle("active", isPass);
        $("opt-os-disk").classList.toggle("active", !isPass);
        $("create-passphrase-fields").classList.toggle("hidden", !isPass);
      });
    });

    $("form-unlock-vault").addEventListener("submit", async (e) => {
      e.preventDefault();
      const errBox = $("unlock-error");
      errBox.classList.add("hidden");
      const btn = $("btn-unlock-submit");
      btn.disabled = true;
      btn.textContent = "解锁中…";
      try {
        const payload = { path: $("unlock-path").value.trim() };
        if (state.unlockMode === "passphrase") payload.passphrase = $("unlock-passphrase").value;
        else if (state.unlockMode === "recovery") payload.recovery_key = $("unlock-recovery").value.trim();
        const snap = await api("POST", "/api/vault/unlock", payload);
        $("unlock-passphrase").value = "";
        $("unlock-recovery").value = "";
        state.workspace = snap;
        showWorkspaceScreen();
        showToast("账本已解锁，欢迎回来", "ok");
      } catch (err) {
        errBox.textContent = err.message;
        errBox.classList.remove("hidden");
      } finally {
        btn.disabled = false;
        btn.textContent = "解锁并进入票据馆";
      }
    });

    $("form-create-vault").addEventListener("submit", async (e) => {
      e.preventDefault();
      const errBox = $("create-error");
      errBox.classList.add("hidden");
      const prot = document.querySelector('input[name="create-protection"]:checked').value;
      const pass = $("create-passphrase").value;
      const confirmPass = $("create-passphrase-confirm").value;
      if (prot === "passphrase") {
        if (pass.length < 6) {
          errBox.textContent = "密码至少需要 6 位字符";
          errBox.classList.remove("hidden");
          return;
        }
        if (pass !== confirmPass) {
          errBox.textContent = "两次输入的密码不一致";
          errBox.classList.remove("hidden");
          return;
        }
      }
      const btn = $("btn-create-submit");
      btn.disabled = true;
      btn.textContent = "正在创建加密账本…";
      try {
        const res = await api("POST", "/api/vault/init", {
          path: $("create-path").value.trim(),
          protection: prot,
          passphrase: prot === "passphrase" ? pass : "",
        });
        $("create-passphrase").value = "";
        $("create-passphrase-confirm").value = "";
        await refreshWorkspace();
        if (res.recovery_key) {
          openRecoveryModal(res.recovery_key, () => {
            showWorkspaceScreen();
            showToast("新加密账本已就绪", "ok");
          });
        } else {
          showWorkspaceScreen();
          showToast("新本地账本已就绪", "ok");
        }
      } catch (err) {
        errBox.textContent = err.message;
        errBox.classList.remove("hidden");
      } finally {
        btn.disabled = false;
        btn.textContent = "创建本地账本并开始";
      }
    });

    // Recovery modal
    $("chk-recovery-saved").addEventListener("change", (e) => { $("btn-confirm-recovery").disabled = !e.target.checked; });
    $("btn-copy-recovery").addEventListener("click", async () => {
      if (!state.pendingRecoveryKey) return;
      try {
        await navigator.clipboard.writeText(state.pendingRecoveryKey);
        showToast("恢复密钥已复制", "ok");
      } catch (_) {
        showToast("请点击下载 .txt 备份文件", "error");
      }
    });
    $("btn-download-recovery").addEventListener("click", () => {
      if (!state.pendingRecoveryKey) return;
      downloadTextFile("SubVault-离线恢复密钥.txt", `# SubVault 离线恢复密钥（请离线妥善保管）\n${state.pendingRecoveryKey}\n`);
    });

    // Manual entry modal
    document.querySelectorAll("[data-man-kind]").forEach((b) => {
      b.addEventListener("click", () => setManualAssetKind(b.getAttribute("data-man-kind")));
    });
    $("btn-man-toggle-key").addEventListener("click", () => {
      const inp = $("man-license-key");
      const show = inp.type === "password";
      inp.type = show ? "text" : "password";
      $("btn-man-toggle-key").textContent = show ? "隐藏" : "显示";
    });
    document.querySelectorAll("[data-man-cycle]").forEach((b) => {
      b.addEventListener("click", () => {
        state.manualCycle = b.getAttribute("data-man-cycle");
        document.querySelectorAll("[data-man-cycle]").forEach((x) => x.classList.toggle("active", x === b));
      });
    });
    document.querySelectorAll("[data-man-acc]").forEach((b) => {
      b.addEventListener("click", () => {
        $("man-account").value = b.getAttribute("data-man-acc");
        document.querySelectorAll("[data-man-acc]").forEach((x) => x.classList.toggle("active", x === b));
      });
    });
    $("form-manual-entry").addEventListener("submit", async (e) => {
      e.preventDefault();
      const btn = $("btn-submit-manual");
      btn.disabled = true;
      try {
        const payload = {
          vendor: $("man-vendor").value,
          plan_name: $("man-plan").value,
          currency: $("man-currency").value,
          amount_text: $("man-amount").value,
          billing_cycle: state.manualCycle,
          account_label: $("man-account").value,
          category: $("man-category").value,
          bill_date: $("man-bill-date").value,
          notes: $("man-notes").value,
          asset_kind: state.manualKind,
        };
        if (state.manualKind === "license") {
          payload.license_key = $("man-license-key").value;
          payload.license_scope = $("man-license-scope").value;
        } else if (state.manualKind === "hardware") {
          payload.warranty_end = $("man-warranty-end").value;
        } else if (state.manualKind === "reimbursable") {
          payload.reimburse_status = $("man-reimburse-status").value;
        }
        const snap = await api("POST", "/api/manual-entry", payload);
        state.workspace = snap;
        closeManualModal();
        renderWorkspace();
        showToast("已加密登记进资产库", "ok");
      } catch (err) {
        showToast(err.message, "error");
      } finally {
        btn.disabled = false;
      }
    });

    // Sidebar navigation & collections
    document.querySelectorAll("[data-nav]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const nav = btn.getAttribute("data-nav");
        navigateTo(nav, nav === "assets" ? "all" : undefined);
      });
    });
    document.querySelectorAll("[data-collection]").forEach((btn) => {
      btn.addEventListener("click", () => navigateTo("assets", btn.getAttribute("data-collection")));
    });

    // Clear demo pack button
    $("btn-clear-demo-pack").addEventListener("click", async () => {
      const btn = $("btn-clear-demo-pack");
      btn.disabled = true;
      btn.textContent = "正在清除…";
      try {
        const snap = await api("POST", "/api/clear-demo-pack", {});
        state.workspace = snap;
        closeDrawer();
        renderWorkspace();
        showToast(`已清除 ${snap.removed_count || 0} 项体验示例数据，你的真实账单完好无损`, "ok");
      } catch (err) {
        showToast(err.message, "error");
      } finally {
        btn.disabled = false;
        btn.textContent = "一键清空示例数据";
      }
    });

    // Single source delete with inline 2-step confirmation
    $("btn-ask-delete-item").addEventListener("click", () => {
      $("btn-ask-delete-item").classList.add("hidden");
      $("delete-confirm-bar").classList.remove("hidden");
    });
    $("btn-cancel-delete-item").addEventListener("click", () => {
      $("delete-confirm-bar").classList.add("hidden");
      $("btn-ask-delete-item").classList.remove("hidden");
    });
    $("btn-confirm-delete-item").addEventListener("click", async () => {
      if (!state.selectedSourceId) return;
      const btn = $("btn-confirm-delete-item");
      btn.disabled = true;
      btn.textContent = "移出中…";
      try {
        const snap = await api("POST", "/api/sources/delete", { source_id: state.selectedSourceId });
        state.workspace = snap;
        closeDrawer();
        renderWorkspace();
        showToast("已从当前账本移出这条记录", "ok");
      } catch (err) {
        showToast(err.message, "error");
      } finally {
        btn.disabled = false;
        btn.textContent = "确认移出";
      }
    });

    // File pickers, lock, shutdown
    $("file-picker-input").addEventListener("change", (e) => { importFileList(e.target.files); e.target.value = ""; });
    $("folder-picker-input").addEventListener("change", (e) => { importFileList(e.target.files); e.target.value = ""; });
    $("btn-lock-vault").addEventListener("click", async () => {
      await api("POST", "/api/vault/lock", {});
      state.workspace = null;
      const info = await api("GET", "/api/bootstrap");
      renderRecentVaults(info.recent_vaults || []);
      showLobbyScreen();
      showToast("账本已锁定，内存密钥已清空", "ok");
    });
    $("btn-shutdown-app").addEventListener("click", async () => {
      try { await api("POST", "/api/shutdown", {}); } catch (_) {}
      document.body.innerHTML = `
        <div style="max-width:440px;margin:130px auto;text-align:center;font-family:var(--font-sans);padding:24px">
          <h2 style="margin:0 0 8px;font-size:19px">SubVault 已安全锁定并退出</h2>
          <p style="margin:0;color:var(--ink-secondary);font-size:13.5px">内存密钥已清空，本机服务已停止，你可以安全关闭此窗口。</p>
        </div>
      `;
    });

    // Global drag & drop
    const overlay = $("global-drop-overlay");
    window.addEventListener("dragenter", (e) => {
      if ($("screen-workspace").classList.contains("hidden")) return;
      e.preventDefault();
      state.dragDepth += 1;
      overlay.classList.remove("hidden");
    });
    window.addEventListener("dragover", (e) => {
      if ($("screen-workspace").classList.contains("hidden")) return;
      e.preventDefault();
    });
    window.addEventListener("dragleave", () => {
      state.dragDepth = Math.max(0, state.dragDepth - 1);
      if (state.dragDepth === 0) overlay.classList.add("hidden");
    });
    window.addEventListener("drop", async (e) => {
      state.dragDepth = 0;
      overlay.classList.add("hidden");
      if ($("screen-workspace").classList.contains("hidden")) return;
      e.preventDefault();
      const files = await collectDroppedFiles(e.dataTransfer);
      importFileList(files);
    });

    // View mode, filters, sort, search
    document.querySelectorAll("[data-view-mode]").forEach((btn) => {
      btn.addEventListener("click", () => {
        state.viewMode = btn.getAttribute("data-view-mode");
        if (state.workspace) renderAssetsPane(state.workspace);
      });
    });
    $("filter-account").addEventListener("change", (e) => { state.accountFilter = e.target.value; renderWorkspace(); });
    $("filter-currency").addEventListener("change", (e) => { state.currencyFilter = e.target.value; renderWorkspace(); });
    $("sort-order").addEventListener("change", (e) => { state.sortOrder = e.target.value; if (state.workspace) renderAssetsPane(state.workspace); });
    $("search-box").addEventListener("input", (e) => {
      state.searchQuery = e.target.value;
      if (state.workspace) {
        renderStageHeader(state.workspace);
        renderAssetsPane(state.workspace);
      }
    });

    // Drawer controls
    $("btn-close-drawer").addEventListener("click", closeDrawer);
    $("drawer-backdrop").addEventListener("click", closeDrawer);
    document.querySelectorAll(".dtab").forEach((btn) => {
      btn.addEventListener("click", () => switchDrawerTab(btn.getAttribute("data-dtab")));
    });
    $("btn-toggle-text-view").addEventListener("click", () => {
      state.textViewActive = !state.textViewActive;
      $("evidence-text-box").classList.toggle("hidden", !state.textViewActive);
    });
    $("csv-search").addEventListener("input", () => {
      const item = (state.workspace?.items || []).find((i) => i.source_id === state.selectedSourceId);
      if (item) renderCsvRows(item);
    });
    $("chk-csv-expense-only").addEventListener("change", () => {
      const item = (state.workspace?.items || []).find((i) => i.source_id === state.selectedSourceId);
      if (item) renderCsvRows(item);
    });

    document.querySelectorAll("[data-ed-kind]").forEach((b) => {
      b.addEventListener("click", () => setEditAssetKind(b.getAttribute("data-ed-kind"), true));
    });
    ["ed-bill-date", "ed-warranty-end", "ed-amount", "ed-currency"].forEach((id) => {
      $(id).addEventListener("input", updateEditHardwareMetrics);
      $(id).addEventListener("change", updateEditHardwareMetrics);
    });
    $("btn-ed-toggle-key").addEventListener("click", () => {
      const inp = $("ed-license-key");
      const show = inp.type === "password";
      inp.type = show ? "text" : "password";
      $("btn-ed-toggle-key").textContent = show ? "隐藏" : "显示";
    });
    $("btn-ed-copy-key").addEventListener("click", () => {
      const val = $("ed-license-key").value.trim();
      if (!val) { showToast("还没有填写激活密钥", "error"); return; }
      copyLicenseKey($("ed-vendor").value, val);
    });

    $("form-edit-item").addEventListener("submit", async (e) => {
      e.preventDefault();
      if (!state.selectedSourceId) return;
      const btn = $("btn-save-item");
      btn.disabled = true;
      btn.textContent = "保存中…";
      try {
        const payload = {
          source_id: state.selectedSourceId,
          vendor: $("ed-vendor").value,
          plan_name: $("ed-plan").value,
          account_label: $("ed-account").value,
          billing_cycle: $("ed-cycle").value,
          currency: $("ed-currency").value,
          amount_text: $("ed-amount").value,
          category: $("ed-category").value,
          bill_date: $("ed-bill-date").value,
          period_start: $("ed-period-start").value,
          period_end: $("ed-period-end").value,
          notes: $("ed-notes").value,
          asset_kind: state.editKind,
          warranty_end: $("ed-warranty-end").value,
          license_scope: $("ed-license-scope").value,
          reimburse_status: $("ed-reimburse-status").value,
        };
        if (state.editKind === "license") payload.license_key = $("ed-license-key").value;
        const snap = await api("POST", "/api/sources/save", payload);
        state.workspace = snap;
        renderWorkspace();
        const refreshed = (snap.items || []).find((i) => i.source_id === state.selectedSourceId);
        if (refreshed) renderDrawer(refreshed);
        showToast("修改已保存，原件未改动", "ok");
      } catch (err) {
        showToast(err.message, "error");
      } finally {
        btn.disabled = false;
        btn.textContent = "保存修改";
      }
    });

    // Redact
    $("btn-run-redact").addEventListener("click", async () => {
      if (!state.selectedSourceId) return;
      const terms = ($("redact-custom-terms").value || "").split(/[,，\n]/).map((s) => s.trim()).filter(Boolean);
      const btn = $("btn-run-redact");
      btn.disabled = true;
      try {
        const res = await api("POST", "/api/sources/redact", { source_id: state.selectedSourceId, redact_terms: terms });
        state.lastRedactedText = res.redacted_text || "";
        $("redact-stats").textContent = `已完成脱敏：共替换 ${res.total_redactions || 0} 处敏感信息（不影响原件）`;
        $("redact-output").textContent = state.lastRedactedText;
        $("redact-result-wrap").classList.remove("hidden");
      } catch (err) {
        showToast(err.message, "error");
      } finally {
        btn.disabled = false;
      }
    });
    $("btn-copy-redact").addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(state.lastRedactedText);
        showToast("脱敏内容已复制", "ok");
      } catch (_) {
        showToast("请点下载 .txt", "error");
      }
    });
    $("btn-dl-redact").addEventListener("click", () => downloadTextFile("subvault-redacted-template.txt", state.lastRedactedText));

    // Exports & Security
    $("btn-dl-excel").addEventListener("click", () => { window.location.href = tokenUrl("/api/export/csv"); });
    $("btn-dl-zip").addEventListener("click", () => {
      const enc = state.workspace?.vault?.encrypted;
      if (enc && !$("chk-zip-ack").checked) {
        showToast("请先勾选明文导出确认框", "error");
        return;
      }
      window.location.href = tokenUrl(`/api/export/zip${enc ? "?acknowledge_plaintext=yes" : ""}`);
    });
    $("btn-export-dir").addEventListener("click", async () => {
      const outDir = $("export-dir-path").value.trim();
      const enc = state.workspace?.vault?.encrypted;
      const ack = $("chk-dir-ack").checked;
      const resBox = $("export-dir-result");
      resBox.classList.add("hidden");
      if (!outDir) { showToast("请填写导出文件夹路径", "error"); return; }
      if (enc && !ack) { showToast("请先勾选明文导出确认框", "error"); return; }
      try {
        const res = await api("POST", "/api/export/package", { out_dir: outDir, acknowledge_plaintext: Boolean(ack) });
        resBox.className = "inline-result";
        resBox.textContent = res.verified
          ? `导出并校验通过：共核验 ${res.verification?.files_verified ?? "?"} 个文件`
          : `已导出到 ${outDir}，但校验存在提醒`;
        resBox.classList.remove("hidden");
        $("verify-dir-path").value = outDir;
      } catch (err) {
        resBox.className = "inline-result error";
        resBox.textContent = err.message;
        resBox.classList.remove("hidden");
      }
    });
    $("btn-verify-dir").addEventListener("click", async () => {
      const dir = $("verify-dir-path").value.trim();
      const resBox = $("verify-dir-result");
      resBox.classList.add("hidden");
      if (!dir) { showToast("请填写备份包目录路径", "error"); return; }
      try {
        const res = await api("POST", "/api/verify-package", { package_dir: dir });
        resBox.className = res.verified ? "inline-result" : "inline-result error";
        resBox.textContent = res.verified
          ? `校验通过：共 ${res.report?.files_verified ?? "?"} 个文件完整无篡改`
          : `校验未通过：${res.report?.reason || "发现文件缺失或被修改"}`;
        resBox.classList.remove("hidden");
      } catch (err) {
        resBox.className = "inline-result error";
        resBox.textContent = err.message;
        resBox.classList.remove("hidden");
      }
    });
    $("form-rotate-pass").addEventListener("submit", async (e) => {
      e.preventDefault();
      const p1 = $("rotate-new-pass").value;
      const p2 = $("rotate-new-pass-confirm").value;
      const rotateRec = $("chk-rotate-recovery").checked;
      if (p1.length < 6) { showToast("新密码至少需要 6 位字符", "error"); return; }
      if (p1 !== p2) { showToast("两次输入的新密码不一致", "error"); return; }
      try {
        const res = await api("POST", "/api/vault/rotate-passphrase", {
          new_passphrase: p1,
          rotate_recovery_key: rotateRec,
        });
        $("rotate-new-pass").value = "";
        $("rotate-new-pass-confirm").value = "";
        $("chk-rotate-recovery").checked = false;
        if (res.recovery_key) openRecoveryModal(res.recovery_key, () => showToast("主密码与恢复密钥已更新", "ok"));
        else showToast("账本主密码已更新", "ok");
      } catch (err) {
        showToast(err.message, "error");
      }
    });
  }

  document.addEventListener("DOMContentLoaded", boot);
})();
