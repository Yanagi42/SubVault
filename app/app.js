(() => {
  "use strict";

  let SESSION_TOKEN = "";
  const hashMatch = window.location.hash.match(/token=([0-9a-fA-F]+)/);
  if (hashMatch) {
    SESSION_TOKEN = hashMatch[1];
    try { window.history.replaceState(null, "", window.location.pathname); } catch (_) {}
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
    sun: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2m0 16v2M4.93 4.93l1.41 1.41m11.32 11.32 1.41 1.41M2 12h2m16 0h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/></svg>`,
    laptop: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><rect width="16" height="12" x="4" y="4" rx="2"/><path d="M2 20h20"/></svg>`,
    refresh: `<svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16"/><path d="M16 16h5v5"/></svg>`,
    arrowRight: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14"/><path d="m12 5 7 7-7 7"/></svg>`,
    trash: `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M19 6l-1 14H6L5 6"/></svg>`,
  };

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
    try { return new Date(ms).toLocaleString("zh-CN", { hour12: false }); } catch (_) { return String(ms); }
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
    try { localStorage.setItem("subvault_ui_theme_v3", found.id); } catch (_) {}
  }

  function cycleTheme() {
    const idx = THEMES.findIndex((t) => t.id === state.theme);
    const next = THEMES[(idx + 1) % THEMES.length];
    applyTheme(next.id);
    if (state.workspace) renderWorkspace();
    showToast(`已切换为${next.label}外观`);
  }

  function togglePrivacy(forceVal) {
    state.privacyBlur = forceVal !== undefined ? forceVal : !state.privacyBlur;
    document.body.classList.toggle("privacy-blur", state.privacyBlur);
    $("btn-privacy-toggle").classList.toggle("active", state.privacyBlur);
    if (state.privacyBlur) showToast("已隐藏金额、账号和密钥，鼠标放上去可临时查看");
    else if (forceVal === undefined) showToast("已恢复显示金额");
  }

  function launchConfetti() {
    const canvas = $("confetti-canvas");
    if (!canvas) return;
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    canvas.classList.remove("hidden");
    const ctx = canvas.getContext("2d");
    const pieces = Array.from({ length: 68 }, () => ({
      x: Math.random() * canvas.width,
      y: -20 - Math.random() * 120,
      r: 4 + Math.random() * 4,
      dx: (Math.random() - 0.5) * 4,
      dy: 2.6 + Math.random() * 4,
      rot: Math.random() * 360,
      drot: (Math.random() - 0.5) * 10,
      color: getChartPalette()[Math.floor(Math.random() * 8)],
    }));
    let frame = 0;
    function tick() {
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      pieces.forEach((p) => {
        p.x += p.dx; p.y += p.dy; p.dy += 0.08; p.rot += p.drot;
        ctx.save(); ctx.translate(p.x, p.y); ctx.rotate((p.rot * Math.PI) / 180);
        ctx.fillStyle = p.color; ctx.fillRect(-p.r, -p.r * 0.5, p.r * 2, p.r); ctx.restore();
      });
      frame += 1;
      if (frame < 100) requestAnimationFrame(tick);
      else { ctx.clearRect(0, 0, canvas.width, canvas.height); canvas.classList.add("hidden"); }
    }
    requestAnimationFrame(tick);
  }

  // --- Metric Helpers ---
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
    const now = new Date();
    const pct = Math.max(0, Math.min(100, Math.round(((now - s) / (e - s)) * 100)));
    const daysLeft = Math.ceil((e - now) / (1000 * 60 * 60 * 24));
    const statusLabel = daysLeft > 0 ? `还剩 ${daysLeft} 天` : `${perVal.period_end} 已到期`;
    return { pct, expired: daysLeft <= 0, range: `${perVal.period_start} — ${perVal.period_end}`, statusLabel };
  }

  function warrantyInfo(buyDate, warrantyEnd) {
    if (!warrantyEnd || !/^\d{4}-\d{2}-\d{2}$/.test(warrantyEnd)) return null;
    const end = new Date(`${warrantyEnd}T00:00:00`);
    if (isNaN(end.getTime())) return null;
    const now = new Date();
    const daysLeft = Math.ceil((end - now) / (1000 * 60 * 60 * 24));
    let remPct = daysLeft <= 0 ? 0 : 65;
    if (buyDate && /^\d{4}-\d{2}-\d{2}$/.test(buyDate)) {
      const start = new Date(`${buyDate}T00:00:00`);
      if (!isNaN(start.getTime()) && end > start) {
        remPct = Math.max(0, Math.min(100, Math.round(((end - now) / (end - start)) * 100)));
      }
    }
    const color = daysLeft < 0 ? "var(--ink-3)" : daysLeft <= 60 ? "var(--amber)" : "var(--iris)";
    const statusLabel = daysLeft > 0 ? `保修剩 ${daysLeft} 天` : daysLeft === 0 ? "今日到期" : `已过保 ${Math.abs(daysLeft)} 天`;
    const shortTag = daysLeft > 0 ? `${daysLeft}d` : "完";
    return { daysLeft, remPct, color, statusLabel, shortTag, warrantyEnd };
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

  function renderRingGauge(pct, color, label) {
    const r = 17;
    const circ = 2 * Math.PI * r;
    const dash = Math.max(2, (Math.max(0, Math.min(100, pct)) / 100) * circ);
    return `
      <div class="ring-gauge">
        <svg width="44" height="44" viewBox="0 0 44 44">
          <circle cx="22" cy="22" r="${r}" fill="none" stroke="var(--recessed)" stroke-width="4" />
          <circle cx="22" cy="22" r="${r}" fill="none" stroke="${color}" stroke-width="4" stroke-linecap="round" stroke-dasharray="${dash.toFixed(1)} ${circ.toFixed(1)}" />
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
      html += `<span class="tick-bar ${i < filled ? cls : ""}"></span>`;
    }
    html += `</div>`;
    return html;
  }

  async function copyLicenseKey(vendor, key, btnEl) {
    try {
      await navigator.clipboard.writeText(key);
      if (btnEl) {
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
    const next = REIMBURSE_META[cur].next;
    try {
      const snap = await api("POST", "/api/sources/reimburse", { source_id: sourceId, reimburse_status: next });
      state.workspace = snap;
      renderWorkspace();
      showToast(`报销状态已改为「${REIMBURSE_META[next].label}」`, "ok");
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
      showToast("请通过双击 subvault-app 打开程序。", "error");
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

  function navigateTo(navKey, collectionKey) {
    state.activeNav = navKey;
    if (navKey === "assets" && collectionKey !== undefined) state.collection = collectionKey;

    document.querySelectorAll("[data-nav]").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-nav") === navKey);
    });
    ["assets", "calendar", "insights", "triage", "security"].forEach((name) => {
      $(`pane-${name}`).classList.toggle("hidden", name !== navKey);
    });

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
    $("cnt-sub").textContent = ac.subscription > 0 ? String(ac.subscription) : "";
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
  }

  function syncAccountDropdown(ws) {
    const sel = $("filter-account");
    sel.innerHTML = `<option value="">所有账号</option>` +
      (ws.accounts || []).map((a) => `<option value="${esc(a)}">${esc(a)}</option>`).join("");
    sel.value = state.accountFilter;
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
            <div class="empty-emblem">${ICO.inbox}</div>
            <h3>账本里还没有记录</h3>
            <p>把 PDF 发票、邮件 HTML、支付宝 / 微信 CSV 或收据截图拖进窗口，也可以直接手动记一笔。</p>
            <div class="btn-row" style="justify-content:center">
              <button type="button" id="btn-empty-import" class="btn btn-ink btn-sm">选择文件导入</button>
              <button type="button" id="btn-empty-manual" class="btn btn-soft btn-sm">记一笔</button>
              <button type="button" id="btn-load-demo-pack" class="btn btn-ghost btn-sm">加载示例数据（可随时清除）</button>
            </div>
          </div>
        `;
        $("btn-empty-import").addEventListener("click", () => $("file-picker-input").click());
        $("btn-empty-manual").addEventListener("click", openManualModal);
        const demoBtn = $("btn-load-demo-pack");
        demoBtn.addEventListener("click", async () => {
          demoBtn.disabled = true;
          demoBtn.textContent = "加载中…";
          try {
            const snap = await api("POST", "/api/import-demo-pack", {});
            state.workspace = snap;
            renderWorkspace();
            showToast("已加载示例数据，顶部可随时清除", "ok");
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
          state.collection = "all";
          renderWorkspace();
        });
      }
      return;
    }

    if (state.collection === "csv") {
      renderCsvWorkspace(list, stage);
    } else if (state.viewMode === "cards") {
      renderObjectGallery(list, stage);
    } else {
      renderTableView(list, stage);
    }
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
      const rsum = ws.reimburse_summary || [];
      const sumBucket = (fieldAmt, fieldCnt) => {
        let cnt = 0;
        const parts = [];
        for (const r of rsum) {
          if (r[fieldCnt] > 0) {
            cnt += r[fieldCnt];
            parts.push(formatMoneyShort(r.currency, r[fieldAmt]));
          }
        }
        return { cnt, text: parts.join(" · ") || "0" };
      };
      const bPend = sumBucket("pending_text", "pending_count");
      const bSub = sumBucket("submitted_text", "submitted_count");
      const bDone = sumBucket("reimbursed_text", "reimbursed_count");
      hero.innerHTML = `
        <div class="context-strip">
          <div class="ctx-title-group">
            <div class="ctx-title">报销记录</div>
            <div class="ctx-sub">点击卡片右下角状态按钮，可依次切换 待报销 → 已提交 → 已到账</div>
          </div>
          <div class="ctx-buckets">
            <div class="ctx-bucket"><span class="micro-pill amber">待报销</span><strong class="sensitive">${esc(bPend.text)}</strong><span style="color:var(--ink-3)">${bPend.cnt} 笔</span></div>
            <div class="ctx-bucket"><span class="micro-pill blue">已提交</span><strong class="sensitive">${esc(bSub.text)}</strong><span style="color:var(--ink-3)">${bSub.cnt} 笔</span></div>
            <div class="ctx-bucket"><span class="micro-pill green">已到账</span><strong class="sensitive">${esc(bDone.text)}</strong><span style="color:var(--ink-3)">${bDone.cnt} 笔</span></div>
          </div>
        </div>
      `;
      return;
    }

    if (col === "subscription") {
      const subs = items.filter((i) => i.annotation?.asset_kind === "subscription");
      const curList = (ws.summary_by_currency || []).filter((c) => c.recurring_count > 0);
      const monthStr = curList.map((c) => formatMoneyShort(c.currency, c.monthly_equiv_recurring)).join(" · ") || "—";
      const yearStr = curList.map((c) => formatMoneyShort(c.currency, c.yearly_equiv_recurring)).join(" · ") || "—";
      hero.innerHTML = `
        <div class="context-strip">
          <div class="ctx-title-group">
            <div class="ctx-title">周期订阅</div>
            <div class="ctx-sub">自动追踪服务周期与涨价变动，往期历史账单已折叠在下方</div>
          </div>
          <div class="ctx-metrics">
            <div class="ctx-metric"><span class="ctx-k">订阅记录</span><span class="ctx-v">${subs.length} 笔</span></div>
            <div class="ctx-metric"><span class="ctx-k">月均折算</span><span class="ctx-v sensitive">${esc(monthStr)}</span></div>
            <div class="ctx-metric"><span class="ctx-k">年度折算</span><span class="ctx-v sensitive">${esc(yearStr)}</span></div>
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

    const curList = ws.summary_by_currency || [];
    const curCodes = curList.map((c) => c.currency);
    if (!curCodes.includes(state.heroCurrency)) state.heroCurrency = curCodes[0] || "CNY";
    const activeCur = curList.find((c) => c.currency === state.heroCurrency) || {
      currency: "CNY", total_recorded: "0.00", monthly_equiv_recurring: "0.00", yearly_equiv_recurring: "0.00", recurring_count: 0, bill_count: 0,
    };

    // Composition by asset_kind in activeCur
    const kindTotals = { hardware: 0, subscription: 0, license: 0, reimbursable: 0, general: 0 };
    let grand = 0;
    for (const it of items) {
      const amt = it.amount_fact?.value;
      if (!amt || amt.currency !== activeCur.currency) continue;
      const cents = amountToCents(amt.amount_text);
      const k = it.annotation?.asset_kind || "general";
      kindTotals[k] = (kindTotals[k] || 0) + cents;
      grand += cents;
    }

    const kindMetaOrder = [
      { key: "hardware", label: "硬件", color: KIND_COLORS.hardware },
      { key: "subscription", label: "订阅", color: KIND_COLORS.subscription },
      { key: "license", label: "买断", color: KIND_COLORS.license },
      { key: "reimbursable", label: "报销", color: KIND_COLORS.reimbursable },
      { key: "general", label: "其他", color: KIND_COLORS.general },
    ].filter((m) => kindTotals[m.key] > 0);

    const ribbonHtml = grand > 0
      ? `
        <div class="composition-wrap">
          <div class="comp-ribbon">
            ${kindMetaOrder.map((m) => {
              const pct = Math.max(4, Math.round((kindTotals[m.key] / grand) * 100));
              return `<div class="comp-seg" data-ribbon-kind="${m.key}" style="width:${pct}%;background:${m.color}" title="${m.label}：${formatMoneyShort(activeCur.currency, centsToText(kindTotals[m.key]))} (${pct}%)"></div>`;
            }).join("")}
          </div>
          <div class="comp-legend">
            <div class="comp-legend-items">
              ${kindMetaOrder.map((m) => {
                const pct = Math.round((kindTotals[m.key] / grand) * 100);
                return `<button type="button" class="comp-leg-btn" data-ribbon-kind="${m.key}"><span class="c-dot" style="background:${m.color}"></span>${m.label} <strong class="sensitive num">${pct}%</strong></button>`;
              }).join("")}
            </div>
            <span style="color:var(--ink-3)">按币种分开统计，不做汇率换算</span>
          </div>
        </div>
      `
      : "";

    const curTabsHtml = curList.length > 0
      ? `<div class="cur-switcher">${curList.map((c) => `<button type="button" class="cur-tab ${c.currency === activeCur.currency ? "active" : ""}" data-hero-cur="${esc(c.currency)}">${esc(c.currency)} · ${esc(CURRENCY_NAMES[c.currency] || c.currency)}</button>`).join("")}</div>`
      : `<span class="micro-eyebrow">账本概览</span>`;

    // Living Signals Column (up to 3 crisp tactile cards)
    const radar = ws.asset_radar || [];
    const signalCards = [];
    for (let idx = 0; idx < Math.min(3, radar.length); idx++) {
      const r = radar[idx];
      if (r.kind === "warranty_expiring") {
        const ring = renderRingGauge(Math.max(8, Math.round((r.days_left / 365) * 100)), r.days_left <= 14 ? "var(--rose)" : "var(--iris)", `${r.days_left}d`);
        signalCards.push(`
          <button type="button" class="signal-tile" data-sig-idx="${idx}">
            <div class="sig-left">
              ${ring}
              <div style="min-width:0">
                <div class="sig-title">${esc(r.vendor)} 保修提醒</div>
                <div class="sig-detail">还剩 ${r.days_left} 天 · ${esc(r.warranty_end)} 截止</div>
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

    hero.innerHTML = `
      <div class="hero-primary">
        <div class="hero-top-bar">
          ${curTabsHtml}
          <span class="micro-pill slate">${activeCur.bill_count} 笔</span>
        </div>

        <div class="hero-figures">
          <div>
            <div class="hero-big-label">${esc(activeCur.currency)} 合计</div>
            <div class="hero-big-num sensitive">${renderEditorialMoney(activeCur.currency, activeCur.total_recorded, false)}</div>
          </div>
          <div class="hero-secondary-stats">
            <div>
              <div class="mini-stat-k">订阅月均</div>
              <div class="mini-stat-v sensitive">${esc(formatMoneyShort(activeCur.currency, activeCur.monthly_equiv_recurring))}<span style="font-size:11.5px;color:var(--ink-3);font-weight:500"> /月</span></div>
            </div>
            <div>
              <div class="mini-stat-k">订阅年计</div>
              <div class="mini-stat-v sensitive">${esc(formatMoneyShort(activeCur.currency, activeCur.yearly_equiv_recurring))}</div>
            </div>
          </div>
        </div>

        ${ribbonHtml}
      </div>

      <div class="hero-signals">${signalCards.join("")}</div>
    `;

    hero.querySelectorAll("[data-hero-cur]").forEach((btn) => {
      btn.addEventListener("click", () => {
        state.heroCurrency = btn.getAttribute("data-hero-cur");
        renderEditorialHero(ws);
      });
    });
    hero.querySelectorAll("[data-ribbon-kind]").forEach((el) => {
      el.addEventListener("click", () => {
        const k = el.getAttribute("data-ribbon-kind");
        if (["subscription", "license", "hardware", "reimbursable"].includes(k)) {
          state.collection = state.collection === k ? "all" : k;
          renderAssetsPane(ws);
        }
      });
    });
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
          <div class="oc-amount sensitive">${renderEditorialMoney(amtVal.currency, amtVal.amount_text, true)}</div>
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
      if (prog) {
        bodyHtml = `
          <div>
            <div class="meter-caption"><span>${esc(prog.range)}</span><span>${esc(prog.statusLabel)}</span></div>
            ${renderTickMeter(prog.pct, prog.expired)}
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

    return `
      <article class="obj-card kind-${esc(kind)} ${prog?.expired ? "is-expired" : ""}" data-source-id="${esc(item.source_id)}">
        ${spineHtml}
        <div class="oc-top">
          <div class="oc-id">
            <div class="brand-badge" style="background:${brand.bg}">${esc(brand.mark)}</div>
            <div style="min-width:0">
              <div class="oc-title">${esc(title)}</div>
              <div class="oc-sub">${esc(sub)}</div>
            </div>
          </div>
          <span class="micro-pill ${flags.has_conflict ? "amber" : prog?.expired ? "slate" : kindMeta.pill}">${flags.has_conflict ? "冲突" : prog?.expired ? "往期" : esc(kindMeta.label)}</span>
        </div>

        <div class="oc-price-row">${priceHtml}</div>
        ${bodyHtml}

        <div class="oc-foot ${kind === "reimbursable" ? "" : "ruled"}">
          <span class="sensitive" style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(metaStr)}</span>
          ${stampHtml || (item.object_intact ? `<span class="verified-check" title="原件校验完整">${ICO.check}</span>` : "")}
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
      <div class="filing-strip" data-source-id="${esc(item.source_id)}">
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
    } else if (state.collection === "subscription") {
      // Group expired past-period bills below active subscriptions when there are active ones
      const active = [];
      const expired = [];
      for (const it of list) {
        const p = periodProgressInfo(it.period_fact?.value);
        if (p && p.expired) expired.push(it);
        else active.push(it);
      }
      if (active.length > 0 && expired.length > 0) {
        mainItems = active;
        secondaryItems = expired;
        secondaryLabel = "往期历史账单";
      }
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
        </section>
      `;
    }).join("");

    stage.innerHTML = `<div class="csv-workspace-stack">${cardsHtml}</div>`;

    list.forEach((item, cIdx) => {
      const allRows = (item.candidates || [])
        .filter((c) => c.kind === "csv_transaction")
        .map((c) => ({ ...c.value, raw_line: c.excerpt }));
      const tbody = stage.querySelector(`[data-csv-tbody="${cIdx}"]`);
      const qInput = stage.querySelector(`[data-csv-filter-q="${cIdx}"]`);
      const expChk = stage.querySelector(`[data-csv-filter-exp="${cIdx}"]`);

      const paintRows = () => {
        const q = (qInput.value || "").trim().toLowerCase();
        const expOnly = expChk.checked;
        const filtered = allRows.filter((r) => {
          if (expOnly && r.direction !== "expense") return false;
          if (q && !`${r.merchant || ""} ${r.description || ""} ${r.date || ""} ${r.amount_text || ""}`.toLowerCase().includes(q)) return false;
          return true;
        });
        if (!filtered.length) {
          tbody.innerHTML = `<tr><td colspan="5" style="text-align:center;color:var(--ink-3);padding:24px">没有符合条件的流水记录</td></tr>`;
          return;
        }
        tbody.innerHTML = filtered.slice(0, 150).map((r, rIdx) => `
          <tr>
            <td style="font-size:12px;color:var(--ink-3);white-space:nowrap" class="num">${esc(r.date || "—")}</td>
            <td style="font-weight:600">${esc(r.merchant || "未标注商户")}</td>
            <td style="color:var(--ink-2)">${esc(r.description || "—")}</td>
            <td class="right sensitive num" style="font-weight:700;white-space:nowrap">${esc(formatMoney(r.currency || "CNY", r.amount_text))}</td>
            <td class="right"><button type="button" class="btn btn-soft btn-sm" data-ws-promote="${rIdx}">加入账本</button></td>
          </tr>
        `).join("");

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

      qInput.addEventListener("input", paintRows);
      expChk.addEventListener("change", paintRows);
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
        <tr data-source-id="${esc(item.source_id)}">
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
          <td class="right"><div class="btn-row" style="justify-content:flex-end">${acts.join("") || (item.object_intact ? `<span class="verified-check">${ICO.check}</span>` : "")}</div></td>
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
      el.addEventListener("click", () => {
        const found = (state.workspace.items || []).find((i) => i.source_id === el.getAttribute("data-source-id"));
        if (found) openDrawer(found);
      });
    });
  }

  // --- View B: 70/30 Split Timeline Calendar + Month Agenda ---
  function renderCalendarPane(ws) {
    const pane = $("pane-calendar");
    const year = state.calendarYear;
    const month = state.calendarMonth;
    const startWeekday = new Date(year, month, 1).getDay();
    const daysInMonth = new Date(year, month + 1, 0).getDate();
    const prevMonthDays = new Date(year, month, 0).getDate();
    const today = new Date();

    const latestRec = new Map();
    for (const item of ws.items || []) {
      const ann = item.annotation || {};
      const amtVal = item.amount_fact?.value;
      if (!amtVal?.amount_text || !["monthly", "quarterly", "yearly"].includes(ann.billing_cycle)) continue;
      const key = `${(ann.vendor || item.display_name).trim().toLowerCase()}|${amtVal.currency}|${ann.billing_cycle}`;
      const dStr = ann.bill_date || item.period_fact?.value?.period_start || "";
      const prev = latestRec.get(key);
      if (!prev || dStr >= prev.dStr) latestRec.set(key, { sid: item.source_id, dStr });
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
          addEvt(rd, { kind: "real", source_id: item.source_id, title, money, currency: amtVal.currency, cents, sub: "账单日" });
        }
      }
      if (!projectable.has(item.source_id)) continue;
      const anchorStr = perVal?.period_end || realDateStr;
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
            addEvt(clampedDay, { kind: "projected", source_id: item.source_id, title, money, currency: amtVal.currency, cents, sub: "预计续费" });
          }
        }
      }
    }

    agenda.sort((a, b) => a.day - b.day);
    const monthTotals = {};
    for (const ev of agenda) {
      if (ev.currency && ev.cents) monthTotals[ev.currency] = (monthTotals[ev.currency] || 0) + ev.cents;
    }
    const monthTotalRows = Object.entries(monthTotals);
    const monthFootHtml = monthTotalRows.length
      ? `
        <div class="agenda-total-foot">
          <div style="font-size:11.5px;font-weight:700;color:var(--ink-3)">${month + 1} 月合计（含预计续费）</div>
          ${monthTotalRows.map(([cur, c]) => `<div class="agenda-total-row"><span>${esc(cur)} · ${esc(CURRENCY_NAMES[cur] || cur)}</span><strong class="sensitive">${esc(formatMoneyShort(cur, centsToText(c)))}</strong></div>`).join("")}
        </div>
      `
      : "";

    let cells = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"].map((w) => `<div class="cal-wk">${w}</div>`).join("");
    for (let i = startWeekday - 1; i >= 0; i--) cells += `<div class="cal-cell other"><div class="cal-day">${prevMonthDays - i}</div></div>`;
    for (let d = 1; d <= daysInMonth; d++) {
      const isToday = today.getFullYear() === year && today.getMonth() === month && today.getDate() === d;
      const evts = eventsByDay[d] || [];
      cells += `
        <div class="cal-cell ${isToday ? "today" : ""}">
          <div class="cal-day">${d}${isToday ? " · 今天" : ""}</div>
          ${evts.map((ev) => `<button type="button" class="cal-evt ${ev.kind}" data-cal-sid="${esc(ev.source_id)}"><span class="cal-evt-name">${esc(ev.title)}</span><span class="${ev.kind === "warranty" ? "" : "sensitive num"}">${esc(ev.money)}</span></button>`).join("")}
        </div>
      `;
    }
    const trailing = (7 - ((startWeekday + daysInMonth) % 7)) % 7;
    for (let nd = 1; nd <= trailing; nd++) cells += `<div class="cal-cell other"><div class="cal-day">${nd}</div></div>`;

    const agendaHtml = agenda.length === 0
      ? `<p class="folio-p">本月没有账单、预计续费或保修到期。</p>`
      : `<div class="agenda-list">${agenda.map((ev) => `
          <div class="agenda-row" data-cal-sid="${esc(ev.source_id)}">
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
              <span><i class="ldot proj"></i>预计续费</span>
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
    pane.querySelectorAll("[data-cal-sid]").forEach((el) => {
      el.addEventListener("click", () => {
        const found = (ws.items || []).find((i) => i.source_id === el.getAttribute("data-cal-sid"));
        if (found) openDrawer(found);
      });
    });
  }

  // --- View C: Insights, Vendor Folio & Savings Studio ---
  function buildMiniSparkline(history, strokeColor) {
    if (!history || history.length < 2) return `<span style="font-size:11px;color:var(--ink-3)">单期记录</span>`;
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
      <div class="legend-row" data-jump-cat="${esc(row.name)}">
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
      <div class="legend-row" data-jump-acc="${esc(row.name)}">
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
            const spark = buildMiniSparkline(vp.history || [], drift?.direction === "up" ? "var(--rose)" : "var(--blue)");
            return `
              <div class="vendor-tile" data-jump-vendor="${esc(vp.vendor)}">
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

    const recurring = (ws.items || []).filter((it) => {
      const amt = it.amount_fact?.value;
      return amt && amt.currency === cur && ["monthly", "yearly", "quarterly", "weekly"].includes(it.annotation?.billing_cycle);
    });
    let yearlySave = 0;
    const cutList = [];
    const simRows = recurring.map((it) => {
      const ann = it.annotation || {};
      const amt = it.amount_fact.value;
      const mult = { monthly: 12, yearly: 1, quarterly: 4, weekly: 52 }[ann.billing_cycle] || 0;
      const yrCents = amountToCents(amt.amount_text) * mult;
      const isCut = state.simCutIds.has(it.source_id);
      if (isCut) {
        yearlySave += yrCents;
        cutList.push(`${ann.vendor || it.display_name}（${formatMoney(cur, amt.amount_text)} / ${CYCLE_LABELS[ann.billing_cycle]}）`);
      }
      return `
        <div class="sim-row ${isCut ? "cut" : ""}">
          <div style="min-width:0">
            <div class="sim-title" style="font-weight:700;font-size:13.5px">${esc(ann.vendor || it.display_name)}</div>
            <div style="font-size:12px;color:var(--ink-3)" class="sensitive num">${esc(formatMoneyShort(cur, amt.amount_text))} / ${esc(CYCLE_LABELS[ann.billing_cycle])} · 年合 ${esc(formatMoneyShort(cur, centsToText(yrCents)))}</div>
          </div>
          <label class="ios-switch"><input type="checkbox" data-sim-sid="${esc(it.source_id)}" ${isCut ? "checked" : ""}><span class="ios-slider"></span></label>
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
            <div class="sim-figure sensitive">每年少花 ${renderEditorialMoney(cur, centsToText(yearlySave), true)}</div>
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

    pane.querySelectorAll("[data-ins-cur]").forEach((b) => b.addEventListener("click", () => { state.insightsCurrency = b.getAttribute("data-ins-cur"); renderInsightsPane(ws); }));
    pane.querySelectorAll("[data-cat-slice]").forEach((c) => {
      c.addEventListener("mouseenter", () => {
        $("donut-center-label").textContent = `${c.getAttribute("data-cat-slice")} · ${c.getAttribute("data-cat-pct")}%`;
        $("donut-center-val").textContent = formatMoneyShort(cur, c.getAttribute("data-cat-amt"));
      });
      c.addEventListener("mouseleave", () => {
        $("donut-center-label").textContent = `${cur} 总额`;
        $("donut-center-val").textContent = formatMoneyShort(cur, centsToText(totalCat));
      });
      c.addEventListener("click", () => { state.categoryFilter = c.getAttribute("data-cat-slice"); state.currencyFilter = cur; navigateTo("assets", "all"); });
    });
    pane.querySelectorAll("[data-jump-cat]").forEach((r) => r.addEventListener("click", () => { state.categoryFilter = r.getAttribute("data-jump-cat"); state.currencyFilter = cur; navigateTo("assets", "all"); }));
    pane.querySelectorAll("[data-jump-acc]").forEach((r) => r.addEventListener("click", () => { const a = r.getAttribute("data-jump-acc"); state.accountFilter = a === "未分组" ? "" : a; navigateTo("assets", "all"); }));
    pane.querySelectorAll("[data-jump-vendor]").forEach((t) => t.addEventListener("click", () => { const v = t.getAttribute("data-jump-vendor") || ""; state.searchQuery = v; $("search-box").value = v; navigateTo("assets", "all"); }));
    pane.querySelectorAll("[data-sim-sid]").forEach((chk) => chk.addEventListener("change", () => {
      const sid = chk.getAttribute("data-sim-sid");
      if (chk.checked) state.simCutIds.add(sid); else state.simCutIds.delete(sid);
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
    if (!queue.length) {
      const skipped = state.triageSkippedIds.size;
      pane.innerHTML = `
        <div class="empty-gallery">
          <div class="empty-emblem" style="background:var(--green-bg);color:var(--green)">${ICO.sparkle}</div>
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
    const rawUrl = tokenUrl(`/api/sources/${encodeURIComponent(item.source_id)}/raw`);
    const previewHtml = item.media_type === "application/pdf"
      ? `<iframe src="${esc(rawUrl)}" title="PDF 原件"></iframe>`
      : item.media_type.startsWith("image/")
        ? `<img src="${esc(rawUrl)}" alt="${esc(item.display_name)}">`
        : item.media_type.startsWith("text/html")
          ? `<iframe sandbox="" src="${esc(rawUrl)}" title="静态 HTML"></iframe>`
          : `<pre id="triage-text-preview" class="code-sheet" style="margin:0">加载中…</pre>`;

    const curs = (state.bootstrap?.currencies || ["CNY", "USD", "EUR", "GBP", "JPY", "HKD"])
      .map((c) => `<option value="${esc(c)}">${esc(c)} ${esc(CURRENCY_SYMBOLS[c] || "")}</option>`).join("");

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
          <div class="preview-frame" style="margin-bottom:0">${previewHtml}</div>
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
              ${["monthly", "yearly", "quarterly", "one_time"].map((c) => `<button type="button" class="tap-chip ${ann.billing_cycle === c ? "active" : ""}" data-tr-cycle="${c}">${CYCLE_LABELS[c]}</button>`).join("")}
            </div>
            <div class="chip-strip">
              <span class="chip-strip-lbl">账号</span>
              ${["个人", "公司工作", "家庭共享"].map((a) => `<button type="button" class="tap-chip ${ann.account_label === a ? "active" : ""}" data-tr-acc="${a}">${a}</button>`).join("")}
            </div>
            <div class="chip-strip">
              <span class="chip-strip-lbl">分类</span>
              ${["AI 工具", "数码硬件", "云存储", "开发工具", "办公效率"].map((k) => `<button type="button" class="tap-chip ${ann.category === k ? "active" : ""}" data-tr-cat="${k}">${k}</button>`).join("")}
            </div>
            <div class="grid-2">
              <label class="field"><span>归属账号</span><input id="tr-account" type="text" value="${esc(ann.account_label || "个人")}"></label>
              <label class="field"><span>分类</span><input id="tr-category" type="text" value="${esc(ann.category || "")}"></label>
            </div>
            <label class="field"><span>账单日期（可选）</span><input id="tr-date" type="date" value="${esc(ann.bill_date || "")}"></label>
            <button id="btn-triage-save" class="btn btn-ink btn-lg btn-block" type="submit">保存并切下一张</button>
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
        if (getTriageQueue(snap).length === 0) { launchConfetti(); showToast("待整理项已全部处理", "ok"); }
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
    wrap.querySelectorAll("[data-open-sid]").forEach((btn) => btn.addEventListener("click", () => {
      const found = items.find((i) => i.source_id === btn.getAttribute("data-open-sid"));
      if (found) openDrawer(found);
    }));
  }

  // --- Floating Detail Drawer ---
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
    document.querySelectorAll(".dtab").forEach((b) => b.classList.toggle("active", b.getAttribute("data-dtab") === tab));
    ["edit", "evidence", "csv", "history", "redact"].forEach((n) => $(`dpane-${n}`).classList.toggle("hidden", n !== tab));
    if (tab === "evidence" && state.selectedSourceId) loadEvidencePreview(state.selectedSourceId);
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
    $("btn-download-raw").href = tokenUrl(`/api/sources/${encodeURIComponent(item.source_id)}/raw?download=1`);

    $("evidence-occurrences").innerHTML = `
      <div class="micro-eyebrow" style="margin:6px 0 2px">导入记录（共 ${item.occurrence_count} 次）</div>
      ${(item.occurrences || []).map((o) => `<div class="occ-row"><strong>${esc(o.display_name)}</strong> · ${formatTime(o.recorded_at)} <span class="micro-pill ${o.storage_reused ? "slate" : "green"}" style="margin-left:6px">${o.storage_reused ? "防重复用" : "首次入库"}</span></div>`).join("")}
    `;

    if (item.status_flags.is_csv_statement) renderCsvRows(item);
    renderHistoryTimeline(item);
  }

  async function loadEvidencePreview(sourceId) {
    const item = (state.workspace?.items || []).find((i) => i.source_id === sourceId);
    if (!item) return;
    const box = $("evidence-preview-box"), textBox = $("evidence-text-box");
    const rawUrl = tokenUrl(`/api/sources/${encodeURIComponent(sourceId)}/raw`);
    if (item.media_type === "application/pdf") { box.innerHTML = `<iframe src="${esc(rawUrl)}" title="PDF 原件"></iframe>`; box.classList.remove("hidden"); }
    else if (item.media_type.startsWith("image/")) { box.innerHTML = `<img src="${esc(rawUrl)}" alt="${esc(item.display_name)}">`; box.classList.remove("hidden"); }
    else if (item.media_type.startsWith("text/html")) { box.innerHTML = `<iframe sandbox="" src="${esc(rawUrl)}" title="静态 HTML 原件"></iframe>`; box.classList.remove("hidden"); }
    else { box.classList.add("hidden"); state.textViewActive = true; }
    textBox.classList.toggle("hidden", !state.textViewActive);
    try {
      const res = await api("GET", `/api/sources/${encodeURIComponent(sourceId)}/text`);
      textBox.textContent = res.text || "（无纯文本预览）";
    } catch (err) { textBox.textContent = `文本加载失败：${err.message}`; }
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
    box.innerHTML = rows.slice(0, 150).map((r, idx) => `
      <div class="csv-row">
        <div>
          <strong>${esc(r.merchant || "未标注商户")} · <span class="sensitive num">${esc(formatMoney(r.currency || "CNY", r.amount_text))}</span></strong>
          <small>${esc(r.date || "")} · ${esc(r.description || "")}</small>
        </div>
        <button type="button" class="btn btn-soft btn-sm" data-csv-promote="${idx}">加入账本</button>
      </div>
    `).join("");
    box.querySelectorAll("[data-csv-promote]").forEach((btn) => btn.addEventListener("click", async () => {
      const r = rows[Number(btn.getAttribute("data-csv-promote"))];
      if (!r) return;
      btn.disabled = true; btn.textContent = "加入中…";
      try {
        const res = await api("POST", "/api/csv/promote", {
          parent_source_id: item.source_id, row_number: r.row_number, merchant: r.merchant || "", description: r.description || "",
          date: r.date || "", currency: r.currency || "CNY", amount_text: r.amount_text || "", billing_cycle: "unknown",
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
      } catch (err) { showToast(err.message, "error"); }
    }));
  }

  // --- Spotlight Command Palette ---
  function openCmdPalette() {
    if (!state.workspace) return;
    state.cmdOpen = true; state.cmdIndex = 0;
    $("modal-cmd").classList.remove("hidden");
    $("cmd-input").value = "";
    renderCmdList("");
    $("cmd-input").focus();
  }
  function closeCmdPalette() { state.cmdOpen = false; $("modal-cmd").classList.add("hidden"); }

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
      { ico: ICO.sun, label: "切换外观", sub: "浅色 / 暖白 / 深色", run: () => cycleTheme() },
      { ico: ICO.eye, label: "隐藏或显示金额", sub: "快捷键 P", run: () => togglePrivacy() },
      { ico: ICO.lock, label: "锁定账本", sub: "返回解锁界面", run: () => $("btn-lock-vault").click() },
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

    const filtered = [...acts, ...assets].filter((a) => !q || a.label.toLowerCase().includes(q) || a.sub.toLowerCase().includes(q));
    state.cmdActions = filtered.slice(0, 12);
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

  function openManualModal() {
    $("modal-manual").classList.remove("hidden");
    $("man-vendor").value = ""; $("man-plan").value = ""; $("man-amount").value = ""; $("man-notes").value = "";
    $("man-license-key").value = ""; $("man-license-key").type = "password"; $("btn-man-toggle-key").textContent = "显示";
    $("man-license-scope").value = ""; $("man-warranty-end").value = ""; $("man-reimburse-status").value = "pending";
    $("man-bill-date").value = new Date().toISOString().slice(0, 10);
    setManualAssetKind("subscription");
    $("man-vendor").focus();
  }
  function closeManualModal() { $("modal-manual").classList.add("hidden"); }

  // --- Upload & Drop ---
  async function importFileList(files) {
    const valid = Array.from(files).filter((f) => f && f.size > 0);
    if (!valid.length) return;
    const prog = $("import-progress");
    prog.classList.remove("hidden");
    const batchId = `b_${Date.now().toString(36)}`;
    let added = 0, reused = 0, failed = 0, lastError = "";
    for (let i = 0; i < valid.length; i++) {
      const f = valid[i];
      prog.textContent = `导入 ${i + 1}/${valid.length} · ${f.name}`;
      try {
        const res = await api("POST", "/api/import", { filename: f.name, content_base64: arrayBufferToBase64(await f.arrayBuffer()), batch_id: batchId });
        if (res.reused) reused += 1; else added += 1;
      } catch (err) { failed += 1; lastError = `${f.name}: ${err.message}`; }
    }
    prog.classList.add("hidden");
    await refreshWorkspace();
    const parts = [];
    if (added) parts.push(`已导入 ${added} 份`);
    if (reused) parts.push(`跳过重复 ${reused} 份`);
    if (failed) parts.push(`失败 ${failed} 份（${lastError}）`);
    showToast(parts.join("，") || "导入完成", failed && !added ? "error" : "ok");
  }

  async function collectDroppedFiles(dt) {
    const out = [];
    const items = dt.items ? Array.from(dt.items) : [];
    if (!items.some((it) => typeof it.webkitGetAsEntry === "function")) return Array.from(dt.files || []);
    async function walk(entry) {
      if (!entry) return;
      if (entry.isFile) { const f = await new Promise((r) => entry.file(r, () => r(null))); if (f) out.push(f); }
      else if (entry.isDirectory) {
        const reader = entry.createReader();
        while (true) {
          const b = await new Promise((r) => reader.readEntries(r, () => r([])));
          if (!b.length) break;
          for (const c of b) await walk(c);
        }
      }
    }
    for (const it of items) { const e = it.webkitGetAsEntry(); if (e) await walk(e); }
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

    // Vault pill popover
    $("vault-pill-btn").addEventListener("click", (e) => {
      e.stopPropagation();
      $("vault-popover").classList.toggle("hidden");
    });
    document.addEventListener("click", (e) => {
      if (!$("vault-popover").contains(e.target) && !$("vault-pill-btn").contains(e.target)) {
        $("vault-popover").classList.add("hidden");
      }
    });

    // Keyboard shortcuts
    document.addEventListener("keydown", (e) => {
      const inInput = ["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName);
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
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
        $("vault-popover").classList.add("hidden");
        if (!$("modal-manual").classList.contains("hidden")) { closeManualModal(); return; }
        if (state.selectedSourceId) { closeDrawer(); return; }
      }
      if (!inInput && !$("screen-workspace").classList.contains("hidden")) {
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
      const btn = $("btn-unlock-submit"); btn.disabled = true; btn.textContent = "解锁中…";
      try {
        const payload = { path: $("unlock-path").value.trim() };
        if (state.unlockMode === "passphrase") payload.passphrase = $("unlock-passphrase").value;
        else if (state.unlockMode === "recovery") payload.recovery_key = $("unlock-recovery").value.trim();
        const snap = await api("POST", "/api/vault/unlock", payload);
        $("unlock-passphrase").value = ""; $("unlock-recovery").value = "";
        state.workspace = snap; showWorkspaceScreen(); showToast("账本已打开", "ok");
      } catch (err) { errBox.textContent = err.message; errBox.classList.remove("hidden"); }
      finally { btn.disabled = false; btn.textContent = "打开账本"; }
    });

    $("form-create-vault").addEventListener("submit", async (e) => {
      e.preventDefault();
      const errBox = $("create-error"); errBox.classList.add("hidden");
      const prot = document.querySelector('input[name="create-protection"]:checked').value;
      const pass = $("create-passphrase").value, cpass = $("create-passphrase-confirm").value;
      if (prot === "passphrase") {
        if (pass.length < 6) { errBox.textContent = "密码至少需要 6 位字符"; errBox.classList.remove("hidden"); return; }
        if (pass !== cpass) { errBox.textContent = "两次输入的密码不一致"; errBox.classList.remove("hidden"); return; }
      }
      const btn = $("btn-create-submit"); btn.disabled = true; btn.textContent = "创建中…";
      try {
        const res = await api("POST", "/api/vault/init", { path: $("create-path").value.trim(), protection: prot, passphrase: prot === "passphrase" ? pass : "" });
        $("create-passphrase").value = ""; $("create-passphrase-confirm").value = "";
        await refreshWorkspace();
        if (res.recovery_key) openRecoveryModal(res.recovery_key, () => { showWorkspaceScreen(); showToast("账本已创建", "ok"); });
        else { showWorkspaceScreen(); showToast("账本已创建", "ok"); }
      } catch (err) { errBox.textContent = err.message; errBox.classList.remove("hidden"); }
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
        const snap = await api("POST", "/api/manual-entry", payload);
        state.workspace = snap; closeManualModal(); renderWorkspace(); showToast("已保存", "ok");
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
      if (state.workspace) renderAssetsPane(state.workspace);
    }));

    // Upload, lock, shutdown
    $("file-picker-input").addEventListener("change", (e) => { importFileList(e.target.files); e.target.value = ""; });
    $("folder-picker-input").addEventListener("change", (e) => { importFileList(e.target.files); e.target.value = ""; });
    $("btn-lock-vault").addEventListener("click", async () => {
      $("vault-popover").classList.add("hidden");
      await api("POST", "/api/vault/lock", {});
      state.workspace = null;
      const info = await api("GET", "/api/bootstrap");
      renderRecentVaults(info.recent_vaults || []);
      showLobbyScreen(); showToast("账本已锁定", "ok");
    });
    $("btn-shutdown-app").addEventListener("click", async () => {
      try { await api("POST", "/api/shutdown", {}); } catch (_) {}
      document.body.innerHTML = `<div style="max-width:440px;margin:130px auto;text-align:center;font-family:var(--font-sans);padding:24px"><h2 style="margin:0 0 8px">SubVault 已退出</h2><p style="margin:0;color:var(--ink-2)">本地服务已停止，可以直接关闭这个页面。</p></div>`;
    });

    // Global drag & drop
    const overlay = $("global-drop-overlay");
    window.addEventListener("dragenter", (e) => { if ($("screen-workspace").classList.contains("hidden")) return; e.preventDefault(); state.dragDepth += 1; overlay.classList.remove("hidden"); });
    window.addEventListener("dragover", (e) => { if ($("screen-workspace").classList.contains("hidden")) return; e.preventDefault(); });
    window.addEventListener("dragleave", () => { state.dragDepth = Math.max(0, state.dragDepth - 1); if (state.dragDepth === 0) overlay.classList.add("hidden"); });
    window.addEventListener("drop", async (e) => {
      state.dragDepth = 0; overlay.classList.add("hidden");
      if ($("screen-workspace").classList.contains("hidden")) return;
      e.preventDefault(); importFileList(await collectDroppedFiles(e.dataTransfer));
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
        showToast("已保存修改", "ok");
      } catch (err) { showToast(err.message, "error"); }
      finally { btn.disabled = false; btn.textContent = "保存修改"; }
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

    $("btn-dl-excel").addEventListener("click", () => { window.location.href = tokenUrl("/api/export/csv"); });
    $("btn-dl-zip").addEventListener("click", () => {
      const enc = state.workspace?.vault?.encrypted;
      if (enc && !$("chk-zip-ack").checked) { showToast("请先勾选明文导出确认框", "error"); return; }
      window.location.href = tokenUrl(`/api/export/zip${enc ? "?acknowledge_plaintext=yes" : ""}`);
    });
    $("btn-export-dir").addEventListener("click", async () => {
      const outDir = $("export-dir-path").value.trim(), enc = state.workspace?.vault?.encrypted, ack = $("chk-dir-ack").checked;
      const resBox = $("export-dir-result"); resBox.classList.add("hidden");
      if (!outDir) { showToast("请填写导出文件夹路径", "error"); return; }
      if (enc && !ack) { showToast("请先勾选明文导出确认框", "error"); return; }
      try {
        const res = await api("POST", "/api/export/package", { out_dir: outDir, acknowledge_plaintext: Boolean(ack) });
        resBox.className = "inline-banner";
        resBox.textContent = res.verified ? `导出并校验通过：共核验 ${res.verification?.files_verified ?? "?"} 个文件` : `已导出到 ${outDir}，但校验存在提醒`;
        resBox.classList.remove("hidden"); $("verify-dir-path").value = outDir;
      } catch (err) { resBox.className = "inline-banner error"; resBox.textContent = err.message; resBox.classList.remove("hidden"); }
    });
    $("btn-verify-dir").addEventListener("click", async () => {
      const dir = $("verify-dir-path").value.trim(), resBox = $("verify-dir-result"); resBox.classList.add("hidden");
      if (!dir) { showToast("请填写备份包目录路径", "error"); return; }
      try {
        const res = await api("POST", "/api/verify-package", { package_dir: dir });
        resBox.className = res.verified ? "inline-banner" : "inline-banner error";
        resBox.textContent = res.verified ? `校验通过：共 ${res.report?.files_verified ?? "?"} 个文件完整无篡改` : `校验未通过：${res.report?.reason || "发现文件缺失或被修改"}`;
        resBox.classList.remove("hidden");
      } catch (err) { resBox.className = "inline-banner error"; resBox.textContent = err.message; resBox.classList.remove("hidden"); }
    });
    $("form-rotate-pass").addEventListener("submit", async (e) => {
      e.preventDefault();
      const p1 = $("rotate-new-pass").value, p2 = $("rotate-new-pass-confirm").value, rot = $("chk-rotate-recovery").checked;
      if (p1.length < 6) { showToast("新密码至少需要 6 位字符", "error"); return; }
      if (p1 !== p2) { showToast("两次输入的新密码不一致", "error"); return; }
      try {
        const res = await api("POST", "/api/vault/rotate-passphrase", { new_passphrase: p1, rotate_recovery_key: rot });
        $("rotate-new-pass").value = ""; $("rotate-new-pass-confirm").value = ""; $("chk-rotate-recovery").checked = false;
        if (res.recovery_key) openRecoveryModal(res.recovery_key, () => showToast("主密码与恢复密钥已更新", "ok"));
        else showToast("主密码已更新", "ok");
      } catch (err) { showToast(err.message, "error"); }
    });
  }

  document.addEventListener("DOMContentLoaded", boot);
})();
