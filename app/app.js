(() => {
  "use strict";

  const TOKEN_STORE_KEY = "subvault_session_token_v1";
  let SESSION_TOKEN = "";
  const hashMatch = window.location.hash.match(/token=([0-9a-fA-F]+)/);
  if (hashMatch) {
    SESSION_TOKEN = hashMatch[1];
    try { sessionStorage.setItem(TOKEN_STORE_KEY, SESSION_TOKEN); } catch (_) {}
    try { window.history.replaceState(null, "", window.location.pathname); } catch (_) {}
  } else {
    try { SESSION_TOKEN = sessionStorage.getItem(TOKEN_STORE_KEY) || ""; } catch (_) {}
  }

  const ICO = {
    copy: `<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="13" height="13" x="9" y="9" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`,
    check: `<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`,
    trendUp: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"><polyline points="22 7 13.5 15.5 8.5 10.5 2 17"/><polyline points="16 7 22 7 22 13"/></svg>`,
    trendDown: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"><polyline points="22 17 13.5 8.5 8.5 13.5 2 7"/><polyline points="16 17 22 17 22 11"/></svg>`,
    shield: `<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10"/><path d="m9 12 2 2 4-4"/></svg>`,
    receipt: `<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M4 2v20l2-1 2 1 2-1 2 1 2-1 2 1 2-1 2 1V2l-2 1-2-1-2 1-2-1-2 1-2-1-2 1Z"/><path d="M8 8h8M8 12h6"/></svg>`,
    key: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><circle cx="7.5" cy="15.5" r="4.5"/><path d="m21 2-9.6 9.6"/><path d="m15.5 7.5 3 3L22 7l-3-3"/></svg>`,
    inbox: `<svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><polyline points="22 12 16 12 14 15 10 15 8 12 2 12"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/></svg>`,
    sparkle: `<svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3Z"/></svg>`,
    plus: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>`,
    calendar: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><rect width="18" height="18" x="3" y="4" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/></svg>`,
    chart: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M3 3v18h18"/><path d="m19 9-5 5-4-4-3 3"/></svg>`,
    bolt: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/></svg>`,
    eye: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z"/><circle cx="12" cy="12" r="3"/></svg>`,
    download: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/></svg>`,
    lock: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><rect width="18" height="11" x="3" y="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>`,
    sun: `<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2m0 16v2M4.93 4.93l1.41 1.41m11.32 11.32 1.41 1.41M2 12h2m16 0h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/></svg>`,
    paper: `<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20"/><path d="M8 7h8M8 11h6"/></svg>`,
    moon: `<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/></svg>`,
    laptop: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><rect width="16" height="12" x="4" y="4" rx="2"/><path d="M2 20h20"/></svg>`,
    refresh: `<svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16"/><path d="M16 16h5v5"/></svg>`,
    arrowRight: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14"/><path d="m12 5 7 7-7 7"/></svg>`,
    trash: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M19 6l-1 14H6L5 6"/></svg>`,
    eyeOpen: `<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z"/><circle cx="12" cy="12" r="3"/></svg>`,
    eyeOff: `<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M9.9 4.24A9.1 9.1 0 0 1 12 4c6.5 0 10 8 10 8a17.6 17.6 0 0 1-2.16 3.19"/><path d="M6.6 6.6C3.9 8.4 2 12 2 12s3.5 8 10 8a9.7 9.7 0 0 0 5.4-1.6"/><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2"/><path d="m2 2 20 20"/></svg>`,
    party: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M5.8 11.3 2 22l10.7-3.79"/><path d="M4 3h.01M22 8h.01M15 2h.01M22 20h.01"/><path d="m22 2-2.24.75a2.9 2.9 0 0 0-1.96 3.12c.1.86-.57 1.63-1.45 1.63h-.38c-.86 0-1.6.6-1.76 1.44L14 10"/><path d="M11 13c1.93 1.93 2.83 4.17 2 5-.83.83-3.07-.07-5-2-1.93-1.93-2.83-4.17-2-5 .83-.83 3.07.07 5 2Z"/></svg>`,
  };

  // 空账本插画：会慢慢转动转盘的小金库，上面飘着两张票据
  const VAULT_ART = `
    <svg class="vault-art" viewBox="0 0 112 96" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <g class="va-paper p1" style="--r:-9deg">
        <rect x="26" y="4" width="26" height="32" rx="3" style="fill: var(--surface)" stroke-width="1.6"/>
        <path d="M31 12h10M31 17h15M31 22h12" stroke-width="1.6" stroke-opacity="0.45"/>
      </g>
      <g class="va-paper p2" style="--r:8deg">
        <rect x="58" y="2" width="26" height="32" rx="3" style="fill: var(--surface)" stroke-width="1.6"/>
        <path d="M63 10h8" stroke-width="2.2" style="stroke: var(--blue)"/>
        <path d="M63 16h15M63 21h11" stroke-width="1.6" stroke-opacity="0.45"/>
      </g>
      <rect x="14" y="24" width="84" height="62" rx="12" style="fill: var(--surface)" stroke-width="2"/>
      <rect x="20" y="30" width="72" height="50" rx="8" stroke-width="1.2" stroke-opacity="0.35"/>
      <path d="M14 40h-3M14 70h-3" stroke-width="4"/>
      <path d="M26 86v5M86 86v5" stroke-width="4"/>
      <circle cx="56" cy="55" r="18" stroke-width="2"/>
      <circle cx="56" cy="55" r="13" stroke-width="1" stroke-opacity="0.4" stroke-dasharray="1.5 3"/>
      <g class="va-wheel">
        <path d="M56 45v20M47.3 50l17.4 10M47.3 60l17.4-10" stroke-width="2.4"/>
        <circle cx="56" cy="55" r="3.5" fill="currentColor" stroke="none"/>
      </g>
    </svg>`;

  const LOCK_ART = `
    <svg viewBox="0 0 24 24" width="30" height="30" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path class="fl-shackle" d="M7 11V7a5 5 0 0 1 10 0v4"/>
      <rect width="18" height="11" x="3" y="11" rx="2.5"/>
      <circle cx="12" cy="16.5" r="1.4" fill="currentColor" stroke="none"/>
    </svg>`;

  const THEMES = [
    { id: "gallery", label: "浅色" },
    { id: "paper", label: "暖白" },
    { id: "night", label: "深色" },
  ];

  const LIGHT_CHART_PALETTE = [
    "#2563eb",
    "#0d9488",
    "#7c3aed",
    "#d97706",
    "#e11d48",
    "#0284c7",
    "#10b981",
    "#6366f1",
  ];
  const DARK_CHART_PALETTE = [
    "#60a5fa",
    "#34d399",
    "#a78bfa",
    "#fbbf24",
    "#fb7185",
    "#38bdf8",
    "#2dd4bf",
    "#818cf8",
  ];
  function getChartPalette() {
    return state.theme === "night" ? DARK_CHART_PALETTE : LIGHT_CHART_PALETTE;
  }

  const KIND_COLORS = {
    hardware: "var(--iris)",
    subscription: "var(--blue)",
    license: "var(--green)",
    reimbursable: "var(--amber)",
    general: "var(--ink-3)",
  };

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
    CNY: "¥", USD: "$", EUR: "€", GBP: "£", JPY: "JP¥", HKD: "HK$",
    CAD: "CA$", AUD: "AU$", SGD: "S$", TWD: "NT$", KRW: "₩", CHF: "CHF ",
  };

  const CURRENCY_NAMES = {
    CNY: "人民币", USD: "美元", EUR: "欧元", GBP: "英镑", JPY: "日元", HKD: "港币",
    CAD: "加元", AUD: "澳元", SGD: "新元", TWD: "新台币", KRW: "韩元", CHF: "瑞士法郎",
  };

  const CYCLE_LABELS = {
    monthly: "月付", yearly: "年付", quarterly: "季付", weekly: "周付", one_time: "一次性", unknown: "未标注",
  };

  const ASSET_KIND_META = {
    subscription: { label: "订阅", pill: "green" },
    license: { label: "买断", pill: "blue" },
    hardware: { label: "硬件", pill: "iris" },
    reimbursable: { label: "报销", pill: "amber" },
    general: { label: "账单", pill: "slate" },
  };

  const REIMBURSE_META = {
    pending: { label: "待报销", next: "submitted", cls: "pending" },
    submitted: { label: "已提交", next: "reimbursed", cls: "submitted" },
    reimbursed: { label: "已到账", next: "pending", cls: "reimbursed" },
  };

  const state = {
    bootstrap: null,
    workspace: null,
    theme: "gallery",
    privacyBlur: false,
    lobbyTab: "unlock",
    unlockMode: "passphrase",
    activeNav: "assets",
    collection: "all",
    viewMode: "cards",
    heroCurrency: "",
    reimburseFilter: "",
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
    csvUiState: new Map(),
    drawerReturnFocus: null,
    modalReturnFocus: null,
    cmdReturnFocus: null,
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
    const bg = LIGHT_CHART_PALETTE[h % LIGHT_CHART_PALETTE.length];
    const first = Array.from(raw)[0] || "S";
    const mark = /[\u4e00-\u9fff]/.test(first) ? first : raw.slice(0, 2).toUpperCase();
    return { bg, mark };
  }

  function formatWithCommas(s) {
    return String(s).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  }

  function renderEditorialMoney(currency, amountText, showCode = true) {
    if (!amountText) return "";
    const sym = CURRENCY_SYMBOLS[currency] || "";
    const [whole, frac] = String(amountText).split(".");
    const dec = frac !== undefined ? `<span class="money-dec">.${esc(frac)}</span>` : "";
    const code = showCode ? `<span class="money-code">${esc(currency)}</span>` : "";
    return `${esc(sym)}${esc(formatWithCommas(whole || "0"))}${dec}${code}`;
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
    if (text === null || text === undefined || text === "") return 0;
    const cleaned = String(text).trim().replace(/[,\s]/g, "");
    if (!cleaned) return 0;
    const neg = cleaned.startsWith("-");
    const unsigned = cleaned.replace(/^[+-]/, "");
    const parts = unsigned.split(".");
    const w = parseInt(parts[0], 10);
    if (!Number.isFinite(w)) return 0;
    const fracRaw = (parts[1] || "").replace(/\D/g, "");
    const f = parseInt(fracRaw.slice(0, 2).padEnd(2, "0"), 10) || 0;
    const total = w * 100 + f;
    return neg ? -total : total;
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
    try { return new Date(ms).toLocaleString("zh-CN", { hour12: false }); } catch (_) { return String(ms); }
  }

  let toastTimer = null;
  function showToast(msg, kind = "info") {
    const el = $("toast");
    $("toast-msg").textContent = msg;
    el.className = `toast ${kind === "error" ? "toast-error" : kind === "ok" ? "toast-ok" : ""}`;
    el.classList.remove("hidden");
    void el.offsetWidth;
    el.classList.add("toast-anim");
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
    if (resp.status === 401 && data.error === "unauthorized") {
      try { sessionStorage.removeItem(TOKEN_STORE_KEY); } catch (_) {}
    }
    if (!resp.ok || data.ok === false) {
      const err = new Error(data.message || data.error || `请求失败 (${resp.status})`);
      err.code = data.error || "request_failed";
      err.status = resp.status;
      throw err;
    }
    return data;
  }

  const activeBlobUrls = new Set();
  function trackBlobUrl(url) {
    activeBlobUrls.add(url);
    return url;
  }
  function revokeBlobUrls() {
    for (const url of activeBlobUrls) {
      try { URL.revokeObjectURL(url); } catch (_) {}
    }
    activeBlobUrls.clear();
  }

  async function fetchAuthedBlob(path) {
    const resp = await fetch(path, { headers: { "X-SubVault-Token": SESSION_TOKEN } });
    if (!resp.ok) {
      const data = await resp.json().catch(() => ({}));
      if (resp.status === 401 && data.error === "unauthorized") {
        try { sessionStorage.removeItem(TOKEN_STORE_KEY); } catch (_) {}
      }
      throw new Error(data.message || data.error || `下载失败 (${resp.status})`);
    }
    const blob = await resp.blob();
    const disp = resp.headers.get("content-disposition") || "";
    let filename = "";
    const utf8Match = disp.match(/filename\*=UTF-8''([^;]+)/i);
    if (utf8Match) {
      try { filename = decodeURIComponent(utf8Match[1]); } catch (_) {}
    }
    if (!filename) {
      const asciiMatch = disp.match(/filename="([^"]+)"/i);
      if (asciiMatch) filename = asciiMatch[1];
    }
    return { blob, filename, contentType: resp.headers.get("content-type") || blob.type || "" };
  }

  async function downloadAuthedFile(path, fallbackName) {
    try {
      const { blob, filename } = await fetchAuthedBlob(path);
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = filename || fallbackName || "download.bin";
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 1500);
    } catch (err) {
      showToast(err.message || "下载失败", "error");
    }
  }

  let previewLoadSeq = 0;
  async function loadAuthedPreviewInto(container, item) {
    if (!container || !item) return;
    const seq = ++previewLoadSeq;
    const baseMime = (item.media_type || "").split(";")[0].trim().toLowerCase();
    const isPdf = baseMime === "application/pdf";
    const isImg = ["image/png", "image/jpeg", "image/webp"].includes(baseMime);
    const isHtml = baseMime === "text/html";
    if (!isPdf && !isImg && !isHtml) return;
    container.innerHTML = `<div class="preview-loading" style="padding:24px;text-align:center;color:var(--ink-3);font-size:13px">原件加载中…</div>`;
    try {
      const { blob } = await fetchAuthedBlob(`/api/sources/${encodeURIComponent(item.source_id)}/raw`);
      if (seq !== previewLoadSeq) return;
      if (isHtml) {
        const buf = new Uint8Array(await blob.arrayBuffer());
        let htmlText;
        if (buf.length >= 2 && buf[0] === 0xFF && buf[1] === 0xFE) {
          htmlText = new TextDecoder("utf-16le").decode(buf);
        } else if (buf.length >= 2 && buf[0] === 0xFE && buf[1] === 0xFF) {
          htmlText = new TextDecoder("utf-16be").decode(buf);
        } else {
          try {
            htmlText = new TextDecoder("utf-8", { fatal: true }).decode(buf);
          } catch (_) {
            try { htmlText = new TextDecoder("gb18030").decode(buf); } catch (__) { htmlText = new TextDecoder("utf-8").decode(buf); }
          }
        }
        if (seq !== previewLoadSeq) return;
        const iframe = document.createElement("iframe");
        iframe.setAttribute("sandbox", "");
        iframe.title = "静态 HTML 原件";
        iframe.srcdoc = htmlText;
        container.innerHTML = "";
        container.appendChild(iframe);
      } else if (isPdf) {
        const pdfBlob = blob.type === "application/pdf" ? blob : new Blob([blob], { type: "application/pdf" });
        const url = trackBlobUrl(URL.createObjectURL(pdfBlob));
        const iframe = document.createElement("iframe");
        iframe.src = `${url}#navpanes=0&view=FitH`;
        iframe.title = "PDF 原件";
        container.innerHTML = "";
        container.appendChild(iframe);
      } else if (isImg) {
        const url = trackBlobUrl(URL.createObjectURL(blob));
        const img = document.createElement("img");
        img.src = url;
        img.alt = item.display_name || "原件截图";
        container.innerHTML = "";
        container.appendChild(img);
      }
    } catch (err) {
      if (seq !== previewLoadSeq) return;
      container.innerHTML = `<div class="preview-loading" style="padding:24px;text-align:center;color:var(--ink-3);font-size:13px">原件预览失败：${esc(err.message)}</div>`;
    }
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
  function themeIconFor(themeId) {
    if (themeId === "night") return ICO.moon;
    if (themeId === "paper") return ICO.paper;
    return ICO.sun;
  }

  function applyTheme(themeId) {
    const found = THEMES.find((t) => t.id === themeId) || THEMES[0];
    state.theme = found.id;
    document.documentElement.setAttribute("data-theme", found.id);
    if ($("lobby-theme-label")) $("lobby-theme-label").textContent = found.label;
    const lobbyBtnSvg = $("lobby-theme-btn")?.querySelector("svg");
    if (lobbyBtnSvg) lobbyBtnSvg.outerHTML = themeIconFor(found.id);
    const topBtn = $("btn-theme-cycle");
    if (topBtn) {
      topBtn.innerHTML = themeIconFor(found.id);
      topBtn.title = `当前：${found.label}外观（点击切换）`;
    }
    try { localStorage.setItem("subvault_ui_theme_v3", found.id); } catch (_) {}
  }

  // 外观切换：从被点击的按钮处以圆形扩散揭开新主题（浏览器支持 View Transitions 时）
  function cycleTheme(ev) {
    const idx = THEMES.findIndex((t) => t.id === state.theme);
    const next = THEMES[(idx + 1) % THEMES.length];
    const swap = () => {
      applyTheme(next.id);
      if (state.workspace) renderWorkspace();
    };
    const origin = (ev && ev.currentTarget && ev.currentTarget.getBoundingClientRect)
      ? ev.currentTarget
      : ($("screen-workspace").classList.contains("hidden") ? $("lobby-theme-btn") : $("btn-theme-cycle"));
    if (typeof document.startViewTransition === "function" && motionOK() && origin) {
      const r = origin.getBoundingClientRect();
      const x = r.left + r.width / 2;
      const y = r.top + r.height / 2;
      const endR = Math.hypot(Math.max(x, window.innerWidth - x), Math.max(y, window.innerHeight - y));
      const vt = document.startViewTransition(swap);
      vt.ready.then(() => {
        document.documentElement.animate(
          { clipPath: [`circle(0px at ${x}px ${y}px)`, `circle(${endR}px at ${x}px ${y}px)`] },
          { duration: 620, easing: "cubic-bezier(0.22, 1, 0.36, 1)", pseudoElement: "::view-transition-new(root)" },
        );
      }).catch(() => {});
    } else {
      swap();
    }
    showToast(`已切换为${next.label}外观`);
  }

  function togglePrivacy(forceVal) {
    const nextVal = forceVal !== undefined ? forceVal : !state.privacyBlur;
    if (nextVal !== state.privacyBlur && forceVal === undefined) runPrivacySweep();
    state.privacyBlur = nextVal;
    document.body.classList.toggle("privacy-blur", state.privacyBlur);
    const btn = $("btn-privacy-toggle");
    btn.classList.toggle("active", state.privacyBlur);
    btn.setAttribute("aria-pressed", String(state.privacyBlur));
    btn.innerHTML = state.privacyBlur ? ICO.eyeOff : ICO.eyeOpen;
    if (state.privacyBlur) showToast("已拉上磨砂玻璃：金额、账号和密钥已隐藏，鼠标放上去可临时查看");
    else if (forceVal === undefined) showToast("磨砂玻璃已收起，金额恢复显示");
  }

  // 一块磨砂玻璃从左往右扫过屏幕，金额按横向位置依次模糊 / 清晰，与玻璃同步
  function runPrivacySweep() {
    if (!motionOK()) return;
    const w = window.innerWidth;
    const vh = window.innerHeight;
    const touched = [];
    document.querySelectorAll(".sensitive").forEach((el) => {
      const r = el.getBoundingClientRect();
      if (!r.width || r.bottom < 0 || r.top > vh) return;
      el.style.transitionDelay = `${Math.round(Math.max(0, Math.min(1, r.left / w)) * 320)}ms`;
      touched.push(el);
    });
    const sweep = document.createElement("div");
    sweep.className = "privacy-sweep";
    document.body.appendChild(sweep);
    setTimeout(() => {
      sweep.remove();
      touched.forEach((el) => { el.style.transitionDelay = ""; });
    }, 820);
  }

  // --- Delight Layer: motion helpers, particles, vault door, living numbers ---
  const motionQuery = window.matchMedia ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
  function motionOK() { return !(motionQuery && motionQuery.matches); }
  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  function pulseClass(el, cls, ms) {
    if (!el) return;
    el.classList.remove(cls);
    void el.offsetWidth;
    el.classList.add(cls);
    setTimeout(() => el.classList.remove(cls), ms);
  }

  function coinSymbol(cur) {
    const s = String(CURRENCY_SYMBOLS[cur] || "¥").trim();
    const m = s.match(/[¥$€£₩]/);
    return m ? m[0] : s.slice(0, 1);
  }

  // 粒子系统：彩带、圆点、小票据与会翻转的金币，共用同一个 canvas 与动画循环
  const fx = { particles: [], running: false, dpr: 1 };

  function fxCanvas() {
    const canvas = $("confetti-canvas");
    if (!canvas) return null;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const w = window.innerWidth;
    const h = window.innerHeight;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;
    }
    fx.dpr = dpr;
    return canvas;
  }

  function drawParticle(ctx, p) {
    ctx.save();
    ctx.globalAlpha = Math.min(1, p.life / 22);
    ctx.translate(p.x, p.y);
    if (p.shape === "coin") {
      const flip = Math.cos((p.rot * Math.PI) / 180);
      ctx.scale(Math.max(0.14, Math.abs(flip)), 1);
      const r = p.size;
      const g = ctx.createRadialGradient(-r * 0.35, -r * 0.4, r * 0.1, 0, 0, r);
      g.addColorStop(0, "#fff4b8");
      g.addColorStop(0.55, "#f4c542");
      g.addColorStop(1, "#c88a0e");
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(0, 0, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = "rgba(140, 90, 6, 0.55)";
      ctx.lineWidth = 1;
      ctx.stroke();
      if (Math.abs(flip) > 0.4) {
        ctx.fillStyle = "rgba(118, 70, 0, 0.78)";
        ctx.font = `800 ${Math.round(r * 1.1)}px system-ui, sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(p.symbol || "¥", 0, r * 0.08);
      }
    } else if (p.shape === "receipt") {
      ctx.rotate((p.rot * Math.PI) / 180);
      const w = p.size * 1.6;
      const h = p.size * 2.2;
      ctx.fillStyle = "#ffffff";
      ctx.strokeStyle = "rgba(17, 19, 24, 0.16)";
      ctx.lineWidth = 1;
      ctx.fillRect(-w / 2, -h / 2, w, h);
      ctx.strokeRect(-w / 2, -h / 2, w, h);
      ctx.fillStyle = p.color;
      ctx.fillRect(-w / 2 + 2, -h / 2 + 2.5, w - 4, 2);
      ctx.fillStyle = "rgba(17, 19, 24, 0.24)";
      ctx.fillRect(-w / 2 + 2, -h / 2 + 6.5, w * 0.62, 1.2);
      ctx.fillRect(-w / 2 + 2, -h / 2 + 9.5, w * 0.44, 1.2);
    } else if (p.shape === "dot") {
      ctx.fillStyle = p.color;
      ctx.beginPath();
      ctx.arc(0, 0, p.size * 0.6, 0, Math.PI * 2);
      ctx.fill();
    } else {
      ctx.rotate((p.rot * Math.PI) / 180);
      ctx.scale(1, Math.cos(p.rot * 0.06));
      ctx.fillStyle = p.color;
      ctx.fillRect(-p.size, -p.size * 0.42, p.size * 2, p.size * 0.84);
    }
    ctx.restore();
  }

  function fxSpawn(list) {
    if (!motionOK() || !list.length) return;
    const canvas = fxCanvas();
    if (!canvas) return;
    fx.particles.push(...list);
    if (fx.running) return;
    fx.running = true;
    canvas.classList.remove("hidden");
    const ctx = canvas.getContext("2d");
    const tick = () => {
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      ctx.setTransform(fx.dpr, 0, 0, fx.dpr, 0, 0);
      const h = window.innerHeight;
      fx.particles = fx.particles.filter((p) => {
        p.vx *= p.drag;
        p.vy = p.vy * p.drag + p.g;
        p.x += p.vx;
        p.y += p.vy;
        p.rot += p.vr;
        p.life -= 1;
        if (p.life <= 0 || p.y > h + 40) return false;
        drawParticle(ctx, p);
        return true;
      });
      if (fx.particles.length) requestAnimationFrame(tick);
      else {
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        canvas.classList.add("hidden");
        fx.running = false;
      }
    };
    requestAnimationFrame(tick);
  }

  // 满屏庆祝：彩带 + 小票据 + 金币；coins:true 时变成金币雨
  function launchConfetti(opts = {}) {
    const w = window.innerWidth;
    const palette = getChartPalette();
    const symbol = opts.symbol || coinSymbol(state.heroCurrency || "CNY");
    const list = Array.from({ length: opts.count || 110 }, (_, i) => {
      const roll = Math.random();
      const shape = opts.coins ? "coin" : roll < 0.14 ? "coin" : roll < 0.26 ? "receipt" : roll < 0.42 ? "dot" : "ribbon";
      return {
        x: Math.random() * w,
        y: -20 - Math.random() * 180,
        vx: (Math.random() - 0.5) * 3,
        vy: 2 + Math.random() * 3.5,
        g: 0.05 + Math.random() * 0.05,
        drag: 0.996,
        rot: Math.random() * 360,
        vr: (Math.random() - 0.5) * 12,
        size: shape === "coin" ? 7 + Math.random() * 4 : 4 + Math.random() * 3.5,
        color: palette[i % palette.length],
        shape,
        symbol,
        life: 280,
      };
    });
    fxSpawn(list);
  }

  // 局部迸发：从某个点向上喷出一小把金币 / 彩点
  function burstAt(x, y, opts = {}) {
    const palette = getChartPalette();
    const list = Array.from({ length: opts.count || 16 }, (_, i) => {
      const a = -Math.PI / 2 + (Math.random() - 0.5) * (opts.spread || 2.3);
      const sp = 3.5 + Math.random() * 4.5;
      const shape = opts.shape || (i % 3 === 0 ? "coin" : i % 3 === 1 ? "dot" : "ribbon");
      return {
        x, y,
        vx: Math.cos(a) * sp,
        vy: Math.sin(a) * sp,
        g: 0.22,
        drag: 0.97,
        rot: Math.random() * 360,
        vr: (Math.random() - 0.5) * 18,
        size: shape === "coin" ? 6 + Math.random() * 3 : 3 + Math.random() * 2.5,
        color: palette[i % palette.length],
        shape,
        symbol: opts.symbol || "¥",
        life: 62 + Math.random() * 22,
      };
    });
    fxSpawn(list);
  }

  function burstFrom(el, opts) {
    if (!el || !el.getBoundingClientRect) return;
    const r = el.getBoundingClientRect();
    if (!r.width) return;
    burstAt(r.left + r.width / 2, r.top + r.height / 2, opts);
  }

  // 字符“解码”效果：随机字符逐位落定为最终文本（不泄露任何真实字符；重复触发自动取消上一帧）
  const scrambleFrames = new WeakMap();
  function scrambleText(el, finalText, dur = 650) {
    if (!el) return;
    const prevRaf = scrambleFrames.get(el);
    if (prevRaf) cancelAnimationFrame(prevRaf);
    if (!motionOK()) { el.textContent = finalText; return; }
    const pool = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    const chars = Array.from(finalText);
    const t0 = performance.now();
    const step = (now) => {
      if (!el.isConnected) return;
      const p = Math.min(1, (now - t0) / dur);
      const settled = Math.floor(p * chars.length);
      el.textContent = chars.map((c, i) => (i < settled || /[\s·\-•]/.test(c) ? c : pool[Math.floor(Math.random() * pool.length)])).join("");
      if (p < 1) scrambleFrames.set(el, requestAnimationFrame(step));
      else { el.textContent = finalText; scrambleFrames.delete(el); }
    };
    scrambleFrames.set(el, requestAnimationFrame(step));
  }

  // 数字滚动：同一个 key 只在数值变化时从旧值滚到新值，搜索时的重复渲染不会重播
  const countMemory = new Map();
  function runCountUps(root) {
    if (!root) return;
    root.querySelectorAll("[data-countup]").forEach((el) => {
      const key = el.getAttribute("data-countup");
      const to = Number(el.getAttribute("data-cents")) || 0;
      const from = countMemory.has(key) ? countMemory.get(key) : 0;
      countMemory.set(key, to);
      if (from === to || !motionOK()) return;
      const cur = el.getAttribute("data-cur") || "";
      const mode = el.getAttribute("data-count-mode") || "editorial";
      const withCode = el.getAttribute("data-code") === "1";
      const paint = (c) => (mode === "short"
        ? esc(formatMoneyShort(cur, centsToText(c)))
        : renderEditorialMoney(cur, centsToText(c), withCode));
      const finalHtml = el.innerHTML;
      const dur = Math.min(1150, 480 + Math.log10(Math.abs(to - from) + 10) * 110);
      const t0 = performance.now();
      el.classList.add("is-counting");
      const step = (now) => {
        if (!el.isConnected) return;
        const p = Math.min(1, (now - t0) / dur);
        const eased = 1 - Math.pow(1 - p, 4);
        if (p < 1) {
          el.innerHTML = paint(Math.round(from + (to - from) * eased));
          requestAnimationFrame(step);
        } else {
          el.innerHTML = finalHtml;
          el.classList.remove("is-counting");
        }
      };
      requestAnimationFrame(step);
    });
  }

  // 场景切换：只有切换视图 / 分类 / 月份时才播放入场错落动画，日常重渲染保持安静
  let sceneKey = "";
  let sceneTimer = null;
  function markScene(key) {
    if (key === sceneKey) return;
    sceneKey = key;
    if (!motionOK()) return;
    document.body.classList.remove("scene-enter");
    void document.body.offsetWidth;
    document.body.classList.add("scene-enter");
    clearTimeout(sceneTimer);
    sceneTimer = setTimeout(() => document.body.classList.remove("scene-enter"), 1500);
  }

  const STAGGER_SELECTORS = [
    ".object-gallery > .obj-card", ".filing-strip", ".clean-table tbody tr", ".signal-tile", ".agenda-row",
    ".vendor-tile", ".sim-row", ".bar-col", ".legend-row", ".attn-row", ".csv-ws-card",
  ];
  function assignStagger(root) {
    if (!root) return;
    STAGGER_SELECTORS.forEach((sel) => {
      root.querySelectorAll(sel).forEach((el, i) => el.style.setProperty("--i", String(Math.min(i, 16))));
    });
  }

  // 卡片 3D 倾斜 + 跟随鼠标的高光；买断密钥卡片额外带一层镭射膜
  function bindTiltEffects() {
    let active = null;
    const reset = (el) => {
      if (!el) return;
      el.classList.remove("is-tilting");
      el.style.removeProperty("--rx");
      el.style.removeProperty("--ry");
    };
    document.addEventListener("pointermove", (e) => {
      if (e.pointerType && e.pointerType !== "mouse") return;
      const card = motionOK() && e.target.closest ? e.target.closest(".obj-card, .mini-artifact") : null;
      if (active && active !== card) reset(active);
      active = card;
      if (!card) return;
      const r = card.getBoundingClientRect();
      const px = (e.clientX - r.left) / r.width;
      const py = (e.clientY - r.top) / r.height;
      const strength = card.classList.contains("mini-artifact") ? 10 : 6;
      card.style.setProperty("--rx", `${((0.5 - py) * strength).toFixed(2)}deg`);
      card.style.setProperty("--ry", `${((px - 0.5) * strength * 1.3).toFixed(2)}deg`);
      card.style.setProperty("--gx", `${(px * 100).toFixed(1)}%`);
      card.style.setProperty("--gy", `${(py * 100).toFixed(1)}%`);
      card.classList.add("is-tilting");
    }, { passive: true });
    document.addEventListener("pointerleave", () => { reset(active); active = null; });
    window.addEventListener("blur", () => { reset(active); active = null; });
  }

  // 顶部导航的滑块：在各个入口之间弹性滑动
  function moveDockGlider(instant) {
    const dock = document.querySelector(".floating-dock");
    const glider = dock && dock.querySelector(".dock-glider");
    const activeBtn = dock && dock.querySelector(".dock-item.active");
    if (!glider || !activeBtn || !activeBtn.offsetWidth) return;
    const jump = instant || !dock.classList.contains("has-glider") || !motionOK();
    if (jump) glider.style.transition = "none";
    glider.style.width = `${activeBtn.offsetWidth}px`;
    glider.style.transform = `translateX(${activeBtn.offsetLeft}px)`;
    if (jump) {
      void glider.offsetWidth;
      glider.style.transition = "";
    }
    dock.classList.add("has-glider");
  }

  // 金库大门：合拢 → 转盘转动 → 在门后切换界面 → 打开
  let doorBusy = false;
  async function playVaultDoor(swap, locking = false) {
    const door = $("vault-door");
    if (!door || !motionOK() || doorBusy) { swap(); return; }
    doorBusy = true;
    try {
      door.dataset.mode = locking ? "lock" : "unlock";
      door.classList.remove("hidden", "closing", "spin", "open");
      void door.offsetWidth;
      door.classList.add("closing");
      await wait(400);
      door.classList.add("spin");
      await wait(480);
      swap();
      door.classList.add("open");
      await wait(680);
    } finally {
      door.classList.add("hidden");
      door.classList.remove("closing", "spin", "open");
      doorBusy = false;
    }
  }

  // 导入时的小纸片：从落点沿弧线飞进左上角的账本胶囊
  function paperTint(name) {
    const ext = (String(name || "").split(".").pop() || "").toLowerCase();
    if (ext === "pdf") return "#e11d48";
    if (ext === "csv" || ext === "txt") return "#059669";
    if (ext.startsWith("htm")) return "#2563eb";
    if (["png", "jpg", "jpeg", "webp"].includes(ext)) return "#7c3aed";
    return "#8a94a6";
  }

  function flyPapersInto(fromX, fromY, files) {
    if (!motionOK()) return;
    const target = $("vault-pill-btn");
    if (!target || !target.offsetParent) return;
    const tr = target.getBoundingClientRect();
    const tx = tr.left + 20;
    const ty = tr.top + tr.height / 2;
    Array.from(files).slice(0, 8).forEach((f, i) => {
      const el = document.createElement("div");
      el.className = "fly-paper";
      el.style.setProperty("--tint", paperTint(f && f.name));
      el.style.left = `${fromX - 13}px`;
      el.style.top = `${fromY - 17}px`;
      document.body.appendChild(el);
      const jitter = (Math.random() - 0.5) * 80;
      const dx = tx - fromX;
      const dy = ty - fromY;
      const r0 = (Math.random() - 0.5) * 36;
      const anim = el.animate([
        { transform: `translate(${jitter * 0.3}px, 8px) rotate(${r0}deg) scale(0.5)`, opacity: 0 },
        { transform: `translate(${jitter}px, -30px) rotate(${r0 * 1.3}deg) scale(1)`, opacity: 1, offset: 0.2 },
        { transform: `translate(${dx * 0.55 + jitter}px, ${dy * 0.55 - 90}px) rotate(${-r0}deg) scale(0.85)`, opacity: 1, offset: 0.62 },
        { transform: `translate(${dx}px, ${dy}px) rotate(${-r0 * 2}deg) scale(0.16)`, opacity: 0.25 },
      ], { duration: 860, delay: i * 90, easing: "cubic-bezier(0.45, 0, 0.55, 1)", fill: "both" });
      anim.onfinish = () => { el.remove(); pulseClass(target, "gulp", 440); };
    });
  }

  function importOriginFor(el) {
    if (!el || !el.offsetParent) return null;
    const r = el.getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  }

  // 新加入账本的卡片：短暂发光并在必要时滚动到视野内
  function itemIdSet() {
    return new Set((state.workspace?.items || []).map((i) => i.source_id));
  }

  function highlightNewItems(prevIds) {
    if (!prevIds) return;
    const fresh = (state.workspace?.items || []).filter((i) => !prevIds.has(i.source_id));
    if (!fresh.length) return;
    requestAnimationFrame(() => {
      let first = null;
      fresh.forEach((it) => {
        const el = $("ledger-stage").querySelector(`[data-source-id="${CSS.escape(it.source_id)}"]`);
        if (el) {
          pulseClass(el, "just-added", 2300);
          if (!first) first = el;
        }
      });
      if (first && state.activeNav === "assets") {
        const r = first.getBoundingClientRect();
        if (r.top > window.innerHeight - 40 || r.bottom < 80) {
          first.scrollIntoView({ behavior: motionOK() ? "smooth" : "auto", block: "center" });
        }
      }
    });
  }

  function flashButtonSuccess(btn, okText, restoreText) {
    btn.disabled = false;
    btn.classList.add("btn-success");
    btn.innerHTML = `${ICO.check} ${esc(okText)}`;
    setTimeout(() => {
      btn.classList.remove("btn-success");
      btn.textContent = restoreText;
    }, 1400);
  }

  // 订阅花费的“生活换算”：点一下换一种说法
  const FUN_EQUIV = {
    CNY: [{ unit: "杯奶茶", cents: 1800 }, { unit: "张电影票", cents: 4500 }, { unit: "份煎饼果子", cents: 800 }, { unit: "顿火锅", cents: 12000 }],
    USD: [{ unit: "杯拿铁", cents: 550 }, { unit: "张电影票", cents: 1500 }, { unit: "块披萨", cents: 350 }, { unit: "本平装书", cents: 1800 }],
    EUR: [{ unit: "杯咖啡", cents: 350 }, { unit: "张电影票", cents: 1200 }, { unit: "个可颂", cents: 150 }],
    GBP: [{ unit: "杯咖啡", cents: 340 }, { unit: "张电影票", cents: 1100 }, { unit: "品脱啤酒", cents: 500 }],
    JPY: [{ unit: "碗拉面", cents: 100000 }, { unit: "张电影票", cents: 190000 }, { unit: "个饭团", cents: 16000 }],
    HKD: [{ unit: "杯奶茶", cents: 2200 }, { unit: "张电影票", cents: 9000 }, { unit: "个菠萝油", cents: 1400 }],
  };

  function funFactsForAll(totals) {
    if (!totals || !totals.length) return ["还没有周期订阅，钱包表示很安心"];
    const dailyParts = totals.map(([cur, c]) => formatMoneyShort(cur, centsToText(Math.max(1, Math.round((c * 12) / 365)))));
    const yearlyParts = totals.map(([cur, c]) => formatMoneyShort(cur, centsToText(c * 12)));
    const facts = [`按周期折算，每天约 ${dailyParts.join(" + ")}`];
    totals.forEach(([cur, monthlyCents]) => {
      if (!monthlyCents || monthlyCents <= 0) return;
      const yearly = monthlyCents * 12;
      const curLabel = totals.length > 1 ? (CURRENCY_NAMES[cur] || cur) : "";
      (FUN_EQUIV[cur] || []).slice(0, totals.length > 1 ? 2 : 4).forEach((eq, i) => {
        const monthlyView = i % 2 === 0;
        const n = (monthlyView ? monthlyCents : yearly) / eq.cents;
        const nTxt = n >= 10 ? String(Math.round(n)) : n.toFixed(1).replace(/\.0$/, "");
        facts.push(`${monthlyView ? "每月" : "一年"}${curLabel}订阅 ≈ ${nTxt} ${eq.unit}`);
      });
    });
    facts.push(`按周期折算，一年约 ${yearlyParts.join(" + ")}`);
    return facts;
  }

  // 主密码“锁芯”：密码越强，顶起的弹子越多
  function passStrengthLevel(p) {
    if (!p) return 0;
    if (p.length < 6) return 1;
    let s = 2;
    if (p.length >= 10) s += 1;
    if (/[a-z]/i.test(p) && /\d/.test(p)) s += 1;
    if (/[^A-Za-z0-9]/.test(p) || (/[a-z]/.test(p) && /[A-Z]/.test(p))) s += 1;
    if (p.length >= 14) s += 1;
    return Math.min(5, s);
  }

  const TUMBLER_LABELS = ["还没设置", "太短了，至少 6 位", "能用，再长一点更好", "还不错", "很牢靠", "固若金汤"];
  function updateTumblers() {
    const meter = $("pass-tumblers");
    if (!meter) return;
    const p = $("create-passphrase").value;
    const c = $("create-passphrase-confirm").value;
    const lvl = passStrengthLevel(p);
    meter.className = `tumbler-meter lvl-${lvl}`;
    meter.querySelectorAll(".tm-pin").forEach((pin, i) => pin.classList.toggle("set", i < lvl));
    $("tm-label").textContent = `锁芯 ${lvl}/5 · ${TUMBLER_LABELS[lvl]}`;
    const match = $("tm-match");
    match.className = "tm-match";
    if (!c) {
      match.textContent = lvl >= 5 ? "这把锁连自己都撬不开，记得保存恢复密钥" : "密码越长，锁芯里顶起的弹子越多";
    } else if (c === p) {
      match.textContent = "两次输入一致 ✓";
      match.classList.add("ok");
    } else if (p.startsWith(c)) {
      match.textContent = "继续输入确认密码…";
    } else {
      match.textContent = "两次输入不一致";
      match.classList.add("bad");
    }
  }

  function celebrateReimburse(sourceId, status) {
    if (!motionOK()) return;
    const sel = CSS.escape(sourceId);
    const btn = document.querySelector(`#ledger-stage [data-reimb-sid="${sel}"]`);
    if (btn) pulseClass(btn, "just-changed", 520);
    if (status !== "reimbursed") return;
    const card = document.querySelector(`#ledger-stage .ticket[data-ticket-sid="${sel}"], #ledger-stage .obj-card[data-source-id="${sel}"]`);
    if (card) {
      const stamp = document.createElement("div");
      stamp.className = "stamp-slam";
      stamp.innerHTML = "已到账<small>REIMBURSED</small>";
      card.appendChild(stamp);
      setTimeout(() => pulseClass(card, "stamp-thud", 360), 240);
      setTimeout(() => stamp.remove(), 1950);
    }
    const item = (state.workspace?.items || []).find((i) => i.source_id === sourceId);
    burstFrom(btn || card, { count: 14, shape: "coin", symbol: coinSymbol(item?.amount_fact?.value?.currency || "CNY") });
  }

  // --- Metric Helpers ---
  function localTodayStr() {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  }

  function localCalendarDaysDiff(dateStr) {
    if (!dateStr || !/^\d{4}-\d{2}-\d{2}$/.test(dateStr)) return null;
    const [y, m, d] = dateStr.split("-").map(Number);
    const now = new Date();
    const targetUtc = Date.UTC(y, m - 1, d);
    const todayUtc = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
    return Math.round((targetUtc - todayUtc) / 86400000);
  }

  function maskLicenseKey(key) {
    const s = String(key || "").trim();
    if (!s) return "";
    return s.length <= 4 ? "••••" : `•••• · •••• · ${s.slice(-4)}`;
  }

  function periodProgressInfo(perVal) {
    if (!perVal || !perVal.period_start || !perVal.period_end) return null;
    const s = new Date(`${perVal.period_start}T00:00:00`);
    const e = new Date(`${perVal.period_end}T00:00:00`);
    if (isNaN(s.getTime()) || isNaN(e.getTime()) || e <= s) return null;
    const daysLeft = localCalendarDaysDiff(perVal.period_end);
    const totalDays = Math.max(1, Math.round((e - s) / 86400000));
    const elapsedDays = totalDays - daysLeft;
    const pct = Math.max(0, Math.min(100, Math.round((elapsedDays / totalDays) * 100)));
    const statusLabel = daysLeft > 0 ? `本期还剩 ${daysLeft} 天` : `账单期截至 ${perVal.period_end}`;
    return { pct, periodEnded: daysLeft <= 0, range: `${perVal.period_start} — ${perVal.period_end}`, statusLabel };
  }

  function warrantyInfo(buyDate, warrantyEnd) {
    const daysLeft = localCalendarDaysDiff(warrantyEnd);
    if (daysLeft === null) return null;
    let remPct = daysLeft <= 0 ? 0 : 65;
    if (buyDate && /^\d{4}-\d{2}-\d{2}$/.test(buyDate)) {
      const start = new Date(`${buyDate}T00:00:00`);
      const end = new Date(`${warrantyEnd}T00:00:00`);
      if (!isNaN(start.getTime()) && !isNaN(end.getTime()) && end > start) {
        const totalDays = Math.max(1, Math.round((end - start) / 86400000));
        remPct = Math.max(0, Math.min(100, Math.round((daysLeft / totalDays) * 100)));
      }
    }
    const color = daysLeft < 0 ? "var(--ink-3)" : daysLeft <= 60 ? "var(--amber)" : "var(--iris)";
    const statusLabel = daysLeft > 0 ? `保修剩 ${daysLeft} 天` : daysLeft === 0 ? "今日到期" : `已过保 ${Math.abs(daysLeft)} 天`;
    const shortTag = daysLeft > 0 ? `${daysLeft}d` : "完";
    return { daysLeft, remPct, color, statusLabel, shortTag, warrantyEnd };
  }

  function costPerDayInfo(buyDate, currency, amountText) {
    if (!buyDate || !/^\d{4}-\d{2}-\d{2}$/.test(buyDate) || !amountText) return null;
    const diff = localCalendarDaysDiff(buyDate);
    if (diff === null || diff > 0) return null;
    const daysUsed = Math.max(1, -diff);
    const cents = amountToCents(amountText);
    if (cents <= 0) return null;
    const perDayCents = Math.max(1, Math.round(cents / daysUsed));
    return { daysUsed, perDayText: formatMoneyShort(currency, centsToText(perDayCents)) };
  }

  function renderRingGauge(pct, color, label) {
    const r = 17;
    const circ = 2 * Math.PI * r;
    const dash = Math.max(2, (Math.max(0, Math.min(100, pct)) / 100) * circ);
    return `
      <div class="ring-gauge">
        <svg width="44" height="44" viewBox="0 0 44 44">
          <circle cx="22" cy="22" r="${r}" fill="none" stroke="var(--recessed)" stroke-width="4" />
          <circle class="rg-progress" cx="22" cy="22" r="${r}" fill="none" stroke="${color}" stroke-width="4" stroke-linecap="round" stroke-dasharray="${dash.toFixed(1)} ${circ.toFixed(1)}" />
        </svg>
        <div class="ring-gauge-lbl" style="color:${color}">${esc(label)}</div>
      </div>
    `;
  }

  function renderTickMeter(pct, expired) {
    const total = 24;
    const filled = Math.max(1, Math.round((pct / 100) * total));
    const cls = expired ? "filled muted" : "filled blue";
    let html = `<div class="tick-meter">`;
    for (let i = 0; i < total; i++) {
      html += `<span class="tick-bar ${i < filled ? cls : ""}" style="--t:${i}"></span>`;
    }
    html += `</div>`;
    return html;
  }

  async function copyLicenseKey(vendor, key, btnEl) {
    try {
      await navigator.clipboard.writeText(key);
      if (btnEl) {
        const plateText = btnEl.closest(".serial-plate")?.querySelector("span");
        if (plateText) scrambleText(plateText, maskLicenseKey(key), 620);
        const orig = btnEl.innerHTML;
        btnEl.classList.add("copied");
        btnEl.innerHTML = `${ICO.check} 已复制`;
        setTimeout(() => { btnEl.classList.remove("copied"); btnEl.innerHTML = orig; }, 1600);
      }
      showToast("已复制密钥", "ok");
    } catch (_) {
      showToast("复制失败，请点开详情手动复制", "error");
    }
  }

  async function cycleReimburseStatus(sourceId, currentStatus) {
    const cur = REIMBURSE_META[currentStatus] ? currentStatus : "pending";
    await setReimburseStatus(sourceId, REIMBURSE_META[cur].next);
  }

  async function setReimburseStatus(sourceId, next) {
    try {
      const snap = await api("POST", "/api/sources/reimburse", { source_id: sourceId, reimburse_status: next });
      state.workspace = snap;
      renderWorkspace();
      celebrateReimburse(sourceId, next);
      showToast(next === "reimbursed" ? "啪！盖章到账，这笔钱回来了" : `已移到「${REIMBURSE_META[next].label}」`, "ok");
    } catch (err) {
      showToast(err.message, "error");
    }
  }

  // --- Bootstrap & Lobby ---
  async function boot() {
    try {
      applyTheme(localStorage.getItem("subvault_ui_theme_v3") || "gallery");
    } catch (_) {
      applyTheme("gallery");
    }
    bindEvents();
    if (!SESSION_TOKEN) {
      showToast("本地会话凭证已失效，请重新打开 SubVault 终端输出的链接或重启 subvault-app。", "error");
      showLobbyScreen();
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
    sel.innerHTML = list.map((c) => `<option value="${esc(c)}">${esc(c)} ${esc(CURRENCY_SYMBOLS[c] || "")}</option>`).join("");
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
    const box = $("recent-vaults-list");
    const empty = $("recent-vaults-empty");
    if (!recent.length) { box.innerHTML = ""; empty.classList.remove("hidden"); return; }
    empty.classList.add("hidden");
    box.innerHTML = recent.map((v, idx) => `
      <button type="button" class="recent-item" data-recent-idx="${idx}">
        <div style="min-width:0">
          <div class="recent-item-name">${esc(v.name)}</div>
          <div class="recent-item-path">${esc(v.path)}</div>
        </div>
        <span class="micro-pill ${v.needs_passphrase ? "green" : "slate"}">${v.needs_passphrase ? "已加密" : "系统盘"}</span>
      </button>
    `).join("");
    box.querySelectorAll("[data-recent-idx]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const target = recent[Number(btn.getAttribute("data-recent-idx"))];
        if (target) { setLobbyTab("unlock"); selectRecentVault(target); }
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

  function resetVaultUiState() {
    state.workspace = null;
    state.groups = [];
    state.activeNav = "assets";
    state.collection = "all";
    state.heroCurrency = "";
    state.reimburseFilter = "";
    state.accountFilter = "";
    state.currencyFilter = "";
    state.categoryFilter = "";
    state.searchQuery = "";
    state.insightsCurrency = "";
    state.simCutIds.clear();
    state.triageSkippedIds.clear();
    state.csvUiState.clear();
    state.selectedSourceId = null;
    state.textViewActive = false;
    state.lastRedactedText = "";
    countMemory.clear();
    revokeBlobUrls();
    if ($("search-box")) $("search-box").value = "";
    if ($("filter-account")) $("filter-account").value = "";
    if ($("chk-zip-ack")) $("chk-zip-ack").checked = false;
    if ($("chk-dir-ack")) $("chk-dir-ack").checked = false;
    if ($("export-dir-result")) $("export-dir-result").classList.add("hidden");
    if ($("verify-dir-result")) $("verify-dir-result").classList.add("hidden");
    if ($("rotate-current-pass")) $("rotate-current-pass").value = "";
    if ($("rotate-new-pass")) $("rotate-new-pass").value = "";
    if ($("rotate-new-pass-confirm")) $("rotate-new-pass-confirm").value = "";
    if ($("chk-rotate-recovery")) $("chk-rotate-recovery").checked = false;
    closeDrawer();
  }

  function showLobbyScreen() {
    sceneKey = "";
    $("screen-lobby").classList.remove("hidden");
    $("screen-workspace").classList.add("hidden");
    closeDrawer();
  }

  function showWorkspaceScreen() {
    sceneKey = "";
    $("screen-lobby").classList.add("hidden");
    $("screen-workspace").classList.remove("hidden");
    renderWorkspace();
    requestAnimationFrame(() => moveDockGlider(true));
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

  function navigateTo(navKey, collectionKey) {
    const navChanged = state.activeNav !== navKey;
    state.activeNav = navKey;
    if (navKey === "assets" && collectionKey !== undefined) state.collection = collectionKey;

    document.querySelectorAll("[data-nav]").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-nav") === navKey);
    });
    moveDockGlider();
    ["assets", "calendar", "insights", "triage", "security"].forEach((name) => {
      $(`pane-${name}`).classList.toggle("hidden", name !== navKey);
    });
    if (navChanged && motionOK()) pulseClass($(`pane-${navKey}`), "pane-enter", 420);
    if (navKey === "security") markScene("security");

    if (state.workspace) {
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
    state.groups = buildAssetGroups(ws);
    const subGroupCount = state.groups.filter((g) => g.kind === "subscription").length;
    const triageCount = getTriageQueue(ws).length;
    const attnCount = (v.duplicate_count || 0) + (v.conflict_count || 0) + (v.needs_entry_count || 0);

    $("mh-vault-name").textContent = v.name || "我的账本";
    $("mh-vault-dot").className = `vp-dot ${v.encrypted ? "green" : "slate"}`;
    $("vp-vault-status").textContent = v.encrypted ? "本地加密 · 离线运行" : "系统盘免密 · 离线运行";
    $("vp-vault-path").textContent = v.path || "";

    $("zip-ack-row").classList.toggle("hidden", !v.encrypted);
    $("dir-ack-row").classList.toggle("hidden", !v.encrypted);
    $("card-security-rotate").classList.toggle("hidden", !v.encrypted);

    $("cnt-all").textContent = String((ws.items || []).length);
    $("cnt-sub").textContent = subGroupCount > 0 ? String(subGroupCount) : "";
    $("cnt-lic").textContent = ac.license > 0 ? String(ac.license) : "";
    $("cnt-hw").textContent = ac.hardware > 0 ? String(ac.hardware) : "";
    $("cnt-reimb").textContent = ac.reimbursable > 0 ? String(ac.reimbursable) : "";
    $("cnt-csv").textContent = v.csv_statement_count > 0 ? String(v.csv_statement_count) : "";

    const triEl = $("cnt-triage");
    triEl.textContent = String(triageCount);
    triEl.classList.toggle("hidden", triageCount === 0);

    const attnEl = $("cnt-attention");
    attnEl.textContent = String(attnCount);
    attnEl.classList.toggle("hidden", attnCount === 0);

    syncAccountDropdown(ws);
    renderAssetsPane(ws);
    if (state.activeNav === "calendar") renderCalendarPane(ws);
    if (state.activeNav === "insights") renderInsightsPane(ws);
    if (state.activeNav === "triage") renderTriagePane(ws);
    if (state.activeNav === "security") renderSecurityAttention(ws);
    moveDockGlider();
  }

  function syncAccountDropdown(ws) {
    const sel = $("filter-account");
    sel.innerHTML = `<option value="">所有账号</option>` +
      (ws.accounts || []).map((a) => `<option value="${esc(a)}">${esc(a)}</option>`).join("");
    sel.value = state.accountFilter;
    // 只有一个（或没有）账号时，筛选毫无意义，直接隐藏
    sel.classList.toggle("hidden", (ws.accounts || []).length <= 1 && !state.accountFilter);
  }

  // --- Asset grouping: bills of the same subscription collapse into one card ---
  const RECURRING_MONTHLY = { monthly: 1, quarterly: 1 / 3, yearly: 1 / 12, weekly: 52 / 12 };

  function itemDate(it) {
    return it.annotation?.bill_date || it.period_fact?.value?.period_start || "";
  }

  function isoOf(d) {
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  }

  function shiftDate(iso, cycle, n) {
    const [y, m, d] = iso.split("-").map(Number);
    if (cycle === "weekly") return isoOf(new Date(y, m - 1, d + 7 * n));
    const months = cycle === "monthly" ? n : cycle === "quarterly" ? 3 * n : 12 * n;
    const total = y * 12 + (m - 1) + months;
    const ny = Math.floor(total / 12);
    const nm = ((total % 12) + 12) % 12;
    const dim = new Date(ny, nm + 1, 0).getDate();
    return isoOf(new Date(ny, nm, Math.min(d, dim)));
  }

  // 按账单写明的周期计算参考节奏（不伪造“已生效/已停用”状态）
  function nextRenewalInfo(item) {
    const ann = item.annotation || {};
    const cycle = ann.billing_cycle;
    if (!RECURRING_MONTHLY[cycle]) return null;
    const per = item.period_fact?.value;
    const today = localTodayStr();
    const anchor = per?.period_end || ann.bill_date || per?.period_start;
    if (!anchor || !/^\d{4}-\d{2}-\d{2}$/.test(anchor)) return null;
    let date = anchor;
    let steps = 0;
    while (date < today && steps < 600) { steps += 1; date = shiftDate(anchor, cycle, steps); }
    if (steps > 2) return { stale: true, last: anchor };
    const prev = per?.period_start && per.period_end === date ? per.period_start : shiftDate(date, cycle, -1);
    const daysLeft = localCalendarDaysDiff(date);
    const span = Math.max(1, localCalendarDaysDiff(date) - localCalendarDaysDiff(prev));
    const pct = Math.max(0, Math.min(100, Math.round(((span - daysLeft) / span) * 100)));
    return { stale: false, date, daysLeft, pct };
  }

  function subscriptionIdentityKey(it) {
    const ann = it.annotation || {};
    const amt = it.amount_fact?.value;
    return [
      "subscription",
      (ann.vendor || it.display_name || "").trim().toLowerCase(),
      (ann.plan_name || "").trim().toLowerCase(),
      (ann.account_label || "").trim().toLowerCase(),
      ann.billing_cycle || "",
      amt?.currency || "",
    ].join("|");
  }

  function buildAssetGroups(ws) {
    const map = new Map();
    for (const it of ws.items || []) {
      if (it.status_flags?.is_csv_statement) continue;
      const ann = it.annotation || {};
      const kind = ann.asset_kind || "general";
      const key = kind === "subscription"
        ? subscriptionIdentityKey(it)
        : `${kind}|${it.source_id}`;
      if (!map.has(key)) map.set(key, { key, kind, items: [] });
      map.get(key).items.push(it);
    }
    return [...map.values()].map((g) => {
      g.items.sort((a, b) => itemDate(b).localeCompare(itemDate(a)));
      const latest = g.items[0];
      g.latest = latest;
      g.cycle = latest.annotation?.billing_cycle || "unknown";
      g.amount = latest.amount_fact?.value || null;
      g.cents = g.amount ? amountToCents(g.amount.amount_text) : 0;
      g.renewal = g.kind === "subscription" ? nextRenewalInfo(latest) : null;
      g.monthlyCents = g.kind === "subscription" && g.amount && RECURRING_MONTHLY[g.cycle] ? Math.round(g.cents * RECURRING_MONTHLY[g.cycle]) : 0;
      return g;
    });
  }

  function groupOfItem(sourceId) {
    return (state.groups || []).find((g) => g.items.some((i) => i.source_id === sourceId)) || null;
  }

  // 每月周期折算：每个独立订阅只取最新一张账单，按币种分开，不依本机时钟私自剔除
  function recurringTotals(groups) {
    const out = {};
    (groups || []).forEach((g) => {
      if (!g.monthlyCents) return;
      out[g.amount.currency] = (out[g.amount.currency] || 0) + g.monthlyCents;
    });
    return Object.entries(out).sort((a, b) => b[1] - a[1]);
  }

  function reimbStatusOf(it) {
    const ann = it.annotation || {};
    if (ann.reimburse_status && ann.reimburse_status !== "none") return ann.reimburse_status;
    return ann.asset_kind === "reimbursable" ? "pending" : null;
  }

  function relDayLabel(days) {
    if (days === 0) return "今天";
    if (days === 1) return "明天";
    if (days < 0) return `${-days} 天前`;
    return `${days} 天后`;
  }

  function moneyListText(byCur) {
    return Object.entries(byCur).filter(([, c]) => c > 0).map(([cur, c]) => formatMoneyShort(cur, centsToText(c))).join(" + ");
  }

  // --- Reimbursements: three-column kanban with drag & drop ---
  const KANBAN_COLS = [
    { status: "pending", label: "待报销", pill: "amber" },
    { status: "submitted", label: "已提交", pill: "blue" },
    { status: "reimbursed", label: "已到账", pill: "green" },
  ];

  function renderReimburseKanban(list, stage) {
    const items = list.filter((it) => reimbStatusOf(it));
    const ticket = (it) => {
      const ann = it.annotation || {};
      const a = it.amount_fact?.value;
      const st = reimbStatusOf(it);
      const brand = brandStyleFor(ann.vendor || it.display_name);
      const nextSt = REIMBURSE_META[st].next;
      return `
        <article class="ticket" draggable="true" data-ticket-sid="${esc(it.source_id)}" data-source-id="${esc(it.source_id)}">
          <div class="tk-top">
            <span class="brand-badge brand-badge-sm" style="background:${brand.bg}">${esc(brand.mark)}</span>
            <span class="tk-name"><strong>${esc(ann.vendor || it.display_name)}</strong><small>${esc(ann.plan_name || ann.category || "")}</small></span>
          </div>
          <div class="tk-amt sensitive num">${a?.amount_text ? renderEditorialMoney(a.currency, a.amount_text, !CURRENCY_SYMBOLS[a.currency]) : "缺少金额"}</div>
          <div class="tk-meta">${esc([ann.bill_date, ann.notes].filter(Boolean).join(" · ").slice(0, 48))}</div>
          <div class="tk-perf"></div>
          <div class="tk-foot">
            <span class="sensitive">${esc(ann.account_label || "未分组")}</span>
            ${st !== "reimbursed" ? `<button type="button" class="tk-next" data-reimb-sid="${esc(it.source_id)}" data-reimb-cur="${esc(st)}">${esc(REIMBURSE_META[nextSt].label)} ${ICO.arrowRight}</button>` : `<span class="tk-done">${ICO.check} 已到账</span>`}
          </div>
        </article>`;
    };
    const colsHtml = KANBAN_COLS.map((col) => {
      const colItems = items.filter((it) => reimbStatusOf(it) === col.status);
      const sum = {};
      colItems.forEach((it) => { const a = it.amount_fact?.value; if (a?.amount_text) sum[a.currency] = (sum[a.currency] || 0) + amountToCents(a.amount_text); });
      return `
        <section class="kb-col" data-kb-status="${col.status}">
          <header class="kb-head">
            <span class="micro-pill ${col.pill}">${col.label}</span>
            <span class="kb-sum sensitive">${esc(moneyListText(sum))}</span>
            <span class="kb-n">${colItems.length}</span>
          </header>
          <div class="kb-list">${colItems.map(ticket).join("") || `<div class="kb-empty">拖到这里</div>`}</div>
        </section>`;
    }).join("");
    stage.innerHTML = `<div class="kb-board">${colsHtml}</div>`;
    bindStageActions(stage);

    stage.querySelectorAll(".ticket").forEach((t) => {
      t.addEventListener("dragstart", (e) => {
        state.internalDrag = true;
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", t.getAttribute("data-ticket-sid"));
        requestAnimationFrame(() => t.classList.add("dragging"));
      });
      t.addEventListener("dragend", () => {
        state.internalDrag = false;
        t.classList.remove("dragging");
        stage.querySelectorAll(".kb-col").forEach((c) => c.classList.remove("drop-hot"));
      });
    });
    stage.querySelectorAll(".kb-col").forEach((col) => {
      col.addEventListener("dragover", (e) => { if (!state.internalDrag) return; e.preventDefault(); col.classList.add("drop-hot"); });
      col.addEventListener("dragleave", (e) => { if (!col.contains(e.relatedTarget)) col.classList.remove("drop-hot"); });
      col.addEventListener("drop", (e) => {
        if (!state.internalDrag) return;
        e.preventDefault();
        e.stopPropagation();
        col.classList.remove("drop-hot");
        const sid = e.dataTransfer.getData("text/plain");
        const target = col.getAttribute("data-kb-status");
        const it = (state.workspace?.items || []).find((i) => i.source_id === sid);
        if (it && reimbStatusOf(it) !== target) setReimburseStatus(sid, target);
      });
    });
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
        const st = ann.reimburse_status && ann.reimburse_status !== "none" ? ann.reimburse_status : "pending";
        if (state.reimburseFilter && st !== state.reimburseFilter) return false;
      }
      if (state.collection === "csv" && !flags.is_csv_statement) return false;

      if (state.accountFilter && (ann.account_label || "") !== state.accountFilter) return false;
      if (state.currencyFilter && (!amtVal || amtVal.currency !== state.currencyFilter)) return false;
      if (state.categoryFilter && (ann.category || "未分类") !== state.categoryFilter) return false;

      if (q) {
        const hay = [
          ann.vendor, ann.plan_name, ann.account_label, ann.category,
          ann.notes, ann.license_scope, ann.warranty_end, item.display_name,
          amtVal ? `${amtVal.currency} ${amtVal.amount_text}` : "",
        ].filter(Boolean).join(" ").toLowerCase();
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

  // --- View A: Editorial Hero + Tactile Object Gallery ---
  function renderAssetsPane(ws) {
    if (state.activeNav === "assets") markScene(`assets|${state.collection}|${state.viewMode}`);
    const demoCount = ws.vault?.demo_count || 0;
    const demoBar = $("demo-notice-bar");
    if (demoCount > 0) {
      $("demo-notice-msg").textContent = `当前有 ${demoCount} 条示例数据，可随时清除，不会影响你自己的账单。`;
      demoBar.classList.remove("hidden");
    } else {
      demoBar.classList.add("hidden");
    }

    const hasItems = (ws.items || []).length > 0;
    $("collection-bar").classList.toggle("hidden", !hasItems);
    renderEditorialHero(ws);

    document.querySelectorAll("[data-collection]").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-collection") === state.collection);
    });
    document.querySelectorAll("[data-view-mode]").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-view-mode") === state.viewMode);
    });
    const viewToggleEl = document.querySelector(".view-toggle");
    if (viewToggleEl) viewToggleEl.classList.toggle("hidden", state.collection === "csv");

    // Active filter chips
    const chips = [];
    if (state.categoryFilter) chips.push(`<span class="active-chip">分类：${esc(state.categoryFilter)} <button type="button" data-clear-f="category">×</button></span>`);
    if (state.currencyFilter) chips.push(`<span class="active-chip">币种：${esc(state.currencyFilter)} <button type="button" data-clear-f="currency">×</button></span>`);
    const chipBox = $("active-filter-chips");
    chipBox.innerHTML = chips.join("");
    chipBox.querySelectorAll("[data-clear-f]").forEach((b) => {
      b.addEventListener("click", () => {
        if (b.getAttribute("data-clear-f") === "category") state.categoryFilter = "";
        if (b.getAttribute("data-clear-f") === "currency") state.currencyFilter = "";
        renderAssetsPane(ws);
      });
    });

    const stage = $("ledger-stage");
    const list = filteredItems(ws);

    if (list.length === 0) {
      if (!hasItems) {
        stage.innerHTML = `
          <div class="empty-gallery">
            ${VAULT_ART}
            <h3>小金库还空着</h3>
            <p>把 PDF 发票、邮件 HTML、支付宝 / 微信 CSV 或收据截图拖进窗口，它们会一张张飞进来；也可以直接手动记一笔。</p>
            <div class="btn-row" style="justify-content:center">
              <button type="button" id="btn-empty-import" class="btn btn-ink btn-sm">选择文件导入</button>
              <button type="button" id="btn-empty-manual" class="btn btn-soft btn-sm">记一笔</button>
              <button type="button" id="btn-load-demo-pack" class="btn btn-ghost btn-sm">加载示例数据（可随时清除）</button>
            </div>
          </div>
        `;
        $("btn-empty-import").addEventListener("click", (e) => {
          state.importOrigin = importOriginFor(e.currentTarget);
          $("file-picker-input").click();
        });
        $("btn-empty-manual").addEventListener("click", openManualModal);
        const demoBtn = $("btn-load-demo-pack");
        demoBtn.addEventListener("click", async () => {
          demoBtn.disabled = true;
          demoBtn.textContent = "加载中…";
          try {
            const snap = await api("POST", "/api/import-demo-pack", {});
            state.workspace = snap;
            renderWorkspace();
            launchConfetti({ count: 80 });
            showToast("示例数据已就位，顶部可随时清除", "ok");
          } catch (err) {
            showToast(err.message, "error");
            demoBtn.disabled = false;
            demoBtn.textContent = "加载示例数据（可随时清除）";
          }
        });
      } else {
        stage.innerHTML = `
          <div class="empty-gallery">
            <h3>没有找到匹配的记录</h3>
            <p>换个关键词，或者切回「全部」查看。</p>
            <button type="button" id="btn-reset-filters" class="btn btn-soft btn-sm">查看全部</button>
          </div>
        `;
        $("btn-reset-filters").addEventListener("click", () => {
          state.searchQuery = "";
          $("search-box").value = "";
          state.accountFilter = "";
          state.currencyFilter = "";
          state.categoryFilter = "";
          state.reimburseFilter = "";
          state.collection = "all";
          renderWorkspace();
        });
      }
      return;
    }

    if (state.collection === "csv") {
      renderCsvWorkspace(list, stage);
    } else if (state.collection === "reimbursable" && state.viewMode === "cards") {
      renderReimburseKanban(list, stage);
    } else if (state.viewMode === "cards") {
      renderObjectGallery(list, stage);
    } else {
      renderTableView(list, stage);
    }
    assignStagger(stage);
  }

  function renderFocusedContextStrip(ws, hero) {
    const items = ws.items || [];
    const col = state.collection;
    hero.classList.add("single-col");

    if (col === "hardware") {
      const hws = items.filter((i) => i.annotation?.asset_kind === "hardware");
      const byCur = {};
      const cpdByCur = {};
      let nearest = null;
      for (const it of hws) {
        const amt = it.amount_fact?.value;
        if (amt?.amount_text) {
          byCur[amt.currency] = (byCur[amt.currency] || 0) + amountToCents(amt.amount_text);
          const cpd = costPerDayInfo(it.annotation?.bill_date, amt.currency, amt.amount_text);
          if (cpd) {
            const perDay = Math.max(1, Math.round(amountToCents(amt.amount_text) / cpd.daysUsed));
            cpdByCur[amt.currency] = (cpdByCur[amt.currency] || 0) + perDay;
          }
        }
        const w = warrantyInfo(it.annotation?.bill_date, it.annotation?.warranty_end);
        if (w && w.daysLeft >= 0 && (!nearest || w.daysLeft < nearest.daysLeft)) {
          nearest = { vendor: it.annotation?.vendor || it.display_name, daysLeft: w.daysLeft };
        }
      }
      const totalStr = Object.entries(byCur).map(([c, v]) => formatMoneyShort(c, centsToText(v))).join(" · ") || "—";
      const cpdStr = Object.entries(cpdByCur).map(([c, v]) => `${formatMoneyShort(c, centsToText(v))}/天`).join(" · ") || "—";
      hero.innerHTML = `
        <div class="context-strip">
          <div class="ctx-title-group">
            <div class="ctx-title">硬件与保修</div>
            <div class="ctx-sub">${nearest ? `最近到期：${esc(nearest.vendor)}（还剩 ${nearest.daysLeft} 天）` : "记录购机发票、保修截止日与每日折算成本"}</div>
          </div>
          <div class="ctx-metrics">
            <div class="ctx-metric"><span class="ctx-k">设备数</span><span class="ctx-v">${hws.length} 台</span></div>
            <div class="ctx-metric"><span class="ctx-k">硬件合计</span><span class="ctx-v sensitive">${esc(totalStr)}</span></div>
            <div class="ctx-metric"><span class="ctx-k">合计日均成本</span><span class="ctx-v sensitive">${esc(cpdStr)}</span></div>
          </div>
        </div>
      `;
      return;
    }

    if (col === "license") {
      const lics = items.filter((i) => i.annotation?.asset_kind === "license");
      const keyCount = lics.filter((i) => i.annotation?.license_key).length;
      const byCur = {};
      for (const it of lics) {
        const amt = it.amount_fact?.value;
        if (amt?.amount_text) byCur[amt.currency] = (byCur[amt.currency] || 0) + amountToCents(amt.amount_text);
      }
      const totalStr = Object.entries(byCur).map(([c, v]) => formatMoneyShort(c, centsToText(v))).join(" · ") || "—";
      hero.innerHTML = `
        <div class="context-strip">
          <div class="ctx-title-group">
            <div class="ctx-title">买断软件与密钥</div>
            <div class="ctx-sub">激活密钥单独加密保存在本地，导出表格时自动略过</div>
          </div>
          <div class="ctx-metrics">
            <div class="ctx-metric"><span class="ctx-k">买断软件</span><span class="ctx-v">${lics.length} 款</span></div>
            <div class="ctx-metric"><span class="ctx-k">已存密钥</span><span class="ctx-v">${keyCount} 个</span></div>
            <div class="ctx-metric"><span class="ctx-k">买断合计</span><span class="ctx-v sensitive">${esc(totalStr)}</span></div>
          </div>
        </div>
      `;
      return;
    }

    if (col === "reimbursable") {
      state.reimburseFilter = "";
      const pend = {};
      const done = {};
      let pendN = 0;
      let doneN = 0;
      for (const it of items) {
        const st = reimbStatusOf(it);
        if (!st) continue;
        const a = it.amount_fact?.value;
        const c = a?.amount_text ? amountToCents(a.amount_text) : 0;
        if (st === "reimbursed") { doneN += 1; if (c) done[a.currency] = (done[a.currency] || 0) + c; }
        else { pendN += 1; if (c) pend[a.currency] = (pend[a.currency] || 0) + c; }
      }
      hero.innerHTML = `
        <div class="context-strip">
          <div class="ctx-title-group">
            <div class="ctx-title">报销看板</div>
            <div class="ctx-sub">把票据拖到下一列推进进度，也可以点票据右下角的按钮</div>
          </div>
          <div class="ctx-metrics">
            <div class="ctx-metric"><span class="ctx-k">还在路上</span><span class="ctx-v sensitive">${esc(moneyListText(pend) || "0")}</span></div>
            <div class="ctx-metric"><span class="ctx-k">进行中</span><span class="ctx-v">${pendN} 笔</span></div>
            <div class="ctx-metric"><span class="ctx-k">已到账</span><span class="ctx-v sensitive">${esc(moneyListText(done) || "0")}</span></div>
          </div>
        </div>
      `;
      return;
    }

    if (col === "subscription") {
      const subGroups = (state.groups || []).filter((g) => g.kind === "subscription");
      const totals = recurringTotals(subGroups);
      const monthStr = totals.map(([cur, c]) => formatMoneyShort(cur, centsToText(c))).join(" + ") || "—";
      const yearStr = totals.map(([cur, c]) => formatMoneyShort(cur, centsToText(c * 12))).join(" + ") || "—";
      hero.innerHTML = `
        <div class="context-strip">
          <div class="ctx-title-group">
            <div class="ctx-title">周期订阅</div>
            <div class="ctx-sub">同一账号与套餐的多张历史账单合并展示；按账单标注周期折算，不代表当前是否仍在扣费</div>
          </div>
          <div class="ctx-metrics">
            <div class="ctx-metric"><span class="ctx-k">订阅</span><span class="ctx-v">${subGroups.length} 项</span></div>
            <div class="ctx-metric"><span class="ctx-k">每月折算</span><span class="ctx-v sensitive">${esc(monthStr)}</span></div>
            <div class="ctx-metric"><span class="ctx-k">一年折算</span><span class="ctx-v sensitive">${esc(yearStr)}</span></div>
          </div>
        </div>
      `;
      return;
    }

    // CSV view
    const csvs = items.filter((i) => i.status_flags?.is_csv_statement);
    const totalRows = csvs.reduce((s, c) => s + (c.csv_row_count || 0), 0);
    hero.innerHTML = `
      <div class="context-strip">
        <div class="ctx-title-group">
          <div class="ctx-title">CSV 流水明细</div>
          <div class="ctx-sub">可直接在下方筛选支付宝 / 微信流水，挑出订阅或重要消费一键加入账本</div>
        </div>
        <div class="ctx-metrics">
          <div class="ctx-metric"><span class="ctx-k">流水文件</span><span class="ctx-v">${csvs.length} 份</span></div>
          <div class="ctx-metric"><span class="ctx-k">解析明细</span><span class="ctx-v">${totalRows} 条</span></div>
        </div>
      </div>
    `;
  }

  function renderEditorialHero(ws) {
    const hero = $("editorial-hero");
    const items = ws.items || [];
    if (items.length === 0) {
      hero.classList.add("hidden");
      return;
    }
    hero.classList.remove("hidden");

    if (state.collection !== "all") {
      renderFocusedContextStrip(ws, hero);
      return;
    }
    hero.classList.remove("single-col");

    // 每月固定支出：各币种并列，每个订阅只算最新一张账单
    const totals = recurringTotals(state.groups);
    const recorded = (ws.summary_by_currency || []).filter((c) => amountToCents(c.total_recorded) > 0);
    const billCount = recorded.reduce((s, c) => s + (c.bill_count || 0), 0);

    // Living Signals Column (up to 3 crisp tactile cards)
    const radar = ws.asset_radar || [];
    const signalCards = [];
    for (let idx = 0; idx < Math.min(3, radar.length); idx++) {
      const r = radar[idx];
      if (r.kind === "warranty_expiring") {
        const localDays = localCalendarDaysDiff(r.warranty_end);
        const daysLeft = localDays !== null ? localDays : r.days_left;
        const ring = renderRingGauge(Math.max(8, Math.round((Math.max(0, daysLeft) / 365) * 100)), daysLeft <= 14 ? "var(--rose)" : "var(--iris)", `${Math.max(0, daysLeft)}d`);
        signalCards.push(`
          <button type="button" class="signal-tile" data-sig-idx="${idx}">
            <div class="sig-left">
              ${ring}
              <div style="min-width:0">
                <div class="sig-title">${esc(r.vendor)} 保修提醒</div>
                <div class="sig-detail">${daysLeft > 0 ? `还剩 ${daysLeft} 天` : "今日到期"} · ${esc(r.warranty_end)} 截止</div>
              </div>
            </div>
            <span style="color:var(--ink-3)">${ICO.arrowRight}</span>
          </button>
        `);
      } else if (r.kind === "price_hike") {
        signalCards.push(`
          <button type="button" class="signal-tile" data-sig-idx="${idx}">
            <div class="sig-left">
              <div class="sig-badge-box rose">${ICO.trendUp}</div>
              <div style="min-width:0">
                <div class="sig-title">${esc(r.vendor)} 涨价 +${Math.abs(r.pct_change || 0)}%</div>
                <div class="sig-detail sensitive">${esc(r.detail)}</div>
              </div>
            </div>
            <span style="color:var(--ink-3)">${ICO.arrowRight}</span>
          </button>
        `);
      } else {
        signalCards.push(`
          <button type="button" class="signal-tile" data-sig-idx="${idx}">
            <div class="sig-left">
              <div class="sig-badge-box amber">${ICO.receipt}</div>
              <div style="min-width:0">
                <div class="sig-title sensitive">${esc(r.label)}</div>
                <div class="sig-detail">点击查看并更新报销状态</div>
              </div>
            </div>
            <span style="color:var(--ink-3)">${ICO.arrowRight}</span>
          </button>
        `);
      }
    }

    if (signalCards.length < 2) {
      const v = ws.vault;
      signalCards.push(`
        <button type="button" class="signal-tile" data-sig-nav="security">
          <div class="sig-left">
            <div class="sig-badge-box green">${ICO.shield}</div>
            <div>
              <div class="sig-title">${v.source_count} 份原件均完整</div>
              <div class="sig-detail">${v.duplicate_count > 0 ? `已跳过重复 ${v.duplicate_count} 次 · ` : ""}随时可导出备份</div>
            </div>
          </div>
          <span style="color:var(--ink-3)">${ICO.arrowRight}</span>
        </button>
      `);
    }

    const facts = funFactsForAll(totals);
    const factIdx = ((state.funIdx || 0) % facts.length + facts.length) % facts.length;
    const figuresHtml = totals.length
      ? totals.map(([cur, c]) => `
          <div class="hero-fig">
            <div class="hero-big-num sensitive" data-countup="hero-m-${esc(cur)}" data-cur="${esc(cur)}" data-cents="${c}">${renderEditorialMoney(cur, centsToText(c), false)}</div>
            <div class="hero-fig-cap">${esc(CURRENCY_NAMES[cur] || cur)} · 按周期月折算</div>
          </div>`).join("")
      : `<div class="hero-fig"><div class="hero-big-num" style="color:var(--ink-3)">—</div><div class="hero-fig-cap">还没有周期订阅</div></div>`;
    const yearLine = totals.length
      ? `按标注周期折算，一年约 <strong class="sensitive">${esc(totals.map(([cur, c]) => formatMoneyShort(cur, centsToText(c * 12))).join(" + "))}</strong>`
      : "记一笔周期订阅后，这里会显示按周期折算的月度参考";
    const recordedLine = recorded.length
      ? `已录金额 ${billCount} 笔（共 ${items.length} 份文件），账本已记录 <span class="sensitive">${esc(recorded.map((c) => formatMoneyShort(c.currency, c.total_recorded)).join(" + "))}</span>`
      : `共 ${items.length} 份原件`;

    hero.innerHTML = `
      <div class="hero-primary">
        <div class="hero-top-bar">
          <span class="hero-eyebrow">周期订阅月度折算</span>
          <span class="hero-note">各币种分开统计 · 仅据账单标注周期，不代表当前是否仍在扣费</span>
        </div>

        <div class="hero-multi">${figuresHtml}</div>

        <div class="hero-foot">
          <div class="hero-year">${yearLine}</div>
          <button type="button" class="fun-fact sensitive" data-fun-cycle title="点一下，换个算法">
            <span class="ff-spark" aria-hidden="true">✦</span>
            <span class="ff-text">${esc(facts[factIdx])}</span>
            <span class="ff-hint">换一个</span>
          </button>
          ${recordedLine ? `<div class="hero-recorded">${recordedLine}</div>` : ""}
        </div>
      </div>

      <div class="hero-signals">${signalCards.join("")}</div>
    `;

    runCountUps(hero);
    assignStagger(hero);
    const funBtn = hero.querySelector("[data-fun-cycle]");
    if (funBtn) {
      funBtn.addEventListener("click", () => {
        state.funIdx = (state.funIdx || 0) + 1;
        const textEl = funBtn.querySelector(".ff-text");
        textEl.textContent = facts[((state.funIdx % facts.length) + facts.length) % facts.length];
        pulseClass(textEl, "flip", 420);
        pulseClass(funBtn.querySelector(".ff-spark"), "pop", 500);
      });
    }

    hero.querySelectorAll("[data-sig-idx]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const r = radar[Number(btn.getAttribute("data-sig-idx"))];
        if (!r) return;
        if (r.source_id) {
          const found = items.find((i) => i.source_id === r.source_id);
          if (found) openDrawer(found);
        } else {
          state.collection = "reimbursable";
          renderAssetsPane(ws);
        }
      });
    });
    hero.querySelectorAll("[data-sig-nav]").forEach((btn) => {
      btn.addEventListener("click", () => navigateTo(btn.getAttribute("data-sig-nav")));
    });
  }

  function renderAssetCard(item, isWide) {
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
    const prog = kind === "subscription" ? periodProgressInfo(perVal) : null;
    const group = kind === "subscription" ? groupOfItem(item.source_id) : null;
    const renewal = group?.renewal || null;

    let driftPill = "";
    if (drift) {
      const up = drift.direction === "up";
      driftPill = `<span class="micro-pill ${up ? "rose" : "green"}" title="较上期 ${esc(drift.prev_text)} 变动 ${esc(drift.diff_text)}">${up ? ICO.trendUp : ICO.trendDown} ${up ? `+${Math.abs(drift.pct_change)}%` : `-${Math.abs(drift.pct_change)}%`}</span>`;
    }

    let priceHtml = "";
    if (amtVal && amtVal.amount_text) {
      const cyc = kind === "hardware" ? "一次购入" : kind === "license" ? "终身买断" : (CYCLE_LABELS[ann.billing_cycle] || "未标注");
      priceHtml = `
        <div class="oc-price-cluster">
          <div class="oc-amount sensitive">${renderEditorialMoney(amtVal.currency, amtVal.amount_text, !CURRENCY_SYMBOLS[amtVal.currency])}</div>
          ${driftPill}
        </div>
        <span class="oc-cycle">${esc(cyc)}</span>
      `;
    } else if (flags.is_csv_statement) {
      priceHtml = `<span class="micro-pill slate">CSV 流水 · ${item.csv_row_count} 条</span><span class="oc-cycle">点击查看</span>`;
    } else if (flags.is_credit_or_order_warning) {
      priceHtml = `<span class="micro-pill amber">退款 / 未付订单</span><span class="oc-cycle">未计入</span>`;
    } else {
      priceHtml = `<span style="font-size:13px;font-weight:700;color:var(--amber)">缺少金额</span><span class="oc-cycle">点击补充</span>`;
    }

    let bodyHtml = "";
    if (kind === "hardware") {
      const w = warrantyInfo(ann.bill_date, ann.warranty_end);
      const cpd = amtVal ? costPerDayInfo(ann.bill_date, amtVal.currency, amtVal.amount_text) : null;
      const extraMeta = isWide
        ? `<div style="display:flex;gap:14px;flex-wrap:wrap;font-size:12px;color:var(--ink-3)">${ann.bill_date ? `<span>购入 ${esc(ann.bill_date)}</span>` : ""}${ann.warranty_end ? `<span>保修至 ${esc(ann.warranty_end)}</span>` : ""}${ann.notes ? `<span>${esc(ann.notes)}</span>` : ""}</div>`
        : "";
      if (w || cpd) {
        bodyHtml = `
          <div class="hw-instrument">
            <div>
              ${cpd ? `<div class="hw-cpd-num sensitive">${esc(cpd.perDayText)} <span style="font-size:11.5px;font-weight:500;color:var(--ink-3)">/ 天</span></div><div class="hw-cpd-sub">已用 ${cpd.daysUsed} 天${w ? ` · ${esc(w.statusLabel)}` : ""}</div>` : `<div class="hw-cpd-sub">${esc(w.statusLabel)}</div>`}
            </div>
            ${w ? renderRingGauge(w.remPct, w.color, w.shortTag) : ""}
          </div>
          ${extraMeta}
        `;
      } else {
        bodyHtml = extraMeta;
      }
    } else if (kind === "license") {
      const parts = [];
      if (ann.license_scope) parts.push(`<div style="font-size:12px;color:var(--ink-2)">${esc(ann.license_scope)}${isWide && ann.bill_date ? ` · 购于 ${esc(ann.bill_date)}` : ""}</div>`);
      if (ann.license_key) {
        parts.push(`
          <div class="serial-plate">
            <span class="sensitive">${esc(maskLicenseKey(ann.license_key))}</span>
            <button type="button" class="copy-key-pill" data-copy-key-sid="${esc(item.source_id)}">${ICO.copy} 复制密钥</button>
          </div>
        `);
      }
      if (isWide && ann.notes) parts.push(`<div style="font-size:12px;color:var(--ink-3)">${esc(ann.notes)}</div>`);
      bodyHtml = parts.join("");
    } else if (kind === "subscription") {
      if (renewal && !renewal.stale) {
        bodyHtml = `
          <div>
            <div class="meter-caption"><span>按周期参考 · ${esc(renewal.date.slice(5).replace("-", "/"))}</span><span>${esc(relDayLabel(renewal.daysLeft))}</span></div>
            ${renderTickMeter(renewal.pct, false)}
          </div>
        `;
      } else if (renewal?.stale) {
        bodyHtml = `
          <div>
            <div class="meter-caption"><span>最近账单期至 ${esc(renewal.last)}</span><span>未录新账单（状态未知）</span></div>
            ${renderTickMeter(100, true)}
          </div>
        `;
      } else if (prog) {
        bodyHtml = `
          <div>
            <div class="meter-caption"><span>${esc(prog.range)}</span><span>${esc(prog.statusLabel)}</span></div>
            ${renderTickMeter(prog.pct, prog.periodEnded)}
          </div>
        `;
      } else if (ann.bill_date) {
        bodyHtml = `<div style="font-size:12px;color:var(--ink-3)">账单日 ${esc(ann.bill_date)}</div>`;
      }
    } else if (kind === "reimbursable") {
      bodyHtml = `
        ${ann.bill_date ? `<div style="font-size:12px;color:var(--ink-3)">开票日期 ${esc(ann.bill_date)}${ann.notes ? ` · ${esc(ann.notes.slice(0, 26))}` : ""}</div>` : ""}
        <div class="ticket-divider"></div>
      `;
    } else if (ann.bill_date) {
      bodyHtml = `<div style="font-size:12px;color:var(--ink-3)">日期 ${esc(ann.bill_date)}</div>`;
    }

    const reimbStatus = ann.reimburse_status && ann.reimburse_status !== "none"
      ? ann.reimburse_status
      : kind === "reimbursable" ? "pending" : null;
    const stampHtml = reimbStatus && REIMBURSE_META[reimbStatus]
      ? `<button type="button" class="stamp-btn ${REIMBURSE_META[reimbStatus].cls}" data-reimb-sid="${esc(item.source_id)}" data-reimb-cur="${esc(reimbStatus)}" title="点击切换报销进度">${REIMBURSE_META[reimbStatus].label} ${ICO.refresh}</button>`
      : "";

    const metaStr = [ann.account_label, ann.category, flags.has_duplicate ? `防重 ×${item.occurrence_count}` : ""].filter(Boolean).join(" · ") || "未分组";
    const spineHtml = kind === "subscription" ? `<span class="obj-spine" style="background:${brand.bg}"></span>` : "";
    const showKindPill = flags.has_conflict || state.collection === "all";
    const kindPillHtml = showKindPill
      ? `<span class="micro-pill ${flags.has_conflict ? "amber" : kindMeta.pill}">${flags.has_conflict ? "待确认冲突" : esc(kindMeta.label)}</span>`
      : "";
    const integrityHtml = item.object_intact ? "" : `<span class="micro-pill rose" title="原件校验异常">校验异常</span>`;
    const billCountHtml = group && group.items.length > 1 ? `<span class="bill-count" title="同一个订阅的账单，点开卡片可以逐张查看">${group.items.length} 张账单</span>` : "";

    return `
      <article class="obj-card kind-${esc(kind)}" tabindex="0" role="button" data-source-id="${esc(item.source_id)}">
        ${spineHtml}
        <div class="oc-top">
          <div class="oc-id">
            <div class="brand-badge" style="background:${brand.bg}">${esc(brand.mark)}</div>
            <div style="min-width:0">
              <div class="oc-title">${esc(title)}</div>
              <div class="oc-sub">${esc(sub)}</div>
            </div>
          </div>
          ${kindPillHtml}
        </div>

        <div class="oc-price-row">${priceHtml}</div>
        ${bodyHtml}

        <div class="oc-foot ${kind === "reimbursable" ? "" : "ruled"}">
          <span class="sensitive" style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(metaStr)}</span>
          <span class="oc-foot-right">${billCountHtml}${stampHtml || integrityHtml}</span>
        </div>
      </article>
    `;
  }

  function renderFilingStripRow(item) {
    const ann = item.annotation || {};
    const amtVal = item.amount_fact ? item.amount_fact.value : null;
    const perVal = item.period_fact ? item.period_fact.value : null;
    const flags = item.status_flags || {};
    const title = ann.vendor || item.display_name;
    const brand = brandStyleFor(title);

    let meta = "";
    let rightHtml = "";
    if (flags.is_csv_statement) {
      meta = `${item.display_name} · ${item.csv_row_count || 0} 条流水${ann.account_label ? ` · ${ann.account_label}` : ""}`;
      rightHtml = `<span class="micro-pill slate">CSV 流水</span><button type="button" class="btn btn-soft btn-sm" data-open-csv-view="1">查看流水</button>`;
    } else if (flags.needs_manual_entry || flags.has_conflict) {
      meta = `${item.media_type} · ${formatBytes(item.byte_length)} · ${flags.has_conflict ? "检测到多个金额待确认" : "尚未记录金额"}`;
      rightHtml = `<span class="micro-pill amber">${flags.has_conflict ? "待确认" : "缺少金额"}</span><button type="button" class="btn btn-soft btn-sm" data-open-triage="1">去补充</button>`;
    } else {
      const range = perVal?.period_start ? `${perVal.period_start} — ${perVal.period_end}` : ann.bill_date || "";
      meta = [ann.plan_name, range, ann.account_label].filter(Boolean).join(" · ");
      const driftBadge = item.price_drift
        ? `<span class="micro-pill ${item.price_drift.direction === "up" ? "rose" : "green"}">${item.price_drift.direction === "up" ? `+${Math.abs(item.price_drift.pct_change)}%` : `-${Math.abs(item.price_drift.pct_change)}%`}</span>`
        : "";
      rightHtml = `${driftBadge}${amtVal?.amount_text ? `<strong class="sensitive num" style="font-size:13.5px">${esc(formatMoney(amtVal.currency, amtVal.amount_text))}</strong>` : ""}<span class="micro-pill slate">往期</span>`;
    }

    return `
      <div class="filing-strip" tabindex="0" role="button" data-source-id="${esc(item.source_id)}">
        <div class="filing-left">
          <div class="brand-badge brand-badge-sm" style="background:${brand.bg}">${esc(brand.mark)}</div>
          <div style="min-width:0">
            <div class="filing-title">${esc(title)}</div>
            <div class="filing-meta sensitive">${esc(meta)}</div>
          </div>
        </div>
        <div class="filing-right">${rightHtml}</div>
      </div>
    `;
  }

  function renderObjectGallery(list, stage) {
    const isWide = state.collection === "hardware" || state.collection === "license";
    // 同一个订阅的多张账单只显示最新一张；搜到旧账单时也换成它所属订阅的最新卡片
    const seen = new Set();
    list = list.map((it) => {
      const g = it.annotation?.asset_kind === "subscription" ? groupOfItem(it.source_id) : null;
      return g ? g.latest : it;
    }).filter((it) => (seen.has(it.source_id) ? false : seen.add(it.source_id)));
    let mainItems = list;
    let secondaryItems = [];
    let secondaryLabel = "";

    if (state.collection === "all") {
      mainItems = [];
      secondaryItems = [];
      for (const it of list) {
        const f = it.status_flags || {};
        if (f.is_csv_statement || f.needs_manual_entry || f.is_credit_or_order_warning) {
          secondaryItems.push(it);
        } else {
          mainItems.push(it);
        }
      }
      secondaryLabel = "流水与待处理文件";
    }

    const gridClass = isWide
      ? (mainItems.length === 1 ? "wide-grid solo-grid" : "wide-grid")
      : "";
    const primaryHtml = mainItems.length
      ? `<div class="object-gallery ${gridClass}">${mainItems.map((it) => renderAssetCard(it, isWide)).join("")}</div>`
      : "";

    const secondaryHtml = secondaryItems.length
      ? `
        <div class="stage-section" ${mainItems.length === 0 ? 'style="margin-top:0"' : ""}>
          <div class="stage-section-head">
            <span class="stage-section-title">${esc(secondaryLabel)} <span class="micro-pill slate">${secondaryItems.length}</span></span>
          </div>
          <div class="filing-list">${secondaryItems.map(renderFilingStripRow).join("")}</div>
        </div>
      `
      : "";

    stage.innerHTML = `${primaryHtml}${secondaryHtml}`;
    bindStageActions(stage);
  }

  function renderCsvWorkspace(list, stage) {
    const cardsHtml = list.map((item, cIdx) => {
      const ann = item.annotation || {};
      const title = ann.vendor || item.display_name;
      const brand = brandStyleFor(title);
      const rows = (item.candidates || [])
        .filter((c) => c.kind === "csv_transaction")
        .map((c) => ({ ...c.value, raw_line: c.excerpt }));

      return `
        <section class="csv-ws-card" data-csv-card="${cIdx}" data-csv-sid="${esc(item.source_id)}">
          <div class="csv-ws-head">
            <div class="csv-ws-id">
              <div class="brand-badge brand-badge-sm" style="background:${brand.bg}">${esc(brand.mark)}</div>
              <div style="min-width:0">
                <div style="font-size:14.5px;font-weight:700">${esc(title)}</div>
                <div style="font-size:12px;color:var(--ink-3)" class="sensitive">${esc(item.display_name)} · 共 ${rows.length} 条${ann.account_label ? ` · ${esc(ann.account_label)}` : ""}</div>
              </div>
            </div>
            <div class="csv-ws-tools">
              <div class="search-capsule" style="width:200px">
                <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>
                <input type="search" data-csv-filter-q="${cIdx}" placeholder="筛选商户或说明…">
              </div>
              <label class="check-row compact">
                <input type="checkbox" data-csv-filter-exp="${cIdx}" checked>
                <span>仅看支出</span>
              </label>
              <button type="button" class="btn btn-soft btn-sm" data-csv-open-detail="${esc(item.source_id)}">原件与详情</button>
            </div>
          </div>
          <div style="overflow-x:auto">
            <table class="csv-ws-table">
              <thead>
                <tr>
                  <th>日期</th>
                  <th>商户</th>
                  <th>商品说明</th>
                  <th class="right">金额</th>
                  <th class="right">操作</th>
                </tr>
              </thead>
              <tbody data-csv-tbody="${cIdx}"></tbody>
            </table>
          </div>
          <div class="csv-ws-foot" data-csv-foot="${cIdx}"></div>
        </section>
      `;
    }).join("");

    stage.innerHTML = `<div class="csv-workspace-stack">${cardsHtml}</div>`;

    const allVaultItems = state.workspace?.items || [];
    const findExistingCsvItem = (r) => {
      const rDate = String(r.date || "").slice(0, 10);
      const rMerch = String(r.merchant || "").trim().toLowerCase();
      const rAmt = String(r.amount_text || "").trim();
      const rCur = String(r.currency || "CNY").trim();
      return allVaultItems.find((it) => {
        if (it.status_flags?.is_csv_statement) return false;
        const a = it.annotation || {};
        const v = it.amount_fact?.value;
        if (!v || v.currency !== rCur || v.amount_text !== rAmt) return false;
        const sameVendor = (a.vendor || "").trim().toLowerCase() === rMerch;
        const sameDate = rDate && (a.bill_date || "").slice(0, 10) === rDate;
        return sameVendor && sameDate;
      });
    };

    list.forEach((item, cIdx) => {
      const allRows = (item.candidates || [])
        .filter((c) => c.kind === "csv_transaction")
        .map((c) => ({ ...c.value, raw_line: c.excerpt }));
      const tbody = stage.querySelector(`[data-csv-tbody="${cIdx}"]`);
      const foot = stage.querySelector(`[data-csv-foot="${cIdx}"]`);
      const qInput = stage.querySelector(`[data-csv-filter-q="${cIdx}"]`);
      const expChk = stage.querySelector(`[data-csv-filter-exp="${cIdx}"]`);
      const uiState = state.csvUiState.get(item.source_id) || { q: "", expOnly: true, limit: 100 };
      state.csvUiState.set(item.source_id, uiState);
      qInput.value = uiState.q;
      expChk.checked = uiState.expOnly;

      const paintRows = () => {
        uiState.q = qInput.value || "";
        uiState.expOnly = expChk.checked;
        const q = uiState.q.trim().toLowerCase();
        const expOnly = uiState.expOnly;
        const filtered = allRows.filter((r) => {
          if (expOnly && r.direction !== "expense") return false;
          if (q && !`${r.merchant || ""} ${r.description || ""} ${r.date || ""} ${r.amount_text || ""}`.toLowerCase().includes(q)) return false;
          return true;
        });

        const sums = {};
        for (const r of filtered) {
          const c = r.currency || "CNY";
          sums[c] = (sums[c] || 0) + amountToCents(r.amount_text);
        }
        const sumText = Object.entries(sums).map(([c, v]) => formatMoney(c, centsToText(v))).join(" · ") || "—";
        const shownCount = Math.min(filtered.length, uiState.limit);
        const moreBtn = filtered.length > shownCount
          ? ` <button type="button" class="btn btn-soft btn-sm" data-csv-more="${cIdx}" style="margin-left:8px">加载更多（剩余 ${filtered.length - shownCount} 条）</button>`
          : "";
        foot.innerHTML = `<span>当前显示 <strong>${shownCount}</strong> / ${filtered.length} 条${expOnly ? "支出" : "记录"}${moreBtn}</span><span>合计 <strong class="sensitive">${esc(sumText)}</strong></span>`;
        const moreEl = foot.querySelector(`[data-csv-more="${cIdx}"]`);
        if (moreEl) {
          moreEl.addEventListener("click", () => {
            uiState.limit += 150;
            paintRows();
          });
        }

        if (!filtered.length) {
          tbody.innerHTML = `<tr><td colspan="5" style="text-align:center;color:var(--ink-3);padding:24px">没有符合条件的流水记录</td></tr>`;
          return;
        }
        tbody.innerHTML = filtered.slice(0, uiState.limit).map((r, rIdx) => {
          const existing = findExistingCsvItem(r);
          const isNonExpense = ["income", "refund", "neutral"].includes(r.direction);
          const dirLabel = { income: "收入", refund: "退款", neutral: "中性交易" }[r.direction] || "";
          const actBtn = existing
            ? `<button type="button" class="csv-added-btn" data-ws-open-existing="${esc(existing.source_id)}">${ICO.check} 已在账本</button>`
            : isNonExpense
              ? `<span class="micro-pill slate">${esc(dirLabel)} · 不计支出</span>`
              : `<button type="button" class="btn btn-soft btn-sm" data-ws-promote="${rIdx}">加入账本</button>`;
          return `
            <tr>
              <td style="font-size:12px;color:var(--ink-3);white-space:nowrap" class="num">${esc(r.date || "—")}</td>
              <td style="font-weight:600">${esc(r.merchant || "未标注商户")}</td>
              <td style="color:var(--ink-2)">${esc(r.description || "—")}</td>
              <td class="right sensitive num" style="font-weight:700;white-space:nowrap">${esc(formatMoney(r.currency || "CNY", r.amount_text))}</td>
              <td class="right">${actBtn}</td>
            </tr>
          `;
        }).join("");

        tbody.querySelectorAll("[data-ws-open-existing]").forEach((btn) => {
          btn.addEventListener("click", () => {
            const found = allVaultItems.find((i) => i.source_id === btn.getAttribute("data-ws-open-existing"));
            if (found) openDrawer(found);
          });
        });

        tbody.querySelectorAll("[data-ws-promote]").forEach((btn) => {
          btn.addEventListener("click", async () => {
            const r = filtered[Number(btn.getAttribute("data-ws-promote"))];
            if (!r) return;
            btn.disabled = true;
            btn.textContent = "加入中…";
            try {
              const res = await api("POST", "/api/csv/promote", {
                parent_source_id: item.source_id,
                row_number: r.row_number,
                merchant: r.merchant || "",
                description: r.description || "",
                date: r.date || "",
                currency: r.currency || "CNY",
                amount_text: r.amount_text || "",
                direction: r.direction || "",
                billing_cycle: "unknown",
                account_label: r.account_hint || undefined,
                raw_line: r.raw_line || "",
              });
              showToast(res.storage_reused ? "这条流水已经加过，已跳过重复" : `已将「${r.merchant || "该笔流水"}」加入账本`, "ok");
              await refreshWorkspace();
            } catch (err) {
              showToast(err.message, "error");
              btn.disabled = false;
              btn.textContent = "加入账本";
            }
          });
        });
      };

      qInput.addEventListener("input", () => { uiState.limit = 100; paintRows(); });
      expChk.addEventListener("change", () => { uiState.limit = 100; paintRows(); });
      paintRows();
    });

    stage.querySelectorAll("[data-csv-open-detail]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const found = (state.workspace.items || []).find((i) => i.source_id === btn.getAttribute("data-csv-open-detail"));
        if (found) openDrawer(found, "evidence");
      });
    });
  }

  function renderTableView(list, stage) {
    const rows = list.map((item) => {
      const ann = item.annotation || {};
      const amtVal = item.amount_fact ? item.amount_fact.value : null;
      const perVal = item.period_fact ? item.period_fact.value : null;
      const flags = item.status_flags || {};
      const kind = ann.asset_kind || "general";
      const kindMeta = ASSET_KIND_META[kind] || ASSET_KIND_META.general;
      const title = ann.vendor || item.display_name;
      const brand = brandStyleFor(title);

      let amtHtml = amtVal?.amount_text
        ? `<span class="sensitive num" style="font-weight:700">${esc(formatMoney(amtVal.currency, amtVal.amount_text))}</span>`
        : flags.is_csv_statement ? `<span class="micro-pill slate">${item.csv_row_count} 条流水</span>` : `<span style="color:var(--amber);font-weight:600">待补录</span>`;
      if (item.price_drift) {
        const up = item.price_drift.direction === "up";
        amtHtml += ` <span class="micro-pill ${up ? "rose" : "green"}">${up ? `+${Math.abs(item.price_drift.pct_change)}%` : `-${Math.abs(item.price_drift.pct_change)}%`}</span>`;
      }

      let dateStr = perVal?.period_start ? `${perVal.period_start} — ${perVal.period_end}` : ann.bill_date || "—";
      if (kind === "hardware" && ann.warranty_end) dateStr = `保修至 ${ann.warranty_end}`;

      const acts = [];
      if (kind === "license" && ann.license_key) acts.push(`<button type="button" class="copy-key-pill" data-copy-key-sid="${esc(item.source_id)}">${ICO.copy} 密钥</button>`);
      const reimb = ann.reimburse_status && ann.reimburse_status !== "none" ? ann.reimburse_status : kind === "reimbursable" ? "pending" : null;
      if (reimb && REIMBURSE_META[reimb]) acts.push(`<button type="button" class="stamp-btn ${REIMBURSE_META[reimb].cls}" data-reimb-sid="${esc(item.source_id)}" data-reimb-cur="${esc(reimb)}">${REIMBURSE_META[reimb].label} ${ICO.refresh}</button>`);

      return `
        <tr tabindex="0" role="button" data-source-id="${esc(item.source_id)}">
          <td>
            <div class="tbl-id">
              <div class="brand-badge brand-badge-sm" style="background:${brand.bg}">${esc(brand.mark)}</div>
              <div>
                <strong>${esc(title)}</strong>
                <small class="sensitive">${esc([ann.plan_name, ann.account_label].filter(Boolean).join(" · "))}</small>
              </div>
            </div>
          </td>
          <td><span class="micro-pill ${kindMeta.pill}">${esc(kindMeta.label)}</span></td>
          <td style="color:var(--ink-2)">${esc(CYCLE_LABELS[ann.billing_cycle] || "—")}</td>
          <td class="right">${amtHtml}</td>
          <td style="font-size:12px;color:var(--ink-3);white-space:nowrap">${esc(dateStr)}</td>
          <td class="right"><div class="btn-row" style="justify-content:flex-end">${acts.join("") || (item.object_intact ? "" : `<span class="micro-pill rose">校验异常</span>`)}</div></td>
        </tr>
      `;
    }).join("");

    stage.innerHTML = `
      <div class="table-sheet">
        <table class="clean-table">
          <thead><tr><th>名称</th><th>类型</th><th>周期</th><th class="right">金额</th><th>日期 / 保修</th><th class="right">操作</th></tr></thead>
          <tbody>${rows}</tbody>
        </table>
      </div>
    `;
    bindStageActions(stage);
  }

  function bindStageActions(stage) {
    stage.querySelectorAll("[data-open-csv-view]").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        state.collection = "csv";
        renderAssetsPane(state.workspace);
      });
    });
    stage.querySelectorAll("[data-open-triage]").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        navigateTo("triage");
      });
    });
    stage.querySelectorAll("[data-copy-key-sid]").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        const found = (state.workspace.items || []).find((i) => i.source_id === btn.getAttribute("data-copy-key-sid"));
        if (found?.annotation?.license_key) copyLicenseKey(found.annotation.vendor || found.display_name, found.annotation.license_key, btn);
      });
    });
    stage.querySelectorAll("[data-reimb-sid]").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        cycleReimburseStatus(btn.getAttribute("data-reimb-sid"), btn.getAttribute("data-reimb-cur"));
      });
    });
    stage.querySelectorAll("[data-source-id]").forEach((el) => {
      const openItem = () => {
        const found = (state.workspace.items || []).find((i) => i.source_id === el.getAttribute("data-source-id"));
        if (found) openDrawer(found);
      };
      el.addEventListener("click", openItem);
      el.addEventListener("keydown", (e) => {
        if ((e.key === "Enter" || e.key === " ") && e.target === el) {
          e.preventDefault();
          openItem();
        }
      });
    });
  }

  // --- View B: 70/30 Split Timeline Calendar + Month Agenda ---
  function renderCalendarPane(ws) {
    const pane = $("pane-calendar");
    const year = state.calendarYear;
    const month = state.calendarMonth;
    if (state.activeNav === "calendar") markScene(`calendar|${year}-${month}`);
    const startWeekday = new Date(year, month, 1).getDay();
    const daysInMonth = new Date(year, month + 1, 0).getDate();
    const prevMonthDays = new Date(year, month, 0).getDate();
    const today = new Date();

    // Group recurring subscriptions consistently by (vendor, plan, account, currency, cycle)
    const latestRec = new Map();
    const realMonthKeys = new Set();
    for (const item of ws.items || []) {
      const ann = item.annotation || {};
      const amtVal = item.amount_fact?.value;
      if (!amtVal?.amount_text || !["monthly", "quarterly", "yearly", "weekly"].includes(ann.billing_cycle)) continue;
      const key = subscriptionIdentityKey(item);
      const dStr = ann.bill_date || item.period_fact?.value?.period_start || "";
      if (dStr && /^\d{4}-\d{2}-\d{2}$/.test(dStr)) {
        const [ry, rm] = dStr.split("-").map(Number);
        if (ry === year && rm === month + 1) realMonthKeys.add(key);
      }
      const prev = latestRec.get(key);
      if (!prev || dStr >= prev.dStr) latestRec.set(key, { sid: item.source_id, dStr, key });
    }
    const projectable = new Set([...latestRec.values()].map((v) => v.sid));

    const eventsByDay = {};
    const agenda = [];
    const addEvt = (day, evt) => {
      if (!eventsByDay[day]) eventsByDay[day] = [];
      eventsByDay[day].push(evt);
      agenda.push({ day, ...evt });
    };

    for (const item of ws.items || []) {
      const ann = item.annotation || {};
      const amtVal = item.amount_fact?.value;
      const perVal = item.period_fact?.value;
      const title = ann.vendor || item.display_name;

      if (ann.warranty_end && /^\d{4}-\d{2}-\d{2}$/.test(ann.warranty_end)) {
        const [wy, wm, wd] = ann.warranty_end.split("-").map(Number);
        if (wy === year && wm === month + 1) addEvt(wd, { kind: "warranty", source_id: item.source_id, title, money: "保修截止", sub: "硬件保修日" });
      }
      if (!amtVal?.amount_text) continue;
      const money = formatMoneyShort(amtVal.currency, amtVal.amount_text);
      const cents = amountToCents(amtVal.amount_text);
      const realDateStr = ann.bill_date || perVal?.period_start;
      let matchedRealDay = null;
      if (realDateStr && /^\d{4}-\d{2}-\d{2}$/.test(realDateStr)) {
        const [ry, rm, rd] = realDateStr.split("-").map(Number);
        if (ry === year && rm === month + 1) {
          matchedRealDay = rd;
          addEvt(rd, { kind: "real", source_id: item.source_id, title, money, currency: amtVal.currency, cents, sub: ann.account_label ? `账单日 · ${ann.account_label}` : "账单日" });
        }
      }
      if (!projectable.has(item.source_id)) continue;
      const subKey = subscriptionIdentityKey(item);
      const anchorStr = perVal?.period_end || realDateStr;
      if (anchorStr && /^\d{4}-\d{2}-\d{2}$/.test(anchorStr)) {
        const [ay, am, ad] = anchorStr.split("-").map(Number);
        if (ann.billing_cycle === "weekly") {
          const anchorUtc = Date.UTC(ay, am - 1, ad);
          const monthStartUtc = Date.UTC(year, month, 1);
          const monthEndUtc = Date.UTC(year, month, daysInMonth);
          const stepMs = 7 * 86400000;
          let curUtc = anchorUtc;
          if (curUtc < monthStartUtc) {
            const k = Math.ceil((monthStartUtc - curUtc) / stepMs);
            curUtc += k * stepMs;
          }
          while (curUtc <= monthEndUtc) {
            const dObj = new Date(curUtc);
            const wDay = dObj.getUTCDate();
            if (curUtc >= anchorUtc && wDay !== matchedRealDay) {
              addEvt(wDay, { kind: "projected", source_id: item.source_id, title, money, currency: amtVal.currency, cents, sub: ann.account_label ? `按周期推算 · ${ann.account_label}` : "按周期推算" });
            }
            curUtc += stepMs;
          }
        } else {
          if (realMonthKeys.has(subKey) && !perVal?.period_end) continue;
          const diff = (year * 12 + month) - (ay * 12 + (am - 1));
          const clampedDay = Math.min(ad, daysInMonth);
          if (diff >= 0 && clampedDay !== matchedRealDay && !(realMonthKeys.has(subKey) && diff === 0)) {
            let hits = false;
            if (ann.billing_cycle === "monthly") hits = true;
            else if (ann.billing_cycle === "quarterly" && diff % 3 === 0) hits = true;
            else if (ann.billing_cycle === "yearly" && diff % 12 === 0) hits = true;
            if (hits) {
              addEvt(clampedDay, { kind: "projected", source_id: item.source_id, title, money, currency: amtVal.currency, cents, sub: ann.account_label ? `按周期推算 · ${ann.account_label}` : "按周期推算" });
            }
          }
        }
      }
    }

    agenda.sort((a, b) => a.day - b.day);
    const realTotals = {};
    const projTotals = {};
    for (const ev of agenda) {
      if (!ev.currency || !ev.cents) continue;
      if (ev.kind === "real") realTotals[ev.currency] = (realTotals[ev.currency] || 0) + ev.cents;
      else if (ev.kind === "projected") projTotals[ev.currency] = (projTotals[ev.currency] || 0) + ev.cents;
    }
    const realRows = Object.entries(realTotals);
    const projRows = Object.entries(projTotals);
    const monthFootHtml = (realRows.length || projRows.length)
      ? `
        <div class="agenda-total-foot">
          ${realRows.length ? `<div style="font-size:11.5px;font-weight:700;color:var(--ink-3)">${month + 1} 月已录账单</div>${realRows.map(([cur, c]) => `<div class="agenda-total-row"><span>${esc(cur)} · ${esc(CURRENCY_NAMES[cur] || cur)}</span><strong class="sensitive">${esc(formatMoneyShort(cur, centsToText(c)))}</strong></div>`).join("")}` : ""}
          ${projRows.length ? `<div style="font-size:11.5px;font-weight:700;color:var(--ink-3);margin-top:${realRows.length ? "6px" : "0"}">按周期推算参考（未确认扣费）</div>${projRows.map(([cur, c]) => `<div class="agenda-total-row"><span>${esc(cur)} · ${esc(CURRENCY_NAMES[cur] || cur)}</span><strong class="sensitive">${esc(formatMoneyShort(cur, centsToText(c)))}</strong></div>`).join("")}` : ""}
        </div>
      `
      : "";

    let cells = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"].map((w) => `<div class="cal-wk">${w}</div>`).join("");
    for (let i = startWeekday - 1; i >= 0; i--) cells += `<div class="cal-cell other"><div class="cal-day">${prevMonthDays - i}</div></div>`;
    for (let d = 1; d <= daysInMonth; d++) {
      const isToday = today.getFullYear() === year && today.getMonth() === month && today.getDate() === d;
      const evts = eventsByDay[d] || [];
      const dayIso = `${year}-${String(month + 1).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
      cells += `
        <div class="cal-cell ${isToday ? "today" : ""}">
          <div class="cal-day-row">
            <span class="cal-day">${d}${isToday ? " · 今天" : ""}</span>
            <button type="button" class="cal-add-btn" data-cal-add-date="${dayIso}" title="在 ${dayIso} 记一笔">+</button>
          </div>
          ${evts.map((ev) => `<button type="button" class="cal-evt ${ev.kind}" data-cal-sid="${esc(ev.source_id)}"><span class="cal-evt-name">${esc(ev.title)}</span><span class="${ev.kind === "warranty" ? "" : "sensitive num"}">${esc(ev.money)}</span></button>`).join("")}
        </div>
      `;
    }
    const trailing = (7 - ((startWeekday + daysInMonth) % 7)) % 7;
    for (let nd = 1; nd <= trailing; nd++) cells += `<div class="cal-cell other"><div class="cal-day">${nd}</div></div>`;

    const agendaHtml = agenda.length === 0
      ? `<p class="folio-p">本月没有账单、周期参考或保修到期。</p>`
      : `<div class="agenda-list">${agenda.map((ev) => `
          <div class="agenda-row" tabindex="0" role="button" data-cal-sid="${esc(ev.source_id)}">
            <div style="display:flex;align-items:center;gap:10px;min-width:0">
              <div class="agenda-date-pill">${ev.day}</div>
              <div style="min-width:0">
                <div style="font-weight:700;font-size:13px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(ev.title)}</div>
                <div style="font-size:11.5px;color:var(--ink-3)">${esc(ev.sub)}</div>
              </div>
            </div>
            <span class="micro-pill ${ev.kind === "warranty" ? "iris" : ev.kind === "projected" ? "amber" : "slate"} ${ev.kind === "warranty" ? "" : "sensitive num"}">${esc(ev.money)}</span>
          </div>
        `).join("")}</div>`;

    pane.innerHTML = `
      <div class="cal-layout">
        <div class="cal-sheet">
          <div class="cal-header">
            <div class="cal-nav">
              <button type="button" id="cal-prev" class="btn btn-soft btn-sm">上月</button>
              <h3 class="cal-month-title">${year} 年 ${month + 1} 月</h3>
              <button type="button" id="cal-next" class="btn btn-soft btn-sm">下月</button>
              <button type="button" id="cal-today" class="btn btn-ghost btn-sm">本月</button>
            </div>
            <div class="cal-legend">
              <span><i class="ldot real"></i>账单日</span>
              <span><i class="ldot proj"></i>按周期推算</span>
              <span><i class="ldot war"></i>保修截止</span>
            </div>
          </div>
          <div class="cal-grid">${cells}</div>
        </div>

        <aside class="agenda-card">
          <div>
            <h3 class="folio-h3">${month + 1} 月日程（${agenda.length} 项）</h3>
          </div>
          ${agendaHtml}
          ${monthFootHtml}
        </aside>
      </div>
    `;

    assignStagger(pane);
    if (state.calDir) {
      pane.querySelector(".cal-grid")?.classList.add(state.calDir > 0 ? "slide-from-right" : "slide-from-left");
      pane.querySelector(".cal-month-title")?.classList.add("flip");
      state.calDir = 0;
    }

    $("cal-prev").addEventListener("click", () => {
      state.calendarMonth -= 1;
      state.calDir = -1;
      if (state.calendarMonth < 0) { state.calendarMonth = 11; state.calendarYear -= 1; }
      renderCalendarPane(ws);
    });
    $("cal-next").addEventListener("click", () => {
      state.calendarMonth += 1;
      state.calDir = 1;
      if (state.calendarMonth > 11) { state.calendarMonth = 0; state.calendarYear += 1; }
      renderCalendarPane(ws);
    });
    $("cal-today").addEventListener("click", () => {
      const n = new Date();
      const diff = (n.getFullYear() * 12 + n.getMonth()) - (state.calendarYear * 12 + state.calendarMonth);
      state.calDir = Math.sign(diff);
      state.calendarYear = n.getFullYear();
      state.calendarMonth = n.getMonth();
      renderCalendarPane(ws);
    });
    pane.querySelectorAll("[data-cal-sid]").forEach((el) => {
      const openCalItem = () => {
        const found = (ws.items || []).find((i) => i.source_id === el.getAttribute("data-cal-sid"));
        if (found) openDrawer(found);
      };
      el.addEventListener("click", openCalItem);
      el.addEventListener("keydown", (e) => {
        if ((e.key === "Enter" || e.key === " ") && e.target === el) {
          e.preventDefault();
          openCalItem();
        }
      });
    });
    pane.querySelectorAll("[data-cal-add-date]").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        openManualModal(btn.getAttribute("data-cal-add-date"));
      });
    });
  }

  // --- View C: Insights, Vendor Folio & Savings Studio ---
  function buildMiniSparkline(history, strokeColor, fallbackDate) {
    if (!history || history.length < 2) {
      return fallbackDate ? `<span style="font-size:11px;color:var(--ink-3)" class="num">${esc(fallbackDate)}</span>` : "";
    }
    const vals = history.slice(-8).map((h) => h.cents || 0);
    const min = Math.min(...vals);
    const max = Math.max(...vals);
    const span = max - min || 1;
    const w = 64, h = 22;
    const pts = vals.map((v, i) => `${((i / (vals.length - 1)) * w).toFixed(1)},${(h - 3 - ((v - min) / span) * (h - 6)).toFixed(1)}`).join(" ");
    const last = pts.split(" ").pop().split(",");
    return `<svg width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" style="overflow:visible"><polyline fill="none" stroke="${strokeColor}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" points="${pts}"/><circle cx="${last[0]}" cy="${last[1]}" r="3" fill="${strokeColor}"/></svg>`;
  }

  function buildSavingsNote(cur, cents, cutCount) {
    if (cutCount === 0 || cents <= 0) return "勾选下方订阅，算算取消后一年能少花多少";
    return `相当于每月少花 ${formatMoneyShort(cur, centsToText(Math.round(cents / 12)))}（已选 ${cutCount} 项）`;
  }

  function renderInsightsPane(ws) {
    const pane = $("pane-insights");
    const curList = (ws.summary_by_currency || []).map((c) => c.currency);
    if (!curList.length) {
      pane.innerHTML = `<div class="empty-gallery"><h3>录入几笔账单后，这里会显示统计</h3><p>按币种查看分类占比、月度支出、商家汇总，以及取消订阅试算。</p></div>`;
      return;
    }

    if (!curList.includes(state.insightsCurrency)) state.insightsCurrency = curList[0];
    const cur = state.insightsCurrency;
    if (state.activeNav === "insights") markScene(`insights|${cur}`);
    const analytics = ws.analytics || { categories: {}, accounts: {}, monthly_trend: {} };
    const catRows = (analytics.categories && analytics.categories[cur]) || [];
    const accRows = (analytics.accounts && analytics.accounts[cur]) || [];
    const monthRows = (analytics.monthly_trend && analytics.monthly_trend[cur]) || [];

    const palette = getChartPalette();
    const totalCat = catRows.reduce((s, r) => s + (r.cents || 0), 0) || 1;
    const r = 66, circ = 2 * Math.PI * r;
    let offset = 0;
    const slicesSvg = catRows.map((row, idx) => {
      const frac = row.cents / totalCat;
      const dash = Math.max(2, frac * circ);
      const seg = `<circle class="donut-seg" cx="85" cy="85" r="${r}" fill="none" stroke="${palette[idx % palette.length]}" stroke-width="16" stroke-dasharray="${dash.toFixed(2)} ${(circ - dash).toFixed(2)}" stroke-dashoffset="${(-offset).toFixed(2)}" transform="rotate(-90 85 85)" data-cat-slice="${esc(row.name)}" data-cat-amt="${esc(row.amount_text)}" data-cat-pct="${Math.round(frac * 100)}" />`;
      offset += dash;
      return seg;
    }).join("");

    const legendHtml = catRows.map((row, idx) => `
      <div class="legend-row" tabindex="0" role="button" data-jump-cat="${esc(row.name)}">
        <div class="legend-l"><span class="swatch" style="background:${palette[idx % palette.length]}"></span><span>${esc(row.name)}</span></div>
        <strong class="sensitive num">${esc(formatMoneyShort(cur, row.amount_text))} · ${Math.round((row.cents / totalCat) * 100)}%</strong>
      </div>
    `).join("");

    const maxMonth = Math.max(...monthRows.map((m) => m.cents || 0), 1);
    const barsHtml = monthRows.length === 0
      ? `<p class="folio-p" style="margin-top:12px">填写账单日期后会显示按月支出。</p>`
      : `<div class="bars-row">${monthRows.slice(-10).map((m) => `
          <div class="bar-col">
            <span class="bar-num sensitive">${esc(formatMoneyShort(cur, m.amount_text))}</span>
            <div class="bar-fill" style="height:${Math.max(8, Math.round((m.cents / maxMonth) * 86))}px"></div>
            <span class="bar-lbl">${esc(m.month)}</span>
          </div>
        `).join("")}</div>`;

    const totalAcc = accRows.reduce((s, r) => s + (r.cents || 0), 0) || 1;
    const allocSegs = accRows.map((row, idx) => `<div class="alloc-seg" style="width:${Math.max(4, Math.round((row.cents / totalAcc) * 100))}%;background:${palette[(idx + 1) % palette.length]}"></div>`).join("");
    const accLegend = accRows.map((row, idx) => `
      <div class="legend-row" tabindex="0" role="button" data-jump-acc="${esc(row.name)}">
        <div class="legend-l"><span class="swatch" style="background:${palette[(idx + 1) % palette.length]}"></span><span class="sensitive">${esc(row.name)}（${row.count} 笔）</span></div>
        <strong class="sensitive num">${esc(formatMoneyShort(cur, row.amount_text))} · ${Math.round((row.cents / totalAcc) * 100)}%</strong>
      </div>
    `).join("");

    const portfolioRows = (ws.vendor_portfolio || []).filter((p) => p.currency === cur);
    const portfolioHtml = portfolioRows.length === 0 ? "" : `
      <div class="folio-card pad-24">
        <h3 class="folio-h3">按商家汇总</h3>
        <p class="folio-p" style="margin-top:2px">点击商家卡片，可筛选该商家的所有记录</p>
        <div class="vendor-grid">
          ${portfolioRows.map((vp) => {
            const brand = brandStyleFor(vp.vendor);
            const km = ASSET_KIND_META[vp.asset_kind] || ASSET_KIND_META.general;
            const drift = vp.price_drift;
            const badge = drift
              ? `<span class="micro-pill ${drift.direction === "up" ? "rose" : "green"}">${drift.direction === "up" ? `+${Math.abs(drift.pct_change)}%` : `-${Math.abs(drift.pct_change)}%`}</span>`
              : `<span class="micro-pill ${km.pill}">${esc(km.label)}</span>`;
            const spark = buildMiniSparkline(vp.history || [], drift?.direction === "up" ? "var(--rose)" : "var(--blue)", vp.latest_date);
            return `
              <div class="vendor-tile" tabindex="0" role="button" data-jump-vendor="${esc(vp.vendor)}">
                <div class="vt-head">
                  <div class="vt-id">
                    <div class="brand-badge brand-badge-sm" style="background:${brand.bg}">${esc(brand.mark)}</div>
                    <div style="min-width:0">
                      <div class="vt-name">${esc(vp.vendor)}</div>
                      <div class="vt-sub sensitive">${vp.bill_count} 笔${(vp.accounts || []).length ? ` · ${esc(vp.accounts.join(" / "))}` : ""}</div>
                    </div>
                  </div>
                  ${badge}
                </div>
                <div class="vt-foot">
                  <div>
                    <div style="font-size:11px;color:var(--ink-3)">累计支出</div>
                    <div class="vt-ltv sensitive">${renderEditorialMoney(cur, vp.total_text, false)}</div>
                  </div>
                  ${spark}
                </div>
              </div>
            `;
          }).join("")}
        </div>
      </div>
    `;

    // Deduplicate recurring subscriptions consistently by (vendor, plan, account, currency, cycle)
    const latestSubMap = new Map();
    for (const it of ws.items || []) {
      const ann = it.annotation || {};
      const amt = it.amount_fact?.value;
      if (!amt || amt.currency !== cur || !["monthly", "yearly", "quarterly", "weekly"].includes(ann.billing_cycle)) continue;
      const key = subscriptionIdentityKey(it);
      const dStr = ann.bill_date || it.period_fact?.value?.period_start || "";
      const prev = latestSubMap.get(key);
      if (!prev || dStr >= prev.dStr) latestSubMap.set(key, { it, dStr });
    }
    const recurring = [...latestSubMap.values()].map((v) => v.it);
    let yearlySave = 0;
    const cutList = [];
    const simRows = recurring.map((it) => {
      const ann = it.annotation || {};
      const amt = it.amount_fact.value;
      const mult = { monthly: 12, yearly: 1, quarterly: 4, weekly: 52 }[ann.billing_cycle] || 0;
      const yrCents = amountToCents(amt.amount_text) * mult;
      const isCut = state.simCutIds.has(it.source_id);
      const subTag = [ann.account_label, ann.plan_name].filter(Boolean).join(" · ");
      if (isCut) {
        yearlySave += yrCents;
        cutList.push(`${ann.vendor || it.display_name}${ann.account_label ? ` [${ann.account_label}]` : ""}（${formatMoney(cur, amt.amount_text)} / ${CYCLE_LABELS[ann.billing_cycle]}）`);
      }
      return `
        <div class="sim-row ${isCut ? "cut" : ""}">
          <div style="min-width:0">
            <div class="sim-title" style="font-weight:700;font-size:13.5px">${esc(ann.vendor || it.display_name)}</div>
            <div style="font-size:12px;color:var(--ink-3)" class="sensitive num">${esc(formatMoneyShort(cur, amt.amount_text))} / ${esc(CYCLE_LABELS[ann.billing_cycle])} · 年合 ${esc(formatMoneyShort(cur, centsToText(yrCents)))}${subTag ? ` · ${esc(subTag)}` : ""}</div>
          </div>
          <label class="ios-switch"><input type="checkbox" aria-label="试算取消 ${esc(ann.vendor || it.display_name)}" data-sim-sid="${esc(it.source_id)}" ${isCut ? "checked" : ""}><span class="ios-slider"></span></label>
        </div>
      `;
    }).join("");

    pane.innerHTML = `
      <div class="insights-stack">
        <div style="display:flex;align-items:center;justify-content:space-between;flex-wrap:wrap;gap:12px">
          <div class="cur-switcher">${curList.map((c) => `<button type="button" class="cur-tab ${c === cur ? "active" : ""}" data-ins-cur="${esc(c)}">${esc(c)} · ${esc(CURRENCY_NAMES[c] || c)}</button>`).join("")}</div>
          <span style="font-size:12px;color:var(--ink-3)">各币种分开统计，不做汇率换算</span>
        </div>

        <div class="insights-top-grid">
          <div class="folio-card pad-24">
            <h3 class="folio-h3">分类占比</h3>
            <div class="donut-wrap">
              <div class="donut-box">
                <svg viewBox="0 0 170 170" width="170" height="170">${slicesSvg}</svg>
                <div class="donut-center">
                  <div id="donut-center-label" class="donut-center-k">${esc(cur)} 总额</div>
                  <div id="donut-center-val" class="donut-center-v sensitive">${esc(formatMoneyShort(cur, centsToText(totalCat)))}</div>
                </div>
              </div>
              <div class="legend-list">${legendHtml}</div>
            </div>
          </div>

          <div class="folio-card pad-24">
            <h3 class="folio-h3">月度支出与账号分布</h3>
            ${barsHtml}
            <div class="alloc-bar">${allocSegs}</div>
            <div class="legend-list">${accLegend}</div>
          </div>
        </div>

        ${portfolioHtml}

        <div class="sim-studio">
          <div class="sim-side">
            <div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap">
              <h3 class="folio-h3" style="margin:0">取消订阅试算</h3>
              <span class="micro-pill slate">仅试算，不修改账本</span>
            </div>
            <div class="sim-figure sensitive">每年少花 <span data-countup="sim-${esc(cur)}" data-cur="${esc(cur)}" data-code="1" data-cents="${yearlySave}">${renderEditorialMoney(cur, centsToText(yearlySave), true)}</span></div>
            <div class="folio-p">${esc(buildSavingsNote(cur, yearlySave, cutList.length))}</div>
            <div class="btn-row" style="margin-top:4px">
              <button type="button" id="btn-sim-copy" class="btn btn-ink btn-sm" ${cutList.length === 0 ? "disabled" : ""}>复制清单 (${cutList.length})</button>
              <button type="button" id="btn-sim-reset" class="btn btn-soft btn-sm" ${cutList.length === 0 ? "disabled" : ""}>重置</button>
            </div>
          </div>
          <div>
            ${recurring.length ? `<div class="sim-grid">${simRows}</div>` : `<p class="folio-p">当前 ${esc(cur)} 下还没有周期订阅。</p>`}
          </div>
        </div>
      </div>
    `;

    runCountUps(pane);
    assignStagger(pane);
    pane.querySelectorAll("[data-ins-cur]").forEach((b) => b.addEventListener("click", () => { state.insightsCurrency = b.getAttribute("data-ins-cur"); renderInsightsPane(ws); }));
    const slices = [...pane.querySelectorAll("[data-cat-slice]")];
    const catLegends = [...pane.querySelectorAll("[data-jump-cat]")];
    const highlightCat = (catName) => {
      slices.forEach((s) => {
        const hit = s.getAttribute("data-cat-slice") === catName;
        s.classList.toggle("is-hover", Boolean(catName && hit));
        s.classList.toggle("dimmed", Boolean(catName && !hit));
        if (hit) {
          $("donut-center-label").textContent = `${catName} · ${s.getAttribute("data-cat-pct")}%`;
          $("donut-center-val").textContent = formatMoneyShort(cur, s.getAttribute("data-cat-amt"));
        }
      });
      catLegends.forEach((r) => r.classList.toggle("is-hover", Boolean(catName && r.getAttribute("data-jump-cat") === catName)));
      if (!catName) {
        $("donut-center-label").textContent = `${cur} 总额`;
        $("donut-center-val").textContent = formatMoneyShort(cur, centsToText(totalCat));
      }
    };
    slices.forEach((c) => {
      c.addEventListener("mouseenter", () => highlightCat(c.getAttribute("data-cat-slice")));
      c.addEventListener("mouseleave", () => highlightCat(""));
      c.addEventListener("click", () => { state.categoryFilter = c.getAttribute("data-cat-slice"); state.currencyFilter = cur; navigateTo("assets", "all"); });
    });
    catLegends.forEach((r) => {
      const jump = () => { state.categoryFilter = r.getAttribute("data-jump-cat"); state.currencyFilter = cur; navigateTo("assets", "all"); };
      r.addEventListener("mouseenter", () => highlightCat(r.getAttribute("data-jump-cat")));
      r.addEventListener("mouseleave", () => highlightCat(""));
      r.addEventListener("click", jump);
      r.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); jump(); } });
    });
    pane.querySelectorAll("[data-jump-acc]").forEach((r) => {
      const jump = () => {
        const a = r.getAttribute("data-jump-acc");
        state.accountFilter = a === "未分组" ? "" : a;
        if ($("filter-account")) $("filter-account").value = state.accountFilter;
        navigateTo("assets", "all");
      };
      r.addEventListener("click", jump);
      r.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); jump(); } });
    });
    pane.querySelectorAll("[data-jump-vendor]").forEach((t) => {
      const jump = () => {
        const v = t.getAttribute("data-jump-vendor") || "";
        state.searchQuery = v;
        if ($("search-box")) $("search-box").value = v;
        navigateTo("assets", "all");
      };
      t.addEventListener("click", jump);
      t.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); jump(); } });
    });
    pane.querySelectorAll("[data-sim-sid]").forEach((chk) => chk.addEventListener("change", () => {
      const sid = chk.getAttribute("data-sim-sid");
      if (chk.checked) {
        state.simCutIds.add(sid);
        burstFrom(chk.closest(".ios-switch"), { count: 12, shape: "coin", spread: 1.6, symbol: coinSymbol(cur) });
      } else state.simCutIds.delete(sid);
      renderInsightsPane(ws);
    }));
    if ($("btn-sim-reset")) $("btn-sim-reset").addEventListener("click", () => { state.simCutIds.clear(); renderInsightsPane(ws); });
    if ($("btn-sim-copy")) $("btn-sim-copy").addEventListener("click", async () => {
      const text = `【SubVault 待取消订阅清单（${cur}）】\n预计每年少花：${formatMoney(cur, centsToText(yearlySave))}\n` + cutList.map((n, i) => `${i + 1}. ${n}`).join("\n");
      try { await navigator.clipboard.writeText(text); showToast("已复制清单", "ok"); }
      catch (_) { downloadTextFile("subvault-cancel-plan.txt", text); }
    });
  }

  // --- View D: Light-Table Triage ---
  function getTriageQueue(ws) {
    return (ws.items || []).filter((i) => (i.status_flags.needs_manual_entry || i.status_flags.has_conflict) && !state.triageSkippedIds.has(i.source_id));
  }

  function renderTriagePane(ws) {
    const pane = $("pane-triage");
    const queue = getTriageQueue(ws);
    if (state.activeNav === "triage") markScene(`triage|${queue[0]?.source_id || "done"}`);
    if (!queue.length) {
      const skipped = state.triageSkippedIds.size;
      pane.innerHTML = `
        <div class="empty-gallery">
          <div class="empty-emblem twinkle" style="background:var(--green-bg);color:var(--green)">${ICO.sparkle}</div>
          <h3>待处理项已清空</h3>
          <p>以后导入扫描件、截图或金额有歧义的账单时，可以在这里对照原件补充信息。</p>
          <div class="btn-row" style="justify-content:center">
            ${skipped > 0 ? `<button type="button" id="btn-reset-skip" class="btn btn-soft btn-sm">重新查看跳过的 ${skipped} 项</button>` : `<button type="button" id="btn-back-assets" class="btn btn-ink btn-sm">返回账本</button>`}
          </div>
        </div>
      `;
      if ($("btn-reset-skip")) $("btn-reset-skip").addEventListener("click", () => { state.triageSkippedIds.clear(); renderTriagePane(ws); });
      if ($("btn-back-assets")) $("btn-back-assets").addEventListener("click", () => navigateTo("assets", "all"));
      return;
    }

    const item = queue[0];
    const ann = item.annotation || {};
    const baseMime = (item.media_type || "").split(";")[0].trim().toLowerCase();
    const isImage = ["image/png", "image/jpeg", "image/webp"].includes(baseMime);
    const hasRichPreview = baseMime === "application/pdf" || isImage || baseMime === "text/html";
    const previewHtml = hasRichPreview
      ? `<div class="preview-loading" style="padding:24px;text-align:center;color:var(--ink-3);font-size:13px">原件加载中…</div>`
      : `<pre id="triage-text-preview" class="code-sheet" style="margin:0">加载中…</pre>`;

    const defaultCur = state.heroCurrency || "CNY";
    const curs = (state.bootstrap?.currencies || ["CNY", "USD", "EUR", "GBP", "JPY", "HKD"])
      .map((c) => `<option value="${esc(c)}" ${c === defaultCur ? "selected" : ""}>${esc(c)} ${esc(CURRENCY_SYMBOLS[c] || "")}</option>`).join("");
    const initialCycle = (ann.billing_cycle && ann.billing_cycle !== "unknown") ? ann.billing_cycle : "monthly";
    const initialAcc = ann.account_label || "个人";

    pane.innerHTML = `
      <div class="folio-card triage-bar">
        <div style="display:flex;align-items:center;gap:10px">
          <span class="micro-pill amber">剩余 ${queue.length} 份</span>
          <strong>${esc(item.display_name)}</strong>
          <span style="font-size:12px;color:var(--ink-3)">填完按 Enter 自动切下一张</span>
        </div>
        <button type="button" id="btn-triage-skip" class="btn btn-ghost btn-sm">暂时跳过</button>
      </div>
      <div class="triage-split">
        <div class="folio-card pad-24">
          <div class="micro-eyebrow" style="margin-bottom:10px">原件预览 · ${formatBytes(item.byte_length)}</div>
          <div class="preview-frame ${isImage ? "is-image" : ""}" data-triage-preview style="margin-bottom:0">${previewHtml}</div>
        </div>
        <div class="folio-card pad-24">
          <h3 class="folio-h3" style="margin-bottom:14px">补充信息</h3>
          ${item.status_flags.has_conflict ? `<div class="warning-note" style="margin-bottom:12px">检测到多个候选金额，请对照左侧原件填写真实金额。</div>` : ""}
          <form id="form-triage" class="form-stack">
            <label class="field"><span>商家 / 服务名</span><input id="tr-vendor" type="text" value="${esc(ann.vendor || "")}" placeholder="如 JetBrains、Apple、电信宽带" required></label>
            <div class="grid-2">
              <label class="field"><span>币种</span><select id="tr-currency">${curs}</select></label>
              <label class="field"><span>金额</span><input id="tr-amount" type="text" placeholder="如 98.00" required></label>
            </div>
            <div class="chip-strip">
              <span class="chip-strip-lbl">周期</span>
              ${["monthly", "yearly", "quarterly", "one_time"].map((c) => `<button type="button" class="tap-chip ${initialCycle === c ? "active" : ""}" data-tr-cycle="${c}">${CYCLE_LABELS[c]}</button>`).join("")}
            </div>
            <div class="chip-strip">
              <span class="chip-strip-lbl">账号</span>
              ${["个人", "公司工作", "家庭共享"].map((a) => `<button type="button" class="tap-chip ${initialAcc === a ? "active" : ""}" data-tr-acc="${a}">${a}</button>`).join("")}
            </div>
            <div class="chip-strip">
              <span class="chip-strip-lbl">分类</span>
              ${["AI 工具", "数码硬件", "云存储", "开发工具", "办公效率"].map((k) => `<button type="button" class="tap-chip ${ann.category === k ? "active" : ""}" data-tr-cat="${k}">${k}</button>`).join("")}
            </div>
            <div class="grid-2">
              <label class="field"><span>归属账号</span><input id="tr-account" type="text" value="${esc(initialAcc)}"></label>
              <label class="field"><span>分类</span><input id="tr-category" type="text" value="${esc(ann.category || "")}"></label>
            </div>
            <label class="field"><span>账单日期（可选）</span><input id="tr-date" type="date" value="${esc(ann.bill_date || "")}"></label>
            <button id="btn-triage-save" class="btn btn-ink btn-lg btn-block" type="submit">保存并切下一张</button>
          </form>
        </div>
      </div>
    `;

    if (hasRichPreview) {
      loadAuthedPreviewInto(pane.querySelector("[data-triage-preview]"), item);
    } else if ($("triage-text-preview")) {
      const targetSid = item.source_id;
      api("GET", `/api/sources/${encodeURIComponent(targetSid)}/text`)
        .then((res) => {
          const el = $("triage-text-preview");
          const curQueue = getTriageQueue(state.workspace || {});
          if (el && curQueue[0]?.source_id === targetSid) el.textContent = res.text || "";
        })
        .catch(() => {});
    }
    let chosenCycle = initialCycle;
    pane.querySelectorAll("[data-tr-cycle]").forEach((b) => b.addEventListener("click", () => {
      chosenCycle = b.getAttribute("data-tr-cycle");
      pane.querySelectorAll("[data-tr-cycle]").forEach((x) => x.classList.toggle("active", x === b));
    }));
    pane.querySelectorAll("[data-tr-acc]").forEach((b) => b.addEventListener("click", () => {
      $("tr-account").value = b.getAttribute("data-tr-acc");
      pane.querySelectorAll("[data-tr-acc]").forEach((x) => x.classList.toggle("active", x === b));
    }));
    pane.querySelectorAll("[data-tr-cat]").forEach((b) => b.addEventListener("click", () => {
      $("tr-category").value = b.getAttribute("data-tr-cat");
      pane.querySelectorAll("[data-tr-cat]").forEach((x) => x.classList.toggle("active", x === b));
    }));
    $("btn-triage-skip").addEventListener("click", () => { state.triageSkippedIds.add(item.source_id); renderTriagePane(ws); });
    $("form-triage").addEventListener("submit", async (e) => {
      e.preventDefault();
      const btn = $("btn-triage-save");
      btn.disabled = true; btn.textContent = "保存中…";
      try {
        const snap = await api("POST", "/api/sources/save", {
          source_id: item.source_id, vendor: $("tr-vendor").value, account_label: $("tr-account").value,
          billing_cycle: chosenCycle, currency: $("tr-currency").value, amount_text: $("tr-amount").value,
          category: $("tr-category").value, bill_date: $("tr-date").value,
        });
        state.workspace = snap; renderWorkspace();
        if (getTriageQueue(snap).length === 0) { launchConfetti(); showToast("待整理项全部清空，干得漂亮", "ok"); }
        else showToast("已保存，进入下一项", "ok");
      } catch (err) {
        showToast(err.message, "error"); btn.disabled = false; btn.textContent = "保存并切下一张";
      }
    });
    $("tr-vendor").focus();
  }

  // --- View E: Security & Dedup Attention ---
  function renderSecurityAttention(ws) {
    const wrap = $("security-attention-wrap");
    const items = ws.items || [];
    const needs = items.filter((i) => i.status_flags.needs_manual_entry || i.status_flags.has_conflict || i.status_flags.is_credit_or_order_warning);
    const dups = items.filter((i) => i.status_flags.has_duplicate);

    const needsHtml = !needs.length ? `<p class="folio-p">目前没有待补充或金额有歧义的文件。</p>` : `<div class="stack-8">${needs.map((it) => `
      <div class="attn-row">
        <div>
          <div style="font-weight:700">${esc(it.annotation?.vendor || it.display_name)}</div>
          <div style="font-size:12px;color:var(--ink-3)">${it.status_flags.has_conflict ? "存在多个候选金额，需要确认" : it.status_flags.is_credit_or_order_warning ? "疑似退款或未支付订单" : "扫描件或截图，点击补充信息"}</div>
        </div>
        <button type="button" class="btn btn-soft btn-sm" data-open-sid="${esc(it.source_id)}">核对</button>
      </div>
    `).join("")}</div>`;

    const dupsHtml = !dups.length ? `<p class="folio-p">目前没有重复导入的文件。同一份文件改名后再导入，也会自动跳过。</p>` : `<div class="stack-8">${dups.map((it) => `
      <div class="attn-row">
        <div>
          <div style="font-weight:700">${esc(it.annotation?.vendor || it.display_name)}</div>
          <div style="font-size:12px;color:var(--ink-3)">共导入 ${it.occurrence_count} 次，跳过重复 ${it.duplicate_hits} 次 · ${esc((it.occurrences || []).map((o) => o.display_name).join("、"))}</div>
        </div>
        <button type="button" class="btn btn-soft btn-sm" data-open-sid="${esc(it.source_id)}">查看</button>
      </div>
    `).join("")}</div>`;

    wrap.innerHTML = `
      <div class="sec-grid-2">
        <div class="folio-card pad-24 stack-12">
          <div style="display:flex;align-items:center;justify-content:space-between"><h3 class="folio-h3">待核对文件</h3><span class="micro-pill ${needs.length ? "amber" : "slate"}">${needs.length}</span></div>
          ${needsHtml}
        </div>
        <div class="folio-card pad-24 stack-12">
          <div style="display:flex;align-items:center;justify-content:space-between"><h3 class="folio-h3">重复导入记录</h3><span class="micro-pill ${dups.length ? "green" : "slate"}">${dups.length}</span></div>
          ${dupsHtml}
        </div>
      </div>
    `;
    assignStagger(wrap);
    wrap.querySelectorAll("[data-open-sid]").forEach((btn) => btn.addEventListener("click", () => {
      const found = items.find((i) => i.source_id === btn.getAttribute("data-open-sid"));
      if (found) openDrawer(found);
    }));
  }

  function trapFocusIn(container, e) {
    if (e.key !== "Tab" || !container || container.classList.contains("hidden")) return;
    const focusable = [...container.querySelectorAll('button:not([disabled]), [href], input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])')]
      .filter((el) => el.offsetParent !== null);
    if (!focusable.length) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  // --- Floating Detail Drawer ---
  function openDrawer(item, defaultTab) {
    if (!state.selectedSourceId) state.drawerReturnFocus = document.activeElement;
    state.selectedSourceId = item.source_id;
    state.textViewActive = false;
    state.lastRedactedText = "";
    $("redact-result-wrap").classList.add("hidden");
    $("redact-custom-terms").value = "";
    $("delete-confirm-bar").classList.add("hidden");
    $("btn-ask-delete-item").classList.remove("hidden");
    $("drawer-backdrop").classList.remove("hidden", "closing");
    $("detail-drawer").classList.remove("hidden", "closing");

    const isCsv = item.status_flags && item.status_flags.is_csv_statement;
    $("dtab-csv-btn").classList.toggle("hidden", !isCsv);
    renderDrawer(item);
    switchDrawerTab(defaultTab || (isCsv ? "csv" : item.status_flags?.needs_manual_entry ? "edit" : "summary"));
    requestAnimationFrame(() => $("btn-close-drawer")?.focus());
  }

  function closeDrawer() {
    const returnTo = state.drawerReturnFocus;
    state.drawerReturnFocus = null;
    state.selectedSourceId = null;
    revokeBlobUrls();
    const drawer = $("detail-drawer");
    const back = $("drawer-backdrop");
    if (drawer.classList.contains("hidden") || !motionOK()) {
      drawer.classList.add("hidden");
      back.classList.add("hidden");
      if (returnTo && returnTo.isConnected) try { returnTo.focus(); } catch (_) {}
      return;
    }
    drawer.classList.add("closing");
    back.classList.add("closing");
    setTimeout(() => {
      drawer.classList.remove("closing");
      back.classList.remove("closing");
      if (state.selectedSourceId) return;
      drawer.classList.add("hidden");
      back.classList.add("hidden");
      if (returnTo && returnTo.isConnected) try { returnTo.focus(); } catch (_) {}
    }, 190);
  }

  function switchDrawerTab(tab) {
    state.activeDrawerTab = tab;
    document.querySelectorAll(".dtab").forEach((b) => b.classList.toggle("active", b.getAttribute("data-dtab") === tab));
    ["summary", "edit", "evidence", "csv", "history", "redact"].forEach((n) => $(`dpane-${n}`).classList.toggle("hidden", n !== tab));
    if (tab === "evidence" && state.selectedSourceId) loadEvidencePreview(state.selectedSourceId);
  }

  // 详情“先读后改”：大号金额、关键信息、原件与校验结果、同一订阅的历史账单
  function renderDrawerSummary(item) {
    const box = $("dpane-summary");
    const ann = item.annotation || {};
    const amt = item.amount_fact?.value;
    const per = item.period_fact?.value;
    const flags = item.status_flags || {};
    const kind = ann.asset_kind || "general";
    const km = ASSET_KIND_META[kind] || ASSET_KIND_META.general;
    const group = groupOfItem(item.source_id);
    const cycleLbl = kind === "hardware" ? "一次购入" : kind === "license" ? "终身买断" : (CYCLE_LABELS[ann.billing_cycle] || "未标注周期");

    const chips = [`<span class="micro-pill ${km.pill}">${esc(km.label)}</span>`];
    if (item.price_drift) {
      const up = item.price_drift.direction === "up";
      chips.push(`<span class="micro-pill ${up ? "rose" : "green"}">较上期 ${up ? "↑" : "↓"} ${Math.abs(item.price_drift.pct_change)}%</span>`);
    }
    if (flags.has_conflict) chips.push(`<span class="micro-pill amber">金额待确认</span>`);
    if (flags.needs_manual_entry) chips.push(`<span class="micro-pill amber">缺少金额</span>`);
    if (flags.is_credit_or_order_warning) chips.push(`<span class="micro-pill amber">退款 / 未付订单，未计入</span>`);
    const st = reimbStatusOf(item);
    if (st) chips.push(`<span class="micro-pill ${st === "reimbursed" ? "green" : st === "submitted" ? "blue" : "amber"}">报销 · ${esc(REIMBURSE_META[st].label)}</span>`);

    const rows = [];
    const add = (k, v, sensitive) => { if (v) rows.push(`<dt>${k}</dt><dd class="${sensitive ? "sensitive" : ""}">${v}</dd>`); };
    add("账号", esc(ann.account_label), true);
    add("分类", esc(ann.category));
    add("账单日期", esc(ann.bill_date));
    if (per?.period_start) add("服务周期", `${esc(per.period_start)} — ${esc(per.period_end)}`);
    if (group?.renewal && !group.renewal.stale) add("周期参考", `${esc(group.renewal.date)} · ${relDayLabel(group.renewal.daysLeft)}（仅据账单周期）`);
    if (group?.renewal?.stale) add("账单状态", `最近账单期至 ${esc(group.renewal.last)}（未录新账单，不代表当前是否扣费）`);
    if (kind === "hardware") {
      const w = warrantyInfo(ann.bill_date, ann.warranty_end);
      if (w) add("保修", `${esc(ann.warranty_end)} · ${esc(w.statusLabel)}`);
      const cpd = amt ? costPerDayInfo(ann.bill_date, amt.currency, amt.amount_text) : null;
      if (cpd) add("日均成本", `${esc(cpd.perDayText)} · 已用 ${cpd.daysUsed} 天`, true);
    }
    add("授权范围", esc(ann.license_scope));
    if (ann.license_key) add("密钥", `<span class="ds-key">${esc(maskLicenseKey(ann.license_key))}</span> <button type="button" class="copy-key-pill" data-ds-copy-key>${ICO.copy} 复制</button>`, true);
    add("备注", esc(ann.notes));
    add("原件", `${esc(item.display_name)} · ${formatBytes(item.byte_length)}`);

    const baseMime = (item.media_type || "").split(";")[0].trim().toLowerCase();
    const isImg = ["image/png", "image/jpeg", "image/webp"].includes(baseMime);
    const hasRichPreview = baseMime === "application/pdf" || isImg || baseMime === "text/html";
    const preview = hasRichPreview
      ? `<div class="preview-loading" style="padding:24px;text-align:center;color:var(--ink-3);font-size:13px">原件加载中…</div>`
      : `<pre class="ds-text" data-ds-text>加载中…</pre>`;

    const bills = group && group.items.length > 1
      ? `
        <section class="ds-bills">
          <div class="ds-section-title">这个订阅的账单 · ${group.items.length}</div>
          ${group.items.map((it) => {
            const p = it.period_fact?.value;
            const a = it.amount_fact?.value;
            const current = it.source_id === item.source_id;
            return `<button type="button" class="ds-bill ${current ? "current" : ""}" data-ds-open-sid="${esc(it.source_id)}" ${current ? "disabled" : ""}><span>${esc(p?.period_start ? `${p.period_start} — ${p.period_end}` : itemDate(it) || it.display_name)}</span><span class="sensitive num">${esc(a ? formatMoneyShort(a.currency, a.amount_text) : "缺少金额")}</span><span class="ds-bill-file">${current ? "当前" : esc(it.display_name)}</span></button>`;
          }).join("")}
        </section>`
      : "";

    box.innerHTML = `
      <div class="ds-hero">
        <div class="ds-amount sensitive">${amt?.amount_text ? renderEditorialMoney(amt.currency, amt.amount_text, !CURRENCY_SYMBOLS[amt.currency]) : `<span style="color:var(--ink-3)">缺少金额</span>`}</div>
        <div class="ds-cycle">${esc(cycleLbl)}${ann.plan_name ? ` · ${esc(ann.plan_name)}` : ""}</div>
        <div class="ds-chips">${chips.join("")}</div>
      </div>
      <div class="ds-grid">
        <dl class="ds-facts">${rows.join("")}</dl>
        <div class="ds-proof">
          <div class="preview-frame ds-preview ${isImg ? "is-image" : ""}" data-ds-preview>${preview}</div>
          <div class="ds-integrity ${item.object_intact ? "ok" : "bad"}">${ICO.shield}<span>${item.object_intact ? "SHA-256 校验一致，原件未改动" : "校验异常，请检查账本"}</span></div>
        </div>
      </div>
      ${bills}
      <div class="ds-actions">
        <button type="button" class="btn btn-ink btn-sm" data-ds-tab="edit">编辑</button>
        <button type="button" class="btn btn-soft btn-sm" data-ds-tab="evidence">原件与校验码</button>
        <button type="button" class="btn btn-ghost btn-sm" data-ds-dl-raw>下载原件</button>
      </div>`;

    box.querySelectorAll("[data-ds-tab]").forEach((b) => b.addEventListener("click", () => switchDrawerTab(b.getAttribute("data-ds-tab"))));
    const dlRawBtn = box.querySelector("[data-ds-dl-raw]");
    if (dlRawBtn) dlRawBtn.addEventListener("click", () => downloadAuthedFile(`/api/sources/${encodeURIComponent(item.source_id)}/raw?download=1`, item.display_name));
    box.querySelectorAll("[data-ds-open-sid]").forEach((b) => b.addEventListener("click", () => {
      const it = (state.workspace?.items || []).find((i) => i.source_id === b.getAttribute("data-ds-open-sid"));
      if (it) openDrawer(it);
    }));
    const keyBtn = box.querySelector("[data-ds-copy-key]");
    if (keyBtn) keyBtn.addEventListener("click", () => {
      const plate = box.querySelector(".ds-key");
      if (plate) scrambleText(plate, maskLicenseKey(ann.license_key), 620);
      copyLicenseKey(ann.vendor || item.display_name, ann.license_key, keyBtn);
    });
    if (hasRichPreview) {
      loadAuthedPreviewInto(box.querySelector("[data-ds-preview]"), item);
    }
    const textBox = box.querySelector("[data-ds-text]");
    if (textBox) {
      const targetSid = item.source_id;
      api("GET", `/api/sources/${encodeURIComponent(targetSid)}/text`)
        .then((res) => { if (state.selectedSourceId === targetSid) textBox.textContent = res.text || "（无纯文本预览）"; })
        .catch(() => { if (state.selectedSourceId === targetSid) textBox.textContent = "（无法预览）"; });
    }
  }

  function setEditAssetKind(kind, userClicked) {
    state.editKind = kind || "general";
    document.querySelectorAll("[data-ed-kind]").forEach((b) => b.classList.toggle("active", b.getAttribute("data-ed-kind") === state.editKind));
    const curReimb = $("ed-reimburse-status").value || "none";
    $("ed-panel-license").classList.toggle("hidden", state.editKind !== "license");
    $("ed-panel-hardware").classList.toggle("hidden", state.editKind !== "hardware");
    $("ed-panel-reimb").classList.toggle("hidden", state.editKind !== "reimbursable" && curReimb === "none");

    const hasPeriod = Boolean($("ed-period-start").value || $("ed-period-end").value);
    $("ed-period-row").classList.toggle("hidden", (state.editKind === "hardware" || state.editKind === "license") && !hasPeriod);

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
    if (state.editKind !== "hardware") { box.classList.add("hidden"); return; }
    const buy = $("ed-bill-date").value, wend = $("ed-warranty-end").value, cur = $("ed-currency").value || "CNY", amt = $("ed-amount").value;
    const pills = [];
    const w = warrantyInfo(buy, wend);
    if (w) pills.push(`<span class="micro-pill ${w.daysLeft <= 60 ? "amber" : "iris"}">${esc(w.statusLabel)}</span>`);
    const cpd = costPerDayInfo(buy, cur, amt);
    if (cpd) pills.push(`<span class="micro-pill slate sensitive num">已用 ${cpd.daysUsed} 天 · 日均 ${esc(cpd.perDayText)}</span>`);
    if (pills.length) { box.innerHTML = pills.join(""); box.classList.remove("hidden"); }
    else box.classList.add("hidden");
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
    $("drawer-stamp").className = `micro-pill ${item.object_intact ? "green" : "rose"}`;

    const warns = [];
    if (item.status_flags.has_conflict) warns.push("该文件包含多个不同的金额候选，请核对原件后在下方保存。");
    if (item.status_flags.is_credit_or_order_warning) warns.push("疑似退款单或未支付订单，系统未计入花费。");
    if (item.status_flags.needs_manual_entry) warns.push("尚未记录金额（常见于扫描件或截图），补填信息即可。");
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
      <div><strong>SHA-256：</strong><code>${esc(item.content_sha256)}</code></div>
      <div><strong>完整性：</strong>${item.object_intact ? "校验一致，原件未改动" : "校验异常，请检查账本"}</div>
    `;
    $("btn-download-raw").onclick = () => downloadAuthedFile(`/api/sources/${encodeURIComponent(item.source_id)}/raw?download=1`, item.display_name);

    $("evidence-occurrences").innerHTML = `
      <div class="micro-eyebrow" style="margin:6px 0 2px">导入记录（共 ${item.occurrence_count} 次）</div>
      ${(item.occurrences || []).map((o) => `<div class="occ-row"><strong>${esc(o.display_name)}</strong> · ${formatTime(o.recorded_at)} <span class="micro-pill ${o.storage_reused ? "slate" : "green"}" style="margin-left:6px">${o.storage_reused ? "防重复用" : "首次入库"}</span></div>`).join("")}
    `;

    if (item.status_flags.is_csv_statement) renderCsvRows(item);
    renderHistoryTimeline(item);
    renderDrawerSummary(item);
  }

  async function loadEvidencePreview(sourceId) {
    const item = (state.workspace?.items || []).find((i) => i.source_id === sourceId);
    if (!item) return;
    const box = $("evidence-preview-box"), textBox = $("evidence-text-box");
    const baseMime = (item.media_type || "").split(";")[0].trim().toLowerCase();
    const isImg = ["image/png", "image/jpeg", "image/webp"].includes(baseMime);
    const hasRichPreview = baseMime === "application/pdf" || isImg || baseMime === "text/html";
    box.classList.toggle("is-image", isImg);
    if (hasRichPreview) {
      box.classList.remove("hidden");
      loadAuthedPreviewInto(box, item);
    } else {
      box.classList.add("hidden");
      state.textViewActive = true;
    }
    textBox.classList.toggle("hidden", !state.textViewActive);
    textBox.textContent = "加载中…";
    try {
      const res = await api("GET", `/api/sources/${encodeURIComponent(sourceId)}/text`);
      if (state.selectedSourceId === sourceId) {
        textBox.textContent = res.text || "（无纯文本预览）";
      }
    } catch (err) {
      if (state.selectedSourceId === sourceId) {
        textBox.textContent = `文本加载失败：${err.message}`;
      }
    }
  }

  function renderCsvRows(item) {
    const box = $("csv-rows-container");
    const q = ($("csv-search").value || "").trim().toLowerCase();
    const expenseOnly = $("chk-csv-expense-only").checked;
    const rows = (item.candidates || []).filter((c) => c.kind === "csv_transaction").map((c) => ({ ...c.value, raw_line: c.excerpt })).filter((r) => {
      if (expenseOnly && r.direction !== "expense") return false;
      if (q && !`${r.merchant || ""} ${r.description || ""} ${r.date || ""} ${r.amount_text || ""}`.toLowerCase().includes(q)) return false;
      return true;
    });
    if (!rows.length) { box.innerHTML = `<p class="folio-p">没有符合条件的流水记录。</p>`; return; }
    const limit = Number(box.dataset.csvLimit || "100");
    const visible = rows.slice(0, limit);
    const moreHtml = rows.length > limit
      ? `<div style="text-align:center;padding-top:8px"><button type="button" class="btn btn-soft btn-sm" id="btn-drawer-csv-more">加载更多（剩余 ${rows.length - limit} 条）</button></div>`
      : "";
    box.innerHTML = visible.map((r, idx) => {
      const isNonExpense = ["income", "refund", "neutral"].includes(r.direction);
      const dirLabel = { income: "收入", refund: "退款", neutral: "中性交易" }[r.direction] || "";
      const actHtml = isNonExpense
        ? `<span class="micro-pill slate">${esc(dirLabel)} · 不计支出</span>`
        : `<button type="button" class="btn btn-soft btn-sm" data-csv-promote="${idx}">加入账本</button>`;
      return `
        <div class="csv-row">
          <div>
            <strong>${esc(r.merchant || "未标注商户")} · <span class="sensitive num">${esc(formatMoney(r.currency || "CNY", r.amount_text))}</span></strong>
            <small>${esc(r.date || "")} · ${esc(r.description || "")}</small>
          </div>
          ${actHtml}
        </div>
      `;
    }).join("") + moreHtml;
    const moreBtn = $("btn-drawer-csv-more");
    if (moreBtn) {
      moreBtn.addEventListener("click", () => {
        box.dataset.csvLimit = String(limit + 150);
        renderCsvRows(item);
      });
    }
    box.querySelectorAll("[data-csv-promote]").forEach((btn) => btn.addEventListener("click", async () => {
      const r = rows[Number(btn.getAttribute("data-csv-promote"))];
      if (!r) return;
      btn.disabled = true; btn.textContent = "加入中…";
      try {
        const res = await api("POST", "/api/csv/promote", {
          parent_source_id: item.source_id, row_number: r.row_number, merchant: r.merchant || "", description: r.description || "",
          date: r.date || "", currency: r.currency || "CNY", amount_text: r.amount_text || "", direction: r.direction || "", billing_cycle: "unknown",
          account_label: r.account_hint || undefined, raw_line: r.raw_line || "",
        });
        showToast(res.storage_reused ? "这条流水已经加过，已跳过重复" : `已将「${r.merchant || "该笔流水"}」加入账本`, "ok");
        await refreshWorkspace();
      } catch (err) { showToast(err.message, "error"); }
      finally { btn.disabled = false; btn.textContent = "加入账本"; }
    }));
  }

  function renderHistoryTimeline(item) {
    const el = $("history-timeline");
    const blocks = [];
    for (const fact of [item.amount_fact, item.period_fact].filter(Boolean)) {
      const decs = fact.decisions || [];
      const lbl = fact.fact_type === "stated_amount_paid" ? "金额" : "服务周期";
      decs.forEach((dec, idx) => {
        const isLast = idx === decs.length - 1;
        const kLbl = { rule_adopt: "自动提取", human_record: "手动补录", human_correct: "人工修正", undo: "撤销变更" }[dec.kind] || dec.kind;
        const val = dec.new_value || {};
        const vTxt = fact.fact_type === "stated_amount_paid"
          ? (val.amount_text ? formatMoney(val.currency || "USD", val.amount_text) : "已清空")
          : (val.period_start ? `${val.period_start} — ${val.period_end}` : "已清空");
        blocks.push(`
          <div class="timeline-row">
            <div>
              <div><strong>${esc(lbl)} · ${esc(kLbl)}</strong> → <span class="sensitive num">${esc(vTxt)}</span></div>
              <div style="font-size:11.5px;color:var(--ink-3)">版本 #${idx + 1} · ${formatTime(dec.recorded_at)}</div>
            </div>
            ${isLast && dec.kind !== "rule_adopt" ? `<button type="button" class="btn btn-soft btn-sm" data-undo-dec="${esc(dec.decision_id)}" data-undo-rev="${fact.revision}">撤销</button>` : ""}
          </div>
        `);
      });
    }
    el.innerHTML = blocks.length ? blocks.join("") : `<p class="folio-p">暂无金额或周期修订记录。</p>`;
    el.querySelectorAll("[data-undo-dec]").forEach((btn) => btn.addEventListener("click", async () => {
      btn.disabled = true;
      try {
        const snap = await api("POST", "/api/facts/undo", { decision_id: btn.getAttribute("data-undo-dec"), expected_revision: Number(btn.getAttribute("data-undo-rev")) });
        state.workspace = snap; renderWorkspace();
        const ref = (snap.items || []).find((i) => i.source_id === state.selectedSourceId);
        if (ref) renderDrawer(ref);
        showToast("已撤销上一步修改", "ok");
      } catch (err) {
        showToast(err.message, "error");
        btn.disabled = false;
      }
    }));
  }

  // --- Spotlight Command Palette ---
  function openCmdPalette() {
    if (!state.workspace) return;
    state.cmdReturnFocus = document.activeElement;
    state.cmdOpen = true; state.cmdIndex = 0;
    $("modal-cmd").classList.remove("hidden");
    $("cmd-input").value = "";
    renderCmdList("");
    $("cmd-input").focus();
  }
  function closeCmdPalette() {
    state.cmdOpen = false;
    $("modal-cmd").classList.add("hidden");
    const ret = state.cmdReturnFocus;
    state.cmdReturnFocus = null;
    if (ret && ret.isConnected) try { ret.focus(); } catch (_) {}
  }

  function renderCmdList(query) {
    const q = query.trim().toLowerCase();
    const acts = [];
    if ((state.workspace?.vault?.demo_count || 0) > 0) {
      acts.push({ ico: ICO.trash, label: "清除示例数据", sub: "仅删除示例，保留真实账单", run: () => $("btn-clear-demo-pack").click() });
    }
    acts.push(
      { ico: ICO.plus, label: "记一笔", sub: "订阅、买断、硬件或报销", run: () => openManualModal() },
      { ico: ICO.download, label: "导入本地文件", sub: "PDF、HTML、CSV 或截图", run: () => $("file-picker-input").click() },
      { ico: ICO.key, label: "查看买断与密钥", sub: "查看和复制软件密钥", run: () => navigateTo("assets", "license") },
      { ico: ICO.laptop, label: "查看硬件保修", sub: "保修剩余天数和日均成本", run: () => navigateTo("assets", "hardware") },
      { ico: ICO.receipt, label: "查看报销记录", sub: "查看并更新报销进度", run: () => navigateTo("assets", "reimbursable") },
      { ico: ICO.calendar, label: "打开日历", sub: "账单日、预计续费和保修截止", run: () => navigateTo("calendar") },
      { ico: ICO.chart, label: "打开统计", sub: "分类占比、商家汇总和试算", run: () => navigateTo("insights") },
      { ico: ICO.bolt, label: "打开待整理", sub: "对照原件补充信息", run: () => navigateTo("triage") },
      { ico: ICO.download, label: "导出 Excel 表格 (.csv)", sub: "包含分类、保修与报销状态", run: () => downloadAuthedFile("/api/export/csv", "subvault-ledger.csv") },
      { ico: ICO.sun, label: "切换外观", sub: "浅色 / 暖白 / 深色", run: () => cycleTheme() },
      { ico: ICO.eye, label: "隐藏或显示金额", sub: "快捷键 P", run: () => togglePrivacy() },
      { ico: ICO.lock, label: "锁定账本", sub: "返回解锁界面", run: () => $("btn-lock-vault").click() },
      { ico: ICO.party, label: "撒个花", sub: "没什么理由，就是想庆祝一下", egg: ["撒花", "庆祝", "party", "confetti", "🎉"], run: () => launchConfetti() },
      { ico: ICO.party, label: "下一场金币雨", sub: "看看钱都去哪儿了", egg: ["金币", "下雨", "coin", "money", "rain"], run: () => launchConfetti({ coins: true, count: 70 }) },
    );

    const assets = (state.workspace?.items || []).map((it) => {
      const ann = it.annotation || {};
      const amt = it.amount_fact?.value;
      return {
        ico: ICO.receipt,
        label: ann.vendor || it.display_name,
        sub: [ann.plan_name, amt ? formatMoney(amt.currency, amt.amount_text) : "缺少金额", ann.account_label].filter(Boolean).join(" · "),
        run: () => openDrawer(it),
      };
    });

    const filtered = [...acts, ...assets].filter((a) => {
      if (a.egg) return Boolean(q) && a.egg.some((k) => k.startsWith(q) || q.includes(k));
      return !q || a.label.toLowerCase().includes(q) || a.sub.toLowerCase().includes(q);
    });
    state.cmdActions = filtered.slice(0, 14);
    state.cmdIndex = Math.min(state.cmdIndex, Math.max(0, state.cmdActions.length - 1));

    const el = $("cmd-list");
    if (!state.cmdActions.length) { el.innerHTML = `<div style="padding:20px;text-align:center;color:var(--ink-3)">没有匹配的结果</div>`; return; }
    el.innerHTML = state.cmdActions.map((a, idx) => `
      <button type="button" class="cmd-item ${idx === state.cmdIndex ? "selected" : ""}" data-cmd-idx="${idx}">
        <span class="cmd-item-l"><span style="color:var(--ink-3);display:inline-flex">${a.ico}</span><span>${esc(a.label)}</span></span>
        <span class="cmd-item-sub">${esc(a.sub)}</span>
      </button>
    `).join("");
    el.querySelectorAll("[data-cmd-idx]").forEach((btn) => btn.addEventListener("click", () => {
      const act = state.cmdActions[Number(btn.getAttribute("data-cmd-idx"))];
      closeCmdPalette(); if (act) act.run();
    }));
    const selEl = el.querySelector(".cmd-item.selected");
    if (selEl) selEl.scrollIntoView({ block: "nearest" });
  }

  // --- Manual Entry Modal ---
  function setManualAssetKind(kind) {
    state.manualKind = kind || "subscription";
    document.querySelectorAll("[data-man-kind]").forEach((b) => b.classList.toggle("active", b.getAttribute("data-man-kind") === state.manualKind));
    $("man-panel-license").classList.toggle("hidden", state.manualKind !== "license");
    $("man-panel-hardware").classList.toggle("hidden", state.manualKind !== "hardware");
    $("man-panel-reimb").classList.toggle("hidden", state.manualKind !== "reimbursable");
    $("man-cycle-row").classList.toggle("hidden", state.manualKind === "hardware" || state.manualKind === "license");
    if (state.manualKind === "hardware" || state.manualKind === "license") state.manualCycle = "one_time";
    else if (state.manualKind === "subscription" && state.manualCycle === "one_time") state.manualCycle = "monthly";
    document.querySelectorAll("[data-man-cycle]").forEach((x) => x.classList.toggle("active", x.getAttribute("data-man-cycle") === state.manualCycle));
  }

  function openManualModal(prefillDate) {
    state.modalReturnFocus = document.activeElement;
    $("modal-manual").classList.remove("hidden");
    $("man-vendor").value = ""; $("man-plan").value = ""; $("man-amount").value = ""; $("man-notes").value = "";
    $("man-account").value = "个人"; $("man-category").value = "";
    document.querySelectorAll("[data-man-acc]").forEach((x) => x.classList.toggle("active", x.getAttribute("data-man-acc") === "个人"));
    $("man-license-key").value = ""; $("man-license-key").type = "password"; $("btn-man-toggle-key").textContent = "显示";
    $("man-license-scope").value = ""; $("man-warranty-end").value = ""; $("man-reimburse-status").value = "pending";
    const defCur = state.heroCurrency || "CNY";
    if ([...$("man-currency").options].some((o) => o.value === defCur)) $("man-currency").value = defCur;
    $("man-bill-date").value = (typeof prefillDate === "string" && /^\d{4}-\d{2}-\d{2}$/.test(prefillDate)) ? prefillDate : localTodayStr();
    setManualAssetKind("subscription");
    $("man-vendor").focus();
  }
  function closeManualModal() {
    $("modal-manual").classList.add("hidden");
    const ret = state.modalReturnFocus;
    state.modalReturnFocus = null;
    if (ret && ret.isConnected) try { ret.focus(); } catch (_) {}
  }

  // --- Upload & Drop ---
  const IGNORED_DROP_NAMES = new Set([".ds_store", "thumbs.db", "desktop.ini"]);

  async function importFileList(files, origin) {
    const all = Array.from(files || []).filter(Boolean);
    const nonIgnored = all.filter((f) => !IGNORED_DROP_NAMES.has((f.name || "").toLowerCase()));
    const valid = nonIgnored.filter((f) => f.size > 0);
    if (!valid.length) {
      if (all.length > 0) showToast("选中的文件为空（0 字节）或为系统隐藏文件，未导入", "error");
      return;
    }
    const from = origin || state.importOrigin || importOriginFor($("btn-import-files"));
    state.importOrigin = null;
    if (from) flyPapersInto(from.x, from.y, valid);
    const prevIds = itemIdSet();
    const prog = $("import-progress");
    prog.classList.remove("hidden");
    const batchId = `b_${Date.now().toString(36)}`;
    let added = 0, reused = 0, failed = 0, lastError = "";
    for (let i = 0; i < valid.length; i++) {
      const f = valid[i];
      const pct = Math.round(((i + 0.5) / valid.length) * 100);
      prog.innerHTML = `<span class="ip-bar" style="width:${pct}%"></span><span class="ip-text">收进金库 ${i + 1}/${valid.length} · ${esc(f.name)}</span>`;
      if (f.size > 8 * 1024 * 1024) {
        failed += 1;
        lastError = `${f.name}: 单个文件不能超过 8 MiB`;
        continue;
      }
      try {
        const res = await api("POST", "/api/import", { filename: f.name, content_base64: arrayBufferToBase64(await f.arrayBuffer()), batch_id: batchId });
        if (res.reused) reused += 1; else added += 1;
      } catch (err) { failed += 1; lastError = `${f.name}: ${err.message}`; }
    }
    prog.classList.add("hidden");
    await refreshWorkspace();
    highlightNewItems(prevIds);
    const parts = [];
    if (added) parts.push(`已收进 ${added} 份`);
    if (reused) parts.push(`认出 ${reused} 份重复，已跳过`);
    if (failed) parts.push(`失败 ${failed} 份（${lastError}）`);
    showToast(parts.join("，") || "导入完成", failed && !added ? "error" : "ok");
  }

  async function collectDroppedFiles(dt) {
    const out = [];
    const items = dt?.items ? Array.from(dt.items) : [];
    const hasEntryApi = items.some((it) => typeof it.webkitGetAsEntry === "function");
    if (!hasEntryApi) return Array.from(dt?.files || []);
    // Snapshot every FileSystemEntry synchronously BEFORE the first await (Chromium clears DataTransferItemList after async yield).
    const rootEntries = items
      .map((it) => {
        try { return typeof it.webkitGetAsEntry === "function" ? it.webkitGetAsEntry() : null; }
        catch (_) { return null; }
      })
      .filter(Boolean);
    if (!rootEntries.length) return Array.from(dt?.files || []);
    async function walk(entry) {
      if (!entry) return;
      if (entry.isFile) {
        const f = await new Promise((r) => entry.file(r, () => r(null)));
        if (f && !IGNORED_DROP_NAMES.has((f.name || "").toLowerCase())) out.push(f);
      } else if (entry.isDirectory) {
        const reader = entry.createReader();
        while (true) {
          const batch = await new Promise((r) => reader.readEntries(r, () => r([])));
          if (!batch.length) break;
          for (const child of batch) await walk(child);
        }
      }
    }
    for (const entry of rootEntries) await walk(entry);
    return out;
  }

  function openRecoveryModal(recoveryKey, onDone) {
    state.pendingRecoveryKey = recoveryKey;
    $("modal-recovery-key").textContent = recoveryKey;
    $("chk-recovery-saved").checked = false;
    $("btn-confirm-recovery").disabled = true;
    $("modal-recovery").classList.remove("hidden");
    scrambleText($("modal-recovery-key"), recoveryKey, 1300);
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
    $("btn-import-files").addEventListener("click", (e) => { state.importOrigin = importOriginFor(e.currentTarget); $("file-picker-input").click(); });
    $("btn-import-folder").addEventListener("click", (e) => { state.importOrigin = importOriginFor(e.currentTarget); $("folder-picker-input").click(); });
    bindTiltEffects();
    window.addEventListener("resize", () => moveDockGlider(true));
    $("create-passphrase").addEventListener("input", updateTumblers);
    $("create-passphrase-confirm").addEventListener("input", updateTumblers);

    // Vault pill popover
    const setPopoverOpen = (open) => {
      $("vault-popover").classList.toggle("hidden", !open);
      $("vault-pill-btn").setAttribute("aria-expanded", String(open));
    };
    $("vault-pill-btn").addEventListener("click", (e) => {
      e.stopPropagation();
      setPopoverOpen($("vault-popover").classList.contains("hidden"));
    });
    document.addEventListener("click", (e) => {
      if (!$("vault-popover").contains(e.target) && !$("vault-pill-btn").contains(e.target)) {
        setPopoverOpen(false);
      }
    });

    // Keyboard shortcuts & dialog focus trap
    document.addEventListener("keydown", (e) => {
      const inInput = ["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName);
      const anyModalOpen = !$("modal-manual").classList.contains("hidden")
        || !$("modal-recovery").classList.contains("hidden")
        || state.cmdOpen;
      if (e.key === "Tab") {
        if (!$("modal-recovery").classList.contains("hidden")) { trapFocusIn($("modal-recovery"), e); return; }
        if (state.cmdOpen) { trapFocusIn($("modal-cmd"), e); return; }
        if (!$("modal-manual").classList.contains("hidden")) { trapFocusIn($("modal-manual"), e); return; }
        if (state.selectedSourceId) { trapFocusIn($("detail-drawer"), e); return; }
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        if (!$("modal-recovery").classList.contains("hidden")) return;
        e.preventDefault();
        if (state.cmdOpen) closeCmdPalette(); else openCmdPalette();
        return;
      }
      if (state.cmdOpen) {
        if (e.key === "Escape") { closeCmdPalette(); return; }
        if (e.key === "ArrowDown") { e.preventDefault(); state.cmdIndex = (state.cmdIndex + 1) % Math.max(1, state.cmdActions.length); renderCmdList($("cmd-input").value); return; }
        if (e.key === "ArrowUp") { e.preventDefault(); state.cmdIndex = (state.cmdIndex - 1 + state.cmdActions.length) % Math.max(1, state.cmdActions.length); renderCmdList($("cmd-input").value); return; }
        if (e.key === "Enter" && state.cmdActions[state.cmdIndex]) { e.preventDefault(); const a = state.cmdActions[state.cmdIndex]; closeCmdPalette(); a.run(); return; }
        return;
      }
      if (e.key === "Escape") {
        setPopoverOpen(false);
        if (!$("modal-manual").classList.contains("hidden")) { closeManualModal(); return; }
        if (state.selectedSourceId) { closeDrawer(); return; }
      }
      if (!inInput && !anyModalOpen && !state.selectedSourceId && !$("screen-workspace").classList.contains("hidden")) {
        if (e.key === "/") { e.preventDefault(); navigateTo("assets"); $("search-box").focus(); }
        else if (e.key.toLowerCase() === "p") togglePrivacy();
      }
    });

    $("cmd-input").addEventListener("input", (e) => { state.cmdIndex = 0; renderCmdList(e.target.value); });
    $("modal-cmd").addEventListener("click", (e) => { if (e.target === $("modal-cmd")) closeCmdPalette(); });
    $("modal-manual").addEventListener("click", (e) => { if (e.target === $("modal-manual")) closeManualModal(); });

    // Folder pickers
    document.querySelectorAll("[data-pick-target]").forEach((btn) => btn.addEventListener("click", async () => {
      const targetId = btn.getAttribute("data-pick-target");
      const title = btn.getAttribute("data-pick-title") || "选择文件夹";
      const suffix = btn.getAttribute("data-pick-suffix") || "";
      btn.disabled = true;
      try {
        const res = await api("POST", "/api/pick-folder", { title });
        if (!res.cancelled && res.path) {
          const sep = res.path.includes("\\") ? "\\" : "/";
          $(targetId).value = suffix ? `${res.path.replace(/[\\/]+$/, "")}${sep}${suffix}` : res.path;
        }
      } catch (err) { showToast(err.message, "error"); }
      finally { btn.disabled = false; }
    }));

    // Lobby controls
    document.querySelectorAll("[data-lobby-tab]").forEach((b) => b.addEventListener("click", () => setLobbyTab(b.getAttribute("data-lobby-tab"))));
    document.querySelectorAll("[data-unlock-mode]").forEach((b) => b.addEventListener("click", () => setUnlockMode(b.getAttribute("data-unlock-mode"))));
    document.querySelectorAll('input[name="create-protection"]').forEach((r) => r.addEventListener("change", () => {
      const isPass = r.value === "passphrase";
      $("opt-passphrase").classList.toggle("active", isPass);
      $("opt-os-disk").classList.toggle("active", !isPass);
      $("create-passphrase-fields").classList.toggle("hidden", !isPass);
    }));

    $("form-unlock-vault").addEventListener("submit", async (e) => {
      e.preventDefault();
      const errBox = $("unlock-error"); errBox.classList.add("hidden");
      const btn = $("btn-unlock-submit"); btn.disabled = true; btn.innerHTML = `<span class="dial-spin" aria-hidden="true"></span>正在转动转盘…`;
      try {
        const payload = { path: $("unlock-path").value.trim() };
        if (state.unlockMode === "passphrase") payload.passphrase = $("unlock-passphrase").value;
        else if (state.unlockMode === "recovery") payload.recovery_key = $("unlock-recovery").value.trim();
        const snap = await api("POST", "/api/vault/unlock", payload);
        $("unlock-passphrase").value = ""; $("unlock-recovery").value = "";
        resetVaultUiState();
        await playVaultDoor(() => { state.workspace = snap; showWorkspaceScreen(); navigateTo("assets", "all"); });
        showToast("金库门已打开，欢迎回来", "ok");
      } catch (err) {
        errBox.textContent = err.message; errBox.classList.remove("hidden");
        if (motionOK()) pulseClass($("form-unlock-vault"), "shake", 520);
      }
      finally { btn.disabled = false; btn.textContent = "打开账本"; }
    });

    $("form-create-vault").addEventListener("submit", async (e) => {
      e.preventDefault();
      const errBox = $("create-error"); errBox.classList.add("hidden");
      const prot = document.querySelector('input[name="create-protection"]:checked').value;
      const pass = $("create-passphrase").value, cpass = $("create-passphrase-confirm").value;
      if (prot === "passphrase") {
        const fail = (msg) => { errBox.textContent = msg; errBox.classList.remove("hidden"); if (motionOK()) pulseClass($("form-create-vault"), "shake", 520); };
        if (pass.length < 6) { fail("密码至少需要 6 位字符"); return; }
        if (pass !== cpass) { fail("两次输入的密码不一致"); return; }
      }
      const btn = $("btn-create-submit"); btn.disabled = true; btn.innerHTML = `<span class="dial-spin" aria-hidden="true"></span>正在浇筑金库…`;
      try {
        const res = await api("POST", "/api/vault/init", { path: $("create-path").value.trim(), protection: prot, passphrase: prot === "passphrase" ? pass : "" });
        $("create-passphrase").value = ""; $("create-passphrase-confirm").value = ""; updateTumblers();
        resetVaultUiState();
        await refreshWorkspace();
        const enter = async () => {
          await playVaultDoor(() => { showWorkspaceScreen(); navigateTo("assets", "all"); });
          launchConfetti({ count: 90 });
          showToast("新金库落成，开始往里放东西吧", "ok");
        };
        if (res.recovery_key) openRecoveryModal(res.recovery_key, enter);
        else await enter();
      } catch (err) {
        errBox.textContent = err.message; errBox.classList.remove("hidden");
        if (motionOK()) pulseClass($("form-create-vault"), "shake", 520);
      }
      finally { btn.disabled = false; btn.textContent = "创建账本"; }
    });

    // Recovery modal
    $("chk-recovery-saved").addEventListener("change", (e) => { $("btn-confirm-recovery").disabled = !e.target.checked; });
    $("btn-copy-recovery").addEventListener("click", async () => {
      if (!state.pendingRecoveryKey) return;
      try { await navigator.clipboard.writeText(state.pendingRecoveryKey); showToast("恢复密钥已复制", "ok"); }
      catch (_) { showToast("请点下载 .txt 备份", "error"); }
    });
    $("btn-download-recovery").addEventListener("click", () => {
      if (state.pendingRecoveryKey) downloadTextFile("SubVault-离线恢复密钥.txt", `# SubVault 离线恢复密钥\n${state.pendingRecoveryKey}\n`);
    });

    // Manual entry modal
    document.querySelectorAll("[data-man-kind]").forEach((b) => b.addEventListener("click", () => setManualAssetKind(b.getAttribute("data-man-kind"))));
    $("btn-man-toggle-key").addEventListener("click", () => {
      const inp = $("man-license-key"), show = inp.type === "password";
      inp.type = show ? "text" : "password"; $("btn-man-toggle-key").textContent = show ? "隐藏" : "显示";
    });
    document.querySelectorAll("[data-man-cycle]").forEach((b) => b.addEventListener("click", () => {
      state.manualCycle = b.getAttribute("data-man-cycle");
      document.querySelectorAll("[data-man-cycle]").forEach((x) => x.classList.toggle("active", x === b));
    }));
    document.querySelectorAll("[data-man-acc]").forEach((b) => b.addEventListener("click", () => {
      $("man-account").value = b.getAttribute("data-man-acc");
      document.querySelectorAll("[data-man-acc]").forEach((x) => x.classList.toggle("active", x === b));
    }));
    $("form-manual-entry").addEventListener("submit", async (e) => {
      e.preventDefault();
      const btn = $("btn-submit-manual"); btn.disabled = true;
      try {
        const payload = {
          vendor: $("man-vendor").value, plan_name: $("man-plan").value, currency: $("man-currency").value,
          amount_text: $("man-amount").value, billing_cycle: state.manualCycle, account_label: $("man-account").value,
          category: $("man-category").value, bill_date: $("man-bill-date").value, notes: $("man-notes").value, asset_kind: state.manualKind,
        };
        if (state.manualKind === "license") { payload.license_key = $("man-license-key").value; payload.license_scope = $("man-license-scope").value; }
        else if (state.manualKind === "hardware") payload.warranty_end = $("man-warranty-end").value;
        else if (state.manualKind === "reimbursable") payload.reimburse_status = $("man-reimburse-status").value;
        const prevIds = itemIdSet();
        const snap = await api("POST", "/api/manual-entry", payload);
        state.workspace = snap; closeManualModal(); renderWorkspace(); highlightNewItems(prevIds); showToast("记好了", "ok");
      } catch (err) { showToast(err.message, "error"); }
      finally { btn.disabled = false; }
    });

    // Clear demo & delete single item
    $("btn-clear-demo-pack").addEventListener("click", async () => {
      const btn = $("btn-clear-demo-pack"); btn.disabled = true; btn.textContent = "清除中…";
      try {
        const snap = await api("POST", "/api/clear-demo-pack", {});
        state.workspace = snap; closeDrawer(); renderWorkspace();
        showToast(`已清除 ${snap.removed_count || 0} 条示例数据`, "ok");
      } catch (err) { showToast(err.message, "error"); }
      finally { btn.disabled = false; btn.textContent = "清除示例"; }
    });
    $("btn-ask-delete-item").addEventListener("click", () => { $("btn-ask-delete-item").classList.add("hidden"); $("delete-confirm-bar").classList.remove("hidden"); });
    $("btn-cancel-delete-item").addEventListener("click", () => { $("delete-confirm-bar").classList.add("hidden"); $("btn-ask-delete-item").classList.remove("hidden"); });
    $("btn-confirm-delete-item").addEventListener("click", async () => {
      if (!state.selectedSourceId) return;
      const btn = $("btn-confirm-delete-item"); btn.disabled = true; btn.textContent = "删除中…";
      try {
        const snap = await api("POST", "/api/sources/delete", { source_id: state.selectedSourceId });
        state.workspace = snap; closeDrawer(); renderWorkspace(); showToast("已删除", "ok");
      } catch (err) { showToast(err.message, "error"); }
      finally { btn.disabled = false; btn.textContent = "确认删除"; }
    });

    // Floating dock & collection pills
    document.querySelectorAll("[data-nav]").forEach((btn) => btn.addEventListener("click", () => navigateTo(btn.getAttribute("data-nav"))));
    document.querySelectorAll("[data-collection]").forEach((btn) => btn.addEventListener("click", () => {
      state.collection = btn.getAttribute("data-collection");
      if (state.collection !== "reimbursable") state.reimburseFilter = "";
      if (state.workspace) renderAssetsPane(state.workspace);
    }));

    // Upload, lock, shutdown
    $("file-picker-input").addEventListener("change", (e) => { importFileList(e.target.files); e.target.value = ""; });
    $("folder-picker-input").addEventListener("change", (e) => { importFileList(e.target.files); e.target.value = ""; });
    $("btn-lock-vault").addEventListener("click", async () => {
      $("vault-popover").classList.add("hidden");
      try {
        await api("POST", "/api/vault/lock", {});
        resetVaultUiState();
        const info = await api("GET", "/api/bootstrap");
        renderRecentVaults(info.recent_vaults || []);
        await playVaultDoor(() => {
          showLobbyScreen();
          setLobbyTab("unlock");
          if ((info.recent_vaults || []).length) selectRecentVault(info.recent_vaults[0]);
        }, true);
        showToast("金库已锁好", "ok");
      } catch (err) {
        showToast(err.message || "锁定账本失败", "error");
      }
    });
    $("btn-shutdown-app").addEventListener("click", async () => {
      try { await api("POST", "/api/shutdown", {}); } catch (_) {}
      resetVaultUiState();
      try { sessionStorage.removeItem(TOKEN_STORE_KEY); } catch (_) {}
      document.body.innerHTML = `<div class="farewell"><div class="farewell-lock">${LOCK_ART}</div><h2>SubVault 已安全退出</h2><p>金库门已锁好，本地服务已停止，可以直接关闭这个页面。</p></div>`;
    });

    // Global drag & drop
    const overlay = $("global-drop-overlay");
    const isFileDrag = (e) => !state.internalDrag && Array.from(e.dataTransfer?.types || []).includes("Files");
    window.addEventListener("dragenter", (e) => { if ($("screen-workspace").classList.contains("hidden") || !isFileDrag(e)) return; e.preventDefault(); state.dragDepth += 1; overlay.classList.remove("hidden"); });
    window.addEventListener("dragover", (e) => { if ($("screen-workspace").classList.contains("hidden") || !isFileDrag(e)) return; e.preventDefault(); });
    window.addEventListener("dragleave", (e) => { if (!isFileDrag(e)) return; state.dragDepth = Math.max(0, state.dragDepth - 1); if (state.dragDepth === 0) overlay.classList.add("hidden"); });
    window.addEventListener("drop", async (e) => {
      if (!isFileDrag(e)) return;
      state.dragDepth = 0; overlay.classList.add("hidden");
      if ($("screen-workspace").classList.contains("hidden")) return;
      e.preventDefault();
      const origin = { x: e.clientX, y: e.clientY };
      importFileList(await collectDroppedFiles(e.dataTransfer), origin);
    });

    // Tools
    document.querySelectorAll("[data-view-mode]").forEach((btn) => btn.addEventListener("click", () => { state.viewMode = btn.getAttribute("data-view-mode"); if (state.workspace) renderAssetsPane(state.workspace); }));
    $("filter-account").addEventListener("change", (e) => { state.accountFilter = e.target.value; if (state.workspace) renderAssetsPane(state.workspace); });
    $("sort-order").addEventListener("change", (e) => { state.sortOrder = e.target.value; if (state.workspace) renderAssetsPane(state.workspace); });
    $("search-box").addEventListener("input", (e) => { state.searchQuery = e.target.value; if (state.workspace) renderAssetsPane(state.workspace); });

    // Drawer
    $("btn-close-drawer").addEventListener("click", closeDrawer);
    $("drawer-backdrop").addEventListener("click", closeDrawer);
    document.querySelectorAll(".dtab").forEach((b) => b.addEventListener("click", () => switchDrawerTab(b.getAttribute("data-dtab"))));
    $("btn-toggle-text-view").addEventListener("click", () => { state.textViewActive = !state.textViewActive; $("evidence-text-box").classList.toggle("hidden", !state.textViewActive); });
    $("csv-search").addEventListener("input", () => { const it = (state.workspace?.items || []).find((i) => i.source_id === state.selectedSourceId); if (it) renderCsvRows(it); });
    $("chk-csv-expense-only").addEventListener("change", () => { const it = (state.workspace?.items || []).find((i) => i.source_id === state.selectedSourceId); if (it) renderCsvRows(it); });
    document.querySelectorAll("[data-ed-kind]").forEach((b) => b.addEventListener("click", () => setEditAssetKind(b.getAttribute("data-ed-kind"), true)));
    ["ed-bill-date", "ed-warranty-end", "ed-amount", "ed-currency"].forEach((id) => { $(id).addEventListener("input", updateEditHardwareMetrics); $(id).addEventListener("change", updateEditHardwareMetrics); });
    $("btn-ed-toggle-key").addEventListener("click", () => { const inp = $("ed-license-key"), show = inp.type === "password"; inp.type = show ? "text" : "password"; $("btn-ed-toggle-key").textContent = show ? "隐藏" : "显示"; });
    $("btn-ed-copy-key").addEventListener("click", () => { const v = $("ed-license-key").value.trim(); if (!v) { showToast("尚未填写密钥", "error"); return; } copyLicenseKey($("ed-vendor").value, v); });

    $("form-edit-item").addEventListener("submit", async (e) => {
      e.preventDefault();
      if (!state.selectedSourceId) return;
      const btn = $("btn-save-item"); btn.disabled = true; btn.textContent = "保存中…";
      let saved = false;
      try {
        const payload = {
          source_id: state.selectedSourceId, vendor: $("ed-vendor").value, plan_name: $("ed-plan").value, account_label: $("ed-account").value,
          billing_cycle: $("ed-cycle").value, currency: $("ed-currency").value, amount_text: $("ed-amount").value, category: $("ed-category").value,
          bill_date: $("ed-bill-date").value, period_start: $("ed-period-start").value, period_end: $("ed-period-end").value, notes: $("ed-notes").value,
          asset_kind: state.editKind, warranty_end: $("ed-warranty-end").value, license_scope: $("ed-license-scope").value, reimburse_status: $("ed-reimburse-status").value,
        };
        if (state.editKind === "license") payload.license_key = $("ed-license-key").value;
        const snap = await api("POST", "/api/sources/save", payload);
        state.workspace = snap; renderWorkspace();
        const ref = (snap.items || []).find((i) => i.source_id === state.selectedSourceId);
        if (ref) renderDrawer(ref);
        saved = true;
        flashButtonSuccess(btn, "已保存", "保存修改");
        showToast("已保存修改", "ok");
      } catch (err) { showToast(err.message, "error"); }
      finally { if (!saved) { btn.disabled = false; btn.textContent = "保存修改"; } }
    });

    // Redact & Exports
    $("btn-run-redact").addEventListener("click", async () => {
      if (!state.selectedSourceId) return;
      const terms = ($("redact-custom-terms").value || "").split(/[,，\n]/).map((s) => s.trim()).filter(Boolean);
      const btn = $("btn-run-redact"); btn.disabled = true;
      try {
        const res = await api("POST", "/api/sources/redact", { source_id: state.selectedSourceId, redact_terms: terms });
        state.lastRedactedText = res.redacted_text || "";
        $("redact-stats").textContent = `已完成脱敏：共替换 ${res.total_redactions || 0} 处敏感信息（不影响原件）`;
        $("redact-output").textContent = state.lastRedactedText;
        $("redact-result-wrap").classList.remove("hidden");
      } catch (err) { showToast(err.message, "error"); }
      finally { btn.disabled = false; }
    });
    $("btn-copy-redact").addEventListener("click", async () => {
      try { await navigator.clipboard.writeText(state.lastRedactedText); showToast("脱敏内容已复制", "ok"); } catch (_) { showToast("请点下载 .txt", "error"); }
    });
    $("btn-dl-redact").addEventListener("click", () => downloadTextFile("subvault-redacted-template.txt", state.lastRedactedText));

    $("btn-dl-excel").addEventListener("click", () => downloadAuthedFile("/api/export/csv", "subvault-ledger.csv"));
    $("btn-dl-zip").addEventListener("click", () => {
      const enc = state.workspace?.vault?.encrypted;
      if (enc && !$("chk-zip-ack").checked) { showToast("请先勾选明文导出确认框", "error"); return; }
      downloadAuthedFile(`/api/export/zip${enc ? "?acknowledge_plaintext=yes" : ""}`, "subvault-exit-pack.zip");
    });
    $("btn-export-dir").addEventListener("click", async () => {
      const outDir = $("export-dir-path").value.trim(), enc = state.workspace?.vault?.encrypted, ack = $("chk-dir-ack").checked;
      const resBox = $("export-dir-result"); resBox.classList.add("hidden");
      if (!outDir) { showToast("请填写导出文件夹路径", "error"); return; }
      if (enc && !ack) { showToast("请先勾选明文导出确认框", "error"); return; }
      try {
        const res = await api("POST", "/api/export/package", { out_dir: outDir, acknowledge_plaintext: Boolean(ack) });
        const count = res.verification?.verified_files ?? "?";
        resBox.className = res.verified ? "inline-banner" : "inline-banner error";
        resBox.textContent = res.verified ? `导出并校验通过：共核验 ${count} 个文件` : `已导出到 ${outDir}，但校验发现问题：${res.verification?.reason || "请检查目录"}`;
        resBox.classList.remove("hidden"); $("verify-dir-path").value = outDir;
      } catch (err) { resBox.className = "inline-banner error"; resBox.textContent = err.message; resBox.classList.remove("hidden"); }
    });
    $("btn-verify-dir").addEventListener("click", async () => {
      const dir = $("verify-dir-path").value.trim(), resBox = $("verify-dir-result"); resBox.classList.add("hidden");
      if (!dir) { showToast("请填写备份包目录路径", "error"); return; }
      try {
        const res = await api("POST", "/api/verify-package", { package_dir: dir });
        const count = res.report?.verified_files ?? "?";
        const defectCount = Array.isArray(res.report?.defects) ? res.report.defects.length : 0;
        resBox.className = res.verified ? "inline-banner" : "inline-banner error";
        resBox.textContent = res.verified
          ? `校验通过：共 ${count} 个文件完整无篡改`
          : `校验未通过：${res.report?.reason || "发现文件缺失或被修改"}${defectCount > 0 ? `（共 ${defectCount} 处异常）` : ""}`;
        resBox.classList.remove("hidden");
      } catch (err) { resBox.className = "inline-banner error"; resBox.textContent = err.message; resBox.classList.remove("hidden"); }
    });
    $("form-rotate-pass").addEventListener("submit", async (e) => {
      e.preventDefault();
      const curCred = $("rotate-current-pass").value;
      const p1 = $("rotate-new-pass").value, p2 = $("rotate-new-pass-confirm").value, rot = $("chk-rotate-recovery").checked;
      if (!curCred.trim()) { showToast("请先输入当前主密码或离线恢复密钥", "error"); return; }
      if (p1.length < 6) { showToast("新密码至少需要 6 位字符", "error"); return; }
      if (p1 !== p2) { showToast("两次输入的新密码不一致", "error"); return; }
      const payload = { new_passphrase: p1, rotate_recovery_key: rot };
      if (/^svrk-/i.test(curCred.trim())) payload.recovery_key = curCred.trim();
      else payload.current_passphrase = curCred;
      try {
        const res = await api("POST", "/api/vault/rotate-passphrase", payload);
        $("rotate-current-pass").value = "";
        $("rotate-new-pass").value = ""; $("rotate-new-pass-confirm").value = ""; $("chk-rotate-recovery").checked = false;
        if (res.recovery_key) openRecoveryModal(res.recovery_key, () => showToast("主密码与恢复密钥已更新", "ok"));
        else showToast("主密码已更新", "ok");
      } catch (err) { showToast(err.message, "error"); }
    });
  }

  document.addEventListener("DOMContentLoaded", boot);
})();
