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

  const THEMES = [
    { id: "paper", label: "纸感账簿", icon: "◧" },
    { id: "midnight", label: "深海金库", icon: "☾" },
    { id: "studio", label: "清爽极简", icon: "○" },
  ];

  const CHART_PALETTE = [
    "#0f766e",
    "#2563eb",
    "#d97706",
    "#9333ea",
    "#e11d48",
    "#059669",
    "#0891b2",
    "#6366f1",
  ];

  const BRAND_PRESETS = {
    openai: { bg: "#10a37f", mark: "AI" },
    anthropic: { bg: "#c96442", mark: "CL" },
    apple: { bg: "#1d1d1f", mark: "AP" },
    github: { bg: "#24292f", mark: "GH" },
    jetbrains: { bg: "#7c3aed", mark: "JB" },
    netflix: { bg: "#dc2626", mark: "NF" },
    spotify: { bg: "#16a34a", mark: "SP" },
    notion: { bg: "#334155", mark: "NT" },
    figma: { bg: "#9333ea", mark: "FG" },
    adobe: { bg: "#b91c1c", mark: "AD" },
    aws: { bg: "#d97706", mark: "AW" },
    cloudflare: { bg: "#ea580c", mark: "CF" },
    wps: { bg: "#2563eb", mark: "WP" },
    金山: { bg: "#2563eb", mark: "金" },
    支付宝: { bg: "#0284c7", mark: "支" },
    微信: { bg: "#16a34a", mark: "微" },
    哔哩: { bg: "#ec4899", mark: "B" },
    腾讯: { bg: "#0284c7", mark: "腾" },
    阿里: { bg: "#ea580c", mark: "阿" },
    京东: { bg: "#dc2626", mark: "京" },
    百度: { bg: "#2563eb", mark: "百" },
    raycast: { bg: "#e11d48", mark: "RC" },
    macbook: { bg: "#1d1d1f", mark: "MB" },
  };

  const ASSET_KIND_META = {
    subscription: { label: "订阅", full: "周期订阅", badgeClass: "badge-emerald" },
    license: { label: "买断", full: "买断授权", badgeClass: "badge-blue" },
    hardware: { label: "硬件", full: "硬件保修", badgeClass: "badge-violet" },
    reimbursable: { label: "报销", full: "报销凭证", badgeClass: "badge-amber" },
    general: { label: "账单", full: "普通账单", badgeClass: "badge-slate" },
  };

  const REIMBURSE_META = {
    pending: { label: "待报销", next: "submitted", cls: "pending" },
    submitted: { label: "已提交", next: "reimbursed", cls: "submitted" },
    reimbursed: { label: "已到账", next: "pending", cls: "reimbursed" },
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

  const CYCLE_LABELS = {
    monthly: "月付",
    yearly: "年付",
    quarterly: "季付",
    weekly: "周付",
    one_time: "一次性",
    unknown: "未标注",
  };

  const state = {
    bootstrap: null,
    workspace: null,
    theme: "paper",
    privacyBlur: false,
    unlockMode: "passphrase",
    activeTab: "ledger",
    viewMode: "cards", // "cards" | "table" | "calendar"
    ledgerFilter: "all",
    accountFilter: "",
    currencyFilter: "",
    categoryFilter: "",
    searchQuery: "",
    calendarYear: new Date().getFullYear(),
    calendarMonth: new Date().getMonth(), // 0-based
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
    for (let i = 0; i < raw.length; i++) {
      h = (h * 31 + raw.charCodeAt(i)) >>> 0;
    }
    const bg = CHART_PALETTE[h % CHART_PALETTE.length];
    const chars = Array.from(raw);
    const first = chars[0] || "S";
    const mark = /[\u4e00-\u9fff]/.test(first) ? first : raw.slice(0, 2).toUpperCase();
    return { bg, mark };
  }

  function formatMoney(currency, amountText) {
    if (!amountText) return "";
    const sym = CURRENCY_SYMBOLS[currency] || `${currency} `;
    return `${sym}${amountText} ${currency}`;
  }

  function formatMoneyShort(currency, amountText) {
    if (!amountText) return "";
    const sym = CURRENCY_SYMBOLS[currency] || "";
    return `${sym}${amountText}`;
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
    toastTimer = setTimeout(() => el.classList.add("hidden"), 4800);
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
    $("theme-icon").textContent = found.icon;
    $("theme-label").textContent = found.label;
    try {
      localStorage.setItem("subvault_ui_theme", found.id);
    } catch (_) {}
  }

  function cycleTheme() {
    const idx = THEMES.findIndex((t) => t.id === state.theme);
    const next = THEMES[(idx + 1) % THEMES.length];
    applyTheme(next.id);
    showToast(`已切换至「${next.label}」主题`);
  }

  function togglePrivacy(forceVal) {
    state.privacyBlur = forceVal !== undefined ? forceVal : !state.privacyBlur;
    document.body.classList.toggle("privacy-blur", state.privacyBlur);
    $("btn-privacy-toggle").classList.toggle("active", state.privacyBlur);
    $("privacy-label").textContent = state.privacyBlur ? "防窥中" : "防窥";
    if (state.privacyBlur) {
      showToast("隐私防窥已开启：金额与账号已打码（鼠标悬停单条可临时查看）");
    }
  }

  // --- Lightweight Celebration Confetti ---
  function launchConfetti() {
    const canvas = $("confetti-canvas");
    if (!canvas) return;
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    canvas.classList.remove("hidden");
    const ctx = canvas.getContext("2d");
    const pieces = Array.from({ length: 75 }, () => ({
      x: Math.random() * canvas.width,
      y: -20 - Math.random() * 140,
      r: 4 + Math.random() * 5,
      dx: (Math.random() - 0.5) * 4,
      dy: 2.5 + Math.random() * 4.5,
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
      if (frame < 110) {
        requestAnimationFrame(tick);
      } else {
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        canvas.classList.add("hidden");
      }
    }
    requestAnimationFrame(tick);
  }

  // --- Bootstrap & Lobby ---
  async function boot() {
    try {
      const savedTheme = localStorage.getItem("subvault_ui_theme");
      applyTheme(savedTheme || "paper");
    } catch (_) {
      applyTheme("paper");
    }
    bindEvents();
    if (!SESSION_TOKEN) {
      showToast("缺少会话凭证，请通过双击 subvault-app 打开本页面。", "error");
      return;
    }
    try {
      const info = await api("GET", "/api/bootstrap");
      state.bootstrap = info;
      const curs = info.currencies || ["CNY", "USD", "EUR", "GBP", "JPY", "HKD"];
      populateCurrencySelect($("ed-currency"), curs);
      populateCurrencySelect($("man-currency"), curs);
      $("create-path").value = info.default_vault_path || "";
      if (info.recent_vaults && info.recent_vaults.length > 0) {
        selectRecentVault(info.recent_vaults[0]);
      } else {
        $("unlock-path").value = info.default_vault_path || "";
      }
      renderRecentVaults(info.recent_vaults || []);
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

  function renderRecentVaults(recent) {
    const container = $("recent-vaults-list");
    if (!recent.length) {
      container.innerHTML = "";
      return;
    }
    container.innerHTML = recent
      .map((v, idx) => {
        const badge = v.needs_passphrase
          ? `<span class="badge badge-emerald">密码加密</span>`
          : `<span class="badge badge-slate">系统盘模式</span>`;
        return `
          <button type="button" class="recent-item" data-recent-idx="${idx}">
            <div>
              <div class="recent-item-name">${esc(v.name)}</div>
              <div class="recent-item-path">${esc(v.path)}</div>
            </div>
            ${badge}
          </button>
        `;
      })
      .join("");
    container.querySelectorAll("[data-recent-idx]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const target = recent[Number(btn.getAttribute("data-recent-idx"))];
        if (target) selectRecentVault(target);
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
    $("topbar-workspace-actions").classList.add("hidden");
    $("btn-privacy-toggle").classList.add("hidden");
    $("btn-cmd-open").classList.add("hidden");
    $("vault-status-chip").textContent = "纯本地 · 不联网";
    $("vault-status-chip").className = "chip chip-neutral";
    $("vault-path-sub").textContent = "零银行登录 · 零邮箱授权 · 私人票据防篡改档案馆";
    closeDrawer();
  }

  function showWorkspaceScreen() {
    $("screen-lobby").classList.add("hidden");
    $("screen-workspace").classList.remove("hidden");
    $("topbar-workspace-actions").classList.remove("hidden");
    $("btn-privacy-toggle").classList.remove("hidden");
    $("btn-cmd-open").classList.remove("hidden");
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

  function switchMainTab(tab) {
    state.activeTab = tab;
    document.querySelectorAll(".tab-item").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-tab") === tab);
    });
    ["ledger", "insights", "triage", "attention", "export"].forEach((name) => {
      $(`pane-${name}`).classList.toggle("hidden", name !== tab);
    });
    if (state.workspace) {
      if (tab === "insights") renderInsightsPane(state.workspace);
      if (tab === "triage") renderTriagePane(state.workspace);
      if (tab === "attention") renderAttentionPane(state.workspace);
    }
  }

  // --- Render Workspace ---
  function renderWorkspace() {
    const ws = state.workspace;
    if (!ws) return;
    const v = ws.vault;

    $("vault-status-chip").textContent = v.encrypted ? "已加密保护 · 本机解锁" : "系统盘模式 · 本机运行";
    $("vault-status-chip").className = v.encrypted ? "chip chip-emerald" : "chip chip-neutral";
    $("vault-path-sub").textContent = `${v.name} — ${v.path}`;

    $("zip-ack-row").classList.toggle("hidden", !v.encrypted);
    $("dir-ack-row").classList.toggle("hidden", !v.encrypted);
    $("card-security-rotate").classList.toggle("hidden", !v.encrypted);

    const items = ws.items || [];
    const triageItems = getTriageQueue(ws);
    $("count-tab-ledger").textContent = String(items.length);
    $("count-tab-triage").textContent = String(triageItems.length);
    $("count-tab-attention").textContent = String(v.duplicate_count + v.conflict_count + v.needs_entry_count);

    // Quick triage banner CTA
    const cta = $("btn-quick-triage-cta");
    if (triageItems.length > 0) {
      $("cta-triage-count").textContent = String(triageItems.length);
      cta.classList.remove("hidden");
    } else {
      cta.classList.add("hidden");
    }

    renderAssetRadar(ws);
    renderSummary(ws);
    syncAssetFilterCounts(ws);
    syncFilterDropdowns(ws);
    renderLedgerStage(ws);
    if (state.activeTab === "insights") renderInsightsPane(ws);
    if (state.activeTab === "triage") renderTriagePane(ws);
    if (state.activeTab === "attention") renderAttentionPane(ws);
  }

  function maskLicenseKey(key) {
    const s = String(key || "").trim();
    if (!s) return "";
    if (s.length <= 4) return "••••";
    return `••••-••••-${s.slice(-4)}`;
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
    const level = daysLeft < 0 ? "expired" : daysLeft <= 60 ? "warn" : "ok";
    const statusLabel = daysLeft > 0
      ? `保修还剩 ${daysLeft} 天`
      : daysLeft === 0
        ? "保修今日到期"
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
    return {
      daysUsed,
      perDayText: formatMoneyShort(currency, centsToText(perDayCents)),
    };
  }

  async function copyLicenseKey(vendor, key) {
    try {
      await navigator.clipboard.writeText(key);
      showToast(`「${vendor || "该授权"}」激活密钥已复制，请注意防窥保管`, "ok");
    } catch (_) {
      showToast("浏览器限制了剪贴板访问，请点开详情查看密钥", "error");
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

  function setLedgerFilter(filterKey) {
    state.ledgerFilter = filterKey;
    $("ledger-filters").querySelectorAll(".pill").forEach((x) => {
      x.classList.toggle("active", x.getAttribute("data-filter") === filterKey);
    });
  }

  function renderAssetRadar(ws) {
    const bar = $("asset-radar");
    const radar = ws.asset_radar || [];
    if (radar.length === 0) {
      bar.classList.add("hidden");
      bar.innerHTML = "";
      return;
    }
    const chipsHtml = radar
      .map((r, idx) => {
        const icon = r.kind === "price_hike"
          ? "📈"
          : r.kind === "warranty_expiring"
            ? "🛡️"
            : "🧾";
        const sev = r.severity || "slate";
        return `
          <button type="button" class="radar-chip ${esc(sev)}" data-radar-idx="${idx}" title="${esc(r.detail || "")}">
            <span>${icon}</span>
            <span class="sensitive">${esc(r.label)}</span>
          </button>
        `;
      })
      .join("");
    bar.innerHTML = `<span class="radar-title">资产雷达</span>${chipsHtml}`;
    bar.classList.remove("hidden");
    bar.querySelectorAll("[data-radar-idx]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const r = radar[Number(btn.getAttribute("data-radar-idx"))];
        if (!r) return;
        if (r.source_id) {
          const found = (ws.items || []).find((i) => i.source_id === r.source_id);
          if (found) openDrawer(found);
        } else if (r.kind === "reimburse_pending" || r.kind === "reimburse_submitted") {
          switchMainTab("ledger");
          setLedgerFilter("reimbursable");
          renderLedgerStage(ws);
        }
      });
    });
  }

  function syncAssetFilterCounts(ws) {
    const counts = ws.asset_counts || {};
    ["subscription", "license", "hardware", "reimbursable"].forEach((k) => {
      const el = document.querySelector(`.pill-cnt[data-cnt="${k}"]`);
      if (el) el.textContent = counts[k] > 0 ? ` ${counts[k]}` : "";
    });
  }

  function renderSummary(ws) {
    const grid = $("summary-section");
    const v = ws.vault;
    const curList = ws.summary_by_currency || [];
    const ac = ws.asset_counts || {};
    const cards = [];

    if (curList.length === 0) {
      cards.push(`
        <div class="kpi-card">
          <div class="kpi-label"><span>已登记资产与账单</span><span class="badge badge-slate">暂无金额</span></div>
          <div class="kpi-value sensitive">0.00</div>
          <div class="kpi-sub">拖入账单或点「+ 手动记一笔」立刻开始</div>
        </div>
      `);
    } else {
      for (const c of curList) {
        const recurLine = c.recurring_count > 0
          ? `周期订阅折算约 <strong class="sensitive">${esc(formatMoneyShort(c.currency, c.monthly_equiv_recurring))}/月</strong> · <span class="sensitive">${esc(formatMoneyShort(c.currency, c.yearly_equiv_recurring))}/年</span>（${c.recurring_count} 项）`
          : `已登记 ${c.bill_count} 笔账单（标记周期即可看月均/年均）`;
        cards.push(`
          <div class="kpi-card">
            <div class="kpi-label">
              <span>${esc(c.currency)} 资产与花费</span>
              <span class="badge badge-emerald">${c.bill_count} 笔</span>
            </div>
            <div class="kpi-value sensitive">${esc(formatMoneyShort(c.currency, c.total_recorded))} <small style="font-size:13px;font-weight:600;color:var(--text-muted)">${esc(c.currency)}</small></div>
            <div class="kpi-sub">${recurLine}</div>
          </div>
        `);
      }
    }

    const assetBits = [];
    if (ac.license > 0) assetBits.push(`买断 ${ac.license}`);
    if (ac.hardware > 0) assetBits.push(`硬件 ${ac.hardware}`);
    if (ac.reimbursable > 0) assetBits.push(`报销 ${ac.reimbursable}`);
    assetBits.push(`防重 ${v.duplicate_count} 次`);

    cards.push(`
      <div class="kpi-card">
        <div class="kpi-label">
          <span>原件防篡改档案</span>
          <span class="badge badge-slate">SHA-256</span>
        </div>
        <div class="kpi-value">${v.source_count} 份原件</div>
        <div class="kpi-sub">${assetBits.join(" · ")}</div>
      </div>
    `);

    grid.innerHTML = cards.join("");
  }

  function syncFilterDropdowns(ws) {
    const accSel = $("filter-account");
    accSel.innerHTML = `<option value="">所有账号</option>` +
      (ws.accounts || []).map((a) => `<option value="${esc(a)}">${esc(a)}</option>`).join("");
    accSel.value = state.accountFilter;

    const curSel = $("filter-currency");
    const curs = (ws.summary_by_currency || []).map((c) => c.currency);
    curSel.innerHTML = `<option value="">所有币种</option>` +
      curs.map((c) => `<option value="${esc(c)}">${esc(c)}</option>`).join("");
    curSel.value = state.currencyFilter;

    if (state.categoryFilter) {
      $("active-cat-label").textContent = `分类：${state.categoryFilter}`;
      $("active-cat-chip").classList.remove("hidden");
    } else {
      $("active-cat-chip").classList.add("hidden");
    }
  }

  function filteredItems(ws) {
    const q = state.searchQuery.trim().toLowerCase();
    return (ws.items || []).filter((item) => {
      const ann = item.annotation || {};
      const amtVal = item.amount_fact ? item.amount_fact.value : null;
      const flags = item.status_flags || {};

      const kind = ann.asset_kind || "general";
      if (state.ledgerFilter === "subscription") {
        if (kind !== "subscription") return false;
      } else if (state.ledgerFilter === "license") {
        if (kind !== "license") return false;
      } else if (state.ledgerFilter === "hardware") {
        if (kind !== "hardware") return false;
      } else if (state.ledgerFilter === "reimbursable") {
        if (kind !== "reimbursable" && (!ann.reimburse_status || ann.reimburse_status === "none")) return false;
      } else if (state.ledgerFilter === "needs") {
        if (!flags.needs_manual_entry && !flags.has_conflict && !flags.is_credit_or_order_warning) return false;
      } else if (state.ledgerFilter === "csv") {
        if (!flags.is_csv_statement) return false;
      }

      if (state.accountFilter && (ann.account_label || "") !== state.accountFilter) return false;
      if (state.currencyFilter && (!amtVal || amtVal.currency !== state.currencyFilter)) return false;
      if (state.categoryFilter) {
        const cat = ann.category || "未分类";
        if (cat !== state.categoryFilter) return false;
      }

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
          item.content_sha256,
          amtVal ? `${amtVal.currency} ${amtVal.amount_text}` : "",
        ]
          .filter(Boolean)
          .join(" ")
          .toLowerCase();
        if (!hay.includes(q)) return false;
      }
      return true;
    });
  }

  // --- Ledger Stage: Cards / Table / Calendar ---
  function renderLedgerStage(ws) {
    const stage = $("ledger-stage");
    document.querySelectorAll(".vsw-btn").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-view-mode") === state.viewMode);
    });

    if (state.viewMode === "calendar") {
      renderCalendarView(ws, stage);
      return;
    }

    const list = filteredItems(ws);
    if (list.length === 0) {
      const isEmptyVault = (ws.items || []).length === 0;
      stage.innerHTML = `
        <div class="card empty-state">
          <h4>${isEmptyVault ? "这座票据馆还是空的" : "没有符合筛选条件的账单"}</h4>
          <p>${isEmptyVault ? "把你的 PDF 发票、邮件 HTML、支付宝/微信 CSV 或收据截图拖进窗口，或点上方「+ 手动记一笔」。" : "可以尝试清空搜索词或切换筛选条件。"}</p>
          ${
            isEmptyVault
              ? `<div style="margin-top:16px;display:flex;justify-content:center;gap:10px;flex-wrap:wrap">
                   <button type="button" id="btn-load-demo-pack" class="btn btn-primary btn-sm">一键载入体验资产包（订阅涨价 / 硬件保修 / 买断密钥 / 报销全开）</button>
                 </div>`
              : ""
          }
        </div>
      `;
      const demoBtn = $("btn-load-demo-pack");
      if (demoBtn) {
        demoBtn.addEventListener("click", async () => {
          demoBtn.disabled = true;
          demoBtn.textContent = "正在载入体验资产包…";
          try {
            const snap = await api("POST", "/api/import-demo-pack", {});
            state.workspace = snap;
            renderWorkspace();
            showToast("已载入体验资产包（含订阅涨价雷达、硬件保修日均成本、加密授权密钥与待报销凭证）！", "ok");
          } catch (err) {
            showToast(err.message, "error");
            demoBtn.disabled = false;
            demoBtn.textContent = "一键载入体验资产包（订阅涨价 / 硬件保修 / 买断密钥 / 报销全开）";
          }
        });
      }
      return;
    }

    if (state.viewMode === "cards") {
      renderCardsGallery(list, stage);
    } else {
      renderTableView(list, stage);
    }
  }

  function periodProgressInfo(perVal) {
    if (!perVal || !perVal.period_start || !perVal.period_end) return null;
    const s = new Date(`${perVal.period_start}T00:00:00`);
    const e = new Date(`${perVal.period_end}T00:00:00`);
    if (isNaN(s.getTime()) || isNaN(e.getTime()) || e <= s) return null;
    const now = new Date();
    const pct = Math.max(0, Math.min(100, Math.round(((now - s) / (e - s)) * 100)));
    const daysLeft = Math.ceil((e - now) / (1000 * 60 * 60 * 24));
    const statusLabel = daysLeft > 0 ? `距周期结束 ${daysLeft} 天` : `周期已于 ${perVal.period_end} 届满`;
    return { pct, range: `${perVal.period_start} ~ ${perVal.period_end}`, statusLabel };
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
        const cycleText = CYCLE_LABELS[ann.billing_cycle] || "未标注";
        const drift = item.price_drift || null;

        let driftBadge = "";
        if (drift) {
          const isUp = drift.direction === "up";
          const pct = Math.abs(drift.pct_change || 0);
          driftBadge = `<span class="badge ${isUp ? "badge-danger" : "badge-emerald"}" title="对比上期 ${esc(drift.prev_text)}（${esc(drift.prev_date || "")}）变动 ${esc(drift.diff_text)}">${isUp ? `↑ 涨价 +${pct}%` : `↓ 降价 -${pct}%`}</span>`;
        }

        let priceBlock = "";
        if (amtVal && amtVal.amount_text) {
          priceBlock = `
            <div style="display:flex;align-items:baseline;gap:8px;flex-wrap:wrap">
              <span class="bill-card-price sensitive">${esc(formatMoneyShort(amtVal.currency, amtVal.amount_text))} <small style="font-size:12px;color:var(--text-muted)">${esc(amtVal.currency)}</small></span>
              ${driftBadge}
            </div>
            <span class="bill-card-cycle">${esc(kind === "hardware" ? "一次性购入" : kind === "license" ? "买断授权" : cycleText)}</span>
          `;
        } else if (flags.is_csv_statement) {
          priceBlock = `<span class="badge badge-slate">CSV 流水 · ${item.csv_row_count} 条可挑拣</span><span class="bill-card-cycle">点击展开</span>`;
        } else if (flags.is_credit_or_order_warning) {
          priceBlock = `<span class="badge badge-amber">退款/未付订单</span><span class="bill-card-cycle">未计入</span>`;
        } else {
          priceBlock = `<span class="amount-missing">点击补录金额 →</span><span class="bill-card-cycle">待整理</span>`;
        }

        // Progressive disclosure context block per asset_kind
        let contextHtml = "";
        if (kind === "hardware") {
          const w = warrantyInfo(ann.bill_date, ann.warranty_end);
          const cpd = amtVal ? costPerDayInfo(ann.bill_date, amtVal.currency, amtVal.amount_text) : null;
          const parts = [];
          if (w) {
            parts.push(`
              <div class="period-bar-wrap">
                <div class="period-bar-labels">
                  <span>🛡️ 保修至 ${esc(w.warrantyEnd)}</span>
                  <span>${esc(w.statusLabel)}</span>
                </div>
                <div class="period-bar-track"><div class="period-bar-fill warranty-fill ${w.level}" style="width:${w.pct}%"></div></div>
              </div>
            `);
          }
          if (cpd) {
            parts.push(`
              <div class="asset-meta-strip">
                <span>已陪伴 ${cpd.daysUsed} 天</span>
                <span class="cpd-pill sensitive">日均 ${esc(cpd.perDayText)}/天</span>
              </div>
            `);
          }
          contextHtml = parts.join("");
        } else if (kind === "license") {
          const parts = [];
          if (ann.license_scope) {
            parts.push(`<div class="asset-meta-strip"><span>🔑 ${esc(ann.license_scope)}</span></div>`);
          }
          if (ann.license_key) {
            parts.push(`
              <div class="asset-key-bar">
                <span class="sensitive">${esc(maskLicenseKey(ann.license_key))}</span>
                <button type="button" class="asset-key-copy" data-copy-key-sid="${esc(item.source_id)}">复制密钥</button>
              </div>
            `);
          }
          contextHtml = parts.join("");
        } else {
          const prog = periodProgressInfo(perVal);
          if (prog) {
            contextHtml = `
              <div class="period-bar-wrap">
                <div class="period-bar-labels"><span>${esc(prog.range)}</span><span>${esc(prog.statusLabel)}</span></div>
                <div class="period-bar-track"><div class="period-bar-fill" style="width:${prog.pct}%"></div></div>
              </div>
            `;
          } else if (ann.bill_date) {
            contextHtml = `<div style="font-size:11.5px;color:var(--text-muted)">账单日期：${esc(ann.bill_date)}</div>`;
          }
        }

        const badges = [];
        if (ann.account_label) badges.push(`<span class="badge badge-slate sensitive">${esc(ann.account_label)}</span>`);
        if (ann.category) badges.push(`<span class="badge badge-slate">${esc(ann.category)}</span>`);
        if (flags.has_duplicate) badges.push(`<span class="badge badge-emerald">防重 ×${item.occurrence_count}</span>`);
        if (flags.has_conflict) badges.push(`<span class="badge badge-amber">金额冲突</span>`);

        const reimbStatus = ann.reimburse_status && ann.reimburse_status !== "none"
          ? ann.reimburse_status
          : kind === "reimbursable"
            ? "pending"
            : null;
        const reimbBtnHtml = reimbStatus && REIMBURSE_META[reimbStatus]
          ? `<button type="button" class="reimb-toggle-btn ${REIMBURSE_META[reimbStatus].cls}" data-reimb-sid="${esc(item.source_id)}" data-reimb-cur="${esc(reimbStatus)}" title="点击一键切换报销状态">🧾 ${REIMBURSE_META[reimbStatus].label} ↻</button>`
          : "";

        const topBadge = kind !== "general" && kind !== "subscription"
          ? `<span class="badge ${kindMeta.badgeClass}">${esc(kindMeta.label)}</span>`
          : item.object_intact
            ? `<span class="badge badge-emerald" title="原件 SHA-256 核验完整">✓ 原件</span>`
            : "";

        return `
          <article class="bill-card" data-source-id="${esc(item.source_id)}">
            <div class="bill-card-top">
              <div class="bill-card-id">
                <div class="brand-avatar" style="background:${brand.bg}">${esc(brand.mark)}</div>
                <div style="min-width:0">
                  <div class="bill-card-title">${esc(title)}</div>
                  <div class="bill-card-sub">${esc(sub)}</div>
                </div>
              </div>
              ${topBadge}
            </div>
            <div class="bill-card-mid">${priceBlock}</div>
            ${contextHtml}
            <div class="bill-card-foot">
              <div class="tags-cell">${badges.join("") || `<span class="badge badge-slate">未分组</span>`}</div>
              ${reimbBtnHtml}
            </div>
          </article>
        `;
      })
      .join("");

    stage.innerHTML = `<div class="bill-gallery">${html}</div>`;
    stage.querySelectorAll("[data-copy-key-sid]").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        const sid = btn.getAttribute("data-copy-key-sid");
        const found = (state.workspace.items || []).find((i) => i.source_id === sid);
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
    stage.querySelectorAll(".bill-card[data-source-id]").forEach((card) => {
      card.addEventListener("click", () => {
        const sid = card.getAttribute("data-source-id");
        const found = (state.workspace.items || []).find((i) => i.source_id === sid);
        if (found) openDrawer(found);
      });
    });
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
        const subtitle = [ann.plan_name, ann.vendor && ann.vendor !== item.display_name ? item.display_name : ""].filter(Boolean).join(" · ");

        let amountHtml = "";
        if (amtVal && amtVal.amount_text) {
          const drift = item.price_drift;
          const driftTag = drift
            ? ` <span class="badge ${drift.direction === "up" ? "badge-danger" : "badge-emerald"}">${drift.direction === "up" ? `↑ +${Math.abs(drift.pct_change)}%` : `↓ -${Math.abs(drift.pct_change)}%`}</span>`
            : "";
          amountHtml = `<span class="amount-cell sensitive">${esc(formatMoney(amtVal.currency, amtVal.amount_text))}</span>${driftTag}`;
        } else if (flags.is_csv_statement) {
          amountHtml = `<span class="badge badge-slate">CSV 流水（${item.csv_row_count} 条）</span>`;
        } else if (flags.is_credit_or_order_warning) {
          amountHtml = `<span class="badge badge-amber">退款/未付订单</span>`;
        } else {
          amountHtml = `<span class="amount-missing">点击补录 →</span>`;
        }

        let dateText = perVal && perVal.period_start
          ? `${perVal.period_start} ~ ${perVal.period_end}`
          : ann.bill_date || "—";
        if (kind === "hardware" && ann.warranty_end) {
          dateText = `保修至 ${ann.warranty_end}`;
        }

        const badges = [`<span class="badge ${kindMeta.badgeClass}">${esc(kindMeta.label)}</span>`];
        if (kind === "license" && ann.license_key) {
          badges.push(`<button type="button" class="asset-key-copy" data-copy-key-sid="${esc(item.source_id)}">复制密钥</button>`);
        }
        const reimbStatus = ann.reimburse_status && ann.reimburse_status !== "none"
          ? ann.reimburse_status
          : kind === "reimbursable"
            ? "pending"
            : null;
        if (reimbStatus && REIMBURSE_META[reimbStatus]) {
          badges.push(`<button type="button" class="reimb-toggle-btn ${REIMBURSE_META[reimbStatus].cls}" data-reimb-sid="${esc(item.source_id)}" data-reimb-cur="${esc(reimbStatus)}">${REIMBURSE_META[reimbStatus].label} ↻</button>`);
        }
        if (flags.has_duplicate) badges.push(`<span class="badge badge-slate">去重 ×${item.occurrence_count}</span>`);
        if (flags.has_conflict) badges.push(`<span class="badge badge-amber">金额冲突</span>`);
        if (ann.category) badges.push(`<span class="badge badge-slate">${esc(ann.category)}</span>`);

        return `
          <tr data-source-id="${esc(item.source_id)}">
            <td>
              <div class="vendor-cell-flex">
                <div class="brand-avatar brand-avatar-sm" style="background:${brand.bg}">${esc(brand.mark)}</div>
                <div>
                  <strong>${esc(title)}</strong>
                  ${subtitle ? `<small>${esc(subtitle)}</small>` : ""}
                </div>
              </div>
            </td>
            <td class="sensitive">${esc(ann.account_label || "未分组")}</td>
            <td>${esc(CYCLE_LABELS[ann.billing_cycle] || "未标注")}</td>
            <td>${amountHtml}</td>
            <td style="white-space:nowrap;font-size:12px;color:var(--text-secondary)">${esc(dateText)}</td>
            <td><div class="tags-cell">${badges.join("")}</div></td>
          </tr>
        `;
      })
      .join("");

    stage.innerHTML = `
      <div class="card table-card">
        <table class="ledger-table">
          <thead>
            <tr>
              <th>资产 / 商家</th>
              <th>归属账号</th>
              <th>周期</th>
              <th>金额</th>
              <th>日期 / 保修截止</th>
              <th>类型与状态</th>
            </tr>
          </thead>
          <tbody>${rowsHtml}</tbody>
        </table>
      </div>
    `;
    stage.querySelectorAll("[data-copy-key-sid]").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        const sid = btn.getAttribute("data-copy-key-sid");
        const found = (state.workspace.items || []).find((i) => i.source_id === sid);
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
    stage.querySelectorAll("tbody tr[data-source-id]").forEach((tr) => {
      tr.addEventListener("click", () => {
        const sid = tr.getAttribute("data-source-id");
        const found = (state.workspace.items || []).find((i) => i.source_id === sid);
        if (found) openDrawer(found);
      });
    });
  }

  function renderCalendarView(ws, stage) {
    const year = state.calendarYear;
    const month = state.calendarMonth; // 0-11
    const firstDay = new Date(year, month, 1);
    const startWeekday = firstDay.getDay(); // 0=Sun
    const daysInMonth = new Date(year, month + 1, 0).getDate();
    const prevMonthDays = new Date(year, month, 0).getDate();
    const today = new Date();

    // Collect real bill events + projected recurring renewals for this month
    const eventsByDay = {};
    const addEvt = (day, evt) => {
      if (!eventsByDay[day]) eventsByDay[day] = [];
      eventsByDay[day].push(evt);
    };

    const items = filteredItems(ws);
    for (const item of items) {
      const ann = item.annotation || {};
      const amtVal = item.amount_fact ? item.amount_fact.value : null;
      const perVal = item.period_fact ? item.period_fact.value : null;
      const title = ann.vendor || item.display_name;

      // Hardware warranty expiry event
      if (ann.warranty_end && /^\d{4}-\d{2}-\d{2}$/.test(ann.warranty_end)) {
        const [wy, wm, wd] = ann.warranty_end.split("-").map(Number);
        if (wy === year && wm === month + 1) {
          addEvt(wd, { kind: "warranty", source_id: item.source_id, title: `🛡️ ${title}`, money: "保修截止" });
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

      // Projected recurring date in viewed month (strictly marked as estimated)
      const anchorStr = (perVal && perVal.period_end) || realDateStr;
      if (anchorStr && /^\d{4}-\d{2}-\d{2}$/.test(anchorStr)) {
        const [ay, am, ad] = anchorStr.split("-").map(Number);
        const anchorYm = ay * 12 + (am - 1);
        const viewYm = year * 12 + month;
        const diff = viewYm - anchorYm;
        const clampedDay = Math.min(ad, daysInMonth);
        if (diff >= 0 && clampedDay !== matchedRealDay) {
          let hits = false;
          if (ann.billing_cycle === "monthly") hits = true;
          else if (ann.billing_cycle === "quarterly" && diff % 3 === 0) hits = true;
          else if (ann.billing_cycle === "yearly" && diff % 12 === 0) hits = true;
          if (hits && !(diff === 0 && perVal && perVal.period_end && matchedRealDay === clampedDay)) {
            addEvt(clampedDay, { kind: "projected", source_id: item.source_id, title, money });
          }
        }
      }
    }

    const weekdays = ["日", "一", "二", "三", "四", "五", "六"];
    let cellsHtml = weekdays.map((w) => `<div class="cal-weekday">周${w}</div>`).join("");

    // Leading days from previous month
    for (let i = startWeekday - 1; i >= 0; i--) {
      cellsHtml += `<div class="cal-cell other-month"><div class="cal-day-num">${prevMonthDays - i}</div></div>`;
    }

    for (let d = 1; d <= daysInMonth; d++) {
      const isToday = today.getFullYear() === year && today.getMonth() === month && today.getDate() === d;
      const evts = eventsByDay[d] || [];
      const pills = evts
        .map(
          (ev) => `
            <button type="button" class="cal-pill ${ev.kind}" data-cal-sid="${esc(ev.source_id)}" title="${ev.kind === "warranty" ? "硬件保修截止日" : ev.kind === "real" ? "真实历史账单" : "按周期推算的预计续费（仅供参考）"}：${esc(ev.title)} ${esc(ev.money)}">
              <span class="cal-pill-name">${esc(ev.title)}</span>
              <span class="${ev.kind === "warranty" ? "" : "sensitive"}">${esc(ev.money)}</span>
            </button>
          `
        )
        .join("");
      cellsHtml += `
        <div class="cal-cell ${isToday ? "is-today" : ""}">
          <div class="cal-day-num">${d}${isToday ? " · 今天" : ""}</div>
          ${pills}
        </div>
      `;
    }

    stage.innerHTML = `
      <div class="card calendar-card">
        <div class="cal-header">
          <div class="cal-nav">
            <button type="button" id="cal-prev" class="btn btn-outline btn-sm">← 上月</button>
            <h3 class="cal-month-title">${year} 年 ${month + 1} 月</h3>
            <button type="button" id="cal-next" class="btn btn-outline btn-sm">下月 →</button>
            <button type="button" id="cal-today" class="btn btn-ghost btn-sm">回到本月</button>
          </div>
          <div class="cal-legend">
            <span><i class="cal-dot-solid"></i>真实账单日</span>
            <span><i class="cal-dot-dashed"></i>按周期推算续费</span>
            <span><i class="cal-dot-violet"></i>硬件保修截止</span>
          </div>
        </div>
        <div class="cal-grid">${cellsHtml}</div>
      </div>
    `;

    $("cal-prev").addEventListener("click", () => {
      state.calendarMonth -= 1;
      if (state.calendarMonth < 0) { state.calendarMonth = 11; state.calendarYear -= 1; }
      renderCalendarView(ws, stage);
    });
    $("cal-next").addEventListener("click", () => {
      state.calendarMonth += 1;
      if (state.calendarMonth > 11) { state.calendarMonth = 0; state.calendarYear += 1; }
      renderCalendarView(ws, stage);
    });
    $("cal-today").addEventListener("click", () => {
      const n = new Date();
      state.calendarYear = n.getFullYear();
      state.calendarMonth = n.getMonth();
      renderCalendarView(ws, stage);
    });
    stage.querySelectorAll("[data-cal-sid]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const sid = btn.getAttribute("data-cal-sid");
        const found = (ws.items || []).find((i) => i.source_id === sid);
        if (found) openDrawer(found);
      });
    });
  }

  // --- View 2: Insights Charts & What-If Savings Simulator ---
  function renderInsightsPane(ws) {
    const pane = $("pane-insights");
    const curList = (ws.summary_by_currency || []).map((c) => c.currency);
    if (curList.length === 0) {
      pane.innerHTML = `
        <div class="card empty-state">
          <h4>导入或记入几笔账单后，这里会自动生成可视化图表与退订省钱沙盘</h4>
          <p>支持按分类看占比、看月度支出节奏、区分个人/工作账号，以及模拟取消订阅一年能省多少钱。</p>
        </div>
      `;
      return;
    }

    if (!curList.includes(state.insightsCurrency)) {
      state.insightsCurrency = curList[0];
    }
    const cur = state.insightsCurrency;
    const analytics = ws.analytics || { categories: {}, accounts: {}, monthly_trend: {} };
    const catRows = (analytics.categories && analytics.categories[cur]) || [];
    const accRows = (analytics.accounts && analytics.accounts[cur]) || [];
    const monthRows = (analytics.monthly_trend && analytics.monthly_trend[cur]) || [];

    // 1. SVG Donut chart for categories
    const totalCatCents = catRows.reduce((s, r) => s + (r.cents || 0), 0) || 1;
    const radius = 68;
    const circ = 2 * Math.PI * radius;
    let offset = 0;
    const slicesSvg = catRows
      .map((row, idx) => {
        const frac = row.cents / totalCatCents;
        const dash = Math.max(2, frac * circ);
        const color = CHART_PALETTE[idx % CHART_PALETTE.length];
        const svg = `<circle class="donut-slice" cx="95" cy="95" r="${radius}" fill="none" stroke="${color}" stroke-width="22" stroke-dasharray="${dash.toFixed(2)} ${(circ - dash).toFixed(2)}" stroke-dashoffset="${(-offset).toFixed(2)}" transform="rotate(-90 95 95)" data-cat-slice="${esc(row.name)}" data-cat-amt="${esc(row.amount_text)}" data-cat-pct="${Math.round(frac * 100)}" />`;
        offset += dash;
        return svg;
      })
      .join("");

    const legendHtml = catRows
      .map((row, idx) => {
        const pct = Math.round((row.cents / totalCatCents) * 100);
        const color = CHART_PALETTE[idx % CHART_PALETTE.length];
        return `
          <div class="legend-row" data-jump-cat="${esc(row.name)}">
            <div class="legend-left">
              <span class="legend-swatch" style="background:${color}"></span>
              <span>${esc(row.name)}（${row.count}）</span>
            </div>
            <strong class="sensitive">${esc(formatMoneyShort(cur, row.amount_text))} · ${pct}%</strong>
          </div>
        `;
      })
      .join("");

    // 2. Account split bar
    const totalAccCents = accRows.reduce((s, r) => s + (r.cents || 0), 0) || 1;
    const splitSegs = accRows
      .map((row, idx) => {
        const pct = Math.max(4, Math.round((row.cents / totalAccCents) * 100));
        return `<div class="split-seg" style="width:${pct}%;background:${CHART_PALETTE[(idx + 2) % CHART_PALETTE.length]}" title="${esc(row.name)}: ${esc(formatMoney(cur, row.amount_text))}"></div>`;
      })
      .join("");
    const accLegend = accRows
      .map((row, idx) => {
        const pct = Math.round((row.cents / totalAccCents) * 100);
        return `
          <div class="legend-row" data-jump-acc="${esc(row.name)}">
            <div class="legend-left">
              <span class="legend-swatch" style="background:${CHART_PALETTE[(idx + 2) % CHART_PALETTE.length]}"></span>
              <span class="sensitive">${esc(row.name)}（${row.count} 笔）</span>
            </div>
            <strong class="sensitive">${esc(formatMoneyShort(cur, row.amount_text))} · ${pct}%</strong>
          </div>
        `;
      })
      .join("");

    // 3. Monthly spend rhythm bar chart
    const maxMonthCents = Math.max(...monthRows.map((m) => m.cents || 0), 1);
    const barsHtml = monthRows.length === 0
      ? `<p class="muted-note">给账单补充日期后，这里会绘出按月花费节奏图。</p>`
      : `<div class="bar-chart">
          ${monthRows
            .slice(-12)
            .map((m) => {
              const h = Math.max(8, Math.round((m.cents / maxMonthCents) * 118));
              return `
                <div class="bar-col">
                  <span class="bar-val sensitive">${esc(formatMoneyShort(cur, m.amount_text))}</span>
                  <div class="bar-rect" style="height:${h}px"></div>
                  <span class="bar-label">${esc(m.month)}</span>
                </div>
              `;
            })
            .join("")}
        </div>`;

    // 4. What-If Savings Simulator (recurring subscriptions in this currency)
    const recurringItems = (ws.items || []).filter((item) => {
      const amt = item.amount_fact?.value;
      const cyc = item.annotation?.billing_cycle;
      return amt && amt.currency === cur && ["monthly", "yearly", "quarterly", "weekly"].includes(cyc);
    });

    let yearlySaveCents = 0;
    const cutNames = [];
    const simCardsHtml = recurringItems
      .map((item) => {
        const ann = item.annotation || {};
        const amt = item.amount_fact.value;
        const cents = amountToCents(amt.amount_text);
        const mult = { monthly: 12, yearly: 1, quarterly: 4, weekly: 52 }[ann.billing_cycle] || 0;
        const yrCents = cents * mult;
        const isCut = state.simCutIds.has(item.source_id);
        if (isCut) {
          yearlySaveCents += yrCents;
          cutNames.push(`${ann.vendor || item.display_name}（${formatMoney(cur, amt.amount_text)} / ${CYCLE_LABELS[ann.billing_cycle]}）`);
        }
        return `
          <div class="sim-item ${isCut ? "cut" : ""}">
            <div style="min-width:0">
              <div class="sim-item-title" style="font-weight:700;font-size:13.5px">${esc(ann.vendor || item.display_name)}</div>
              <div style="font-size:12px;color:var(--text-muted)" class="sensitive">${esc(formatMoneyShort(cur, amt.amount_text))} / ${esc(CYCLE_LABELS[ann.billing_cycle])} · 年合 ${esc(formatMoneyShort(cur, centsToText(yrCents)))}</div>
            </div>
            <label class="switch" title="模拟停掉此订阅">
              <input type="checkbox" data-sim-sid="${esc(item.source_id)}" ${isCut ? "checked" : ""}>
              <span class="slider"></span>
            </label>
          </div>
        `;
      })
      .join("");

    const equivLine = buildFunEquivalence(cur, yearlySaveCents);
    const curPills = curList
      .map((c) => `<button type="button" class="pill ${c === cur ? "active" : ""}" data-ins-cur="${esc(c)}">${esc(c)}</button>`)
      .join("");

    // 5. Vendor Asset Portfolio (LTV + sparkline + price drift)
    const portfolioRows = (ws.vendor_portfolio || []).filter((p) => p.currency === cur);
    const portfolioTilesHtml = portfolioRows
      .map((vp) => {
        const brand = brandStyleFor(vp.vendor);
        const kindMeta = ASSET_KIND_META[vp.asset_kind] || ASSET_KIND_META.general;
        const drift = vp.price_drift;
        const driftTag = drift
          ? `<span class="badge ${drift.direction === "up" ? "badge-danger" : "badge-emerald"}">${drift.direction === "up" ? `↑ +${Math.abs(drift.pct_change)}%` : `↓ -${Math.abs(drift.pct_change)}%`}</span>`
          : `<span class="badge ${kindMeta.badgeClass}">${esc(kindMeta.label)}</span>`;
        const spark = buildMiniSparkline(vp.history || [], drift?.direction === "up" ? "#be123c" : "#0f6b58");
        const accStr = (vp.accounts || []).length > 0 ? ` · ${vp.accounts.join(" / ")}` : "";
        return `
          <div class="vendor-tile" data-jump-vendor="${esc(vp.vendor)}">
            <div class="vendor-tile-head">
              <div class="vendor-tile-id">
                <div class="brand-avatar brand-avatar-sm" style="background:${brand.bg}">${esc(brand.mark)}</div>
                <div style="min-width:0">
                  <div class="vendor-tile-name">${esc(vp.vendor)}</div>
                  <div class="vendor-tile-sub sensitive">${vp.bill_count} 笔账单${esc(accStr)}</div>
                </div>
              </div>
              ${driftTag}
            </div>
            <div class="vendor-tile-foot">
              <div>
                <div style="font-size:11px;color:var(--text-muted)">历史累计投入</div>
                <div class="vendor-tile-ltv sensitive">${esc(formatMoneyShort(cur, vp.total_text))}</div>
              </div>
              ${spark}
            </div>
          </div>
        `;
      })
      .join("");

    const portfolioSection = portfolioRows.length > 0
      ? `
        <div class="card portfolio-card">
          <h3 style="margin:0 0 4px;font-size:15px">🏪 商家资产档案（累计投入与价格轨迹）</h3>
          <p class="chart-sub" style="margin-bottom:14px">自动归集同一商家的多期账单，一眼看出长期总花费与涨价趋势；点击卡片直接筛选该商家全部凭证</p>
          <div class="portfolio-grid">${portfolioTilesHtml}</div>
        </div>
      `
      : "";

    pane.innerHTML = `
      <div class="insights-wrap">
        <div class="insights-top-bar">
          <div class="filter-group">${curPills}</div>
          <span class="muted-note" style="margin:0">按币种独立分析，绝不做虚假汇率换算</span>
        </div>

        <div class="insights-grid">
          <div class="card chart-card">
            <h3>分类花费占比（点击扇区直接筛选账单）</h3>
            <p class="chart-sub">${esc(cur)} 全部已入库账单分布</p>
            <div class="donut-layout">
              <div class="donut-svg-wrap">
                <svg viewBox="0 0 190 190" width="190" height="190">${slicesSvg}</svg>
                <div class="donut-center">
                  <div id="donut-center-label" class="donut-center-label">${esc(cur)} 总额</div>
                  <div id="donut-center-val" class="donut-center-val sensitive">${esc(formatMoneyShort(cur, centsToText(totalCatCents)))}</div>
                </div>
              </div>
              <div class="donut-legend">${legendHtml}</div>
            </div>
          </div>

          <div class="card chart-card">
            <h3>月度账单节奏 &amp; 账号分账</h3>
            <p class="chart-sub">按真实账单日期分布，快速区分个人自费与公司工作</p>
            ${barsHtml}
            <div style="margin-top:16px">
              <div class="split-bar">${splitSegs}</div>
              ${accLegend}
            </div>
          </div>
        </div>

        ${portfolioSection}

        <div class="card sim-card">
          <div class="sim-hero">
            <div>
              <div style="display:flex;align-items:center;gap:8px;margin-bottom:4px">
                <h3 style="margin:0;font-size:17px">✂️ 退订省钱沙盘（What-If 模拟器）</h3>
                <span class="badge badge-slate">纯模拟 · 不改真实账本</span>
              </div>
              <div class="sim-savings-num sensitive">每年可省 ${esc(formatMoney(cur, centsToText(yearlySaveCents)))}</div>
              <div class="sim-equiv">${esc(equivLine)}</div>
            </div>
            <div style="display:flex;gap:8px;flex-wrap:wrap">
              <button type="button" id="btn-sim-copy" class="btn btn-primary btn-sm" ${cutNames.length === 0 ? "disabled" : ""}>复制待精简清单 (${cutNames.length})</button>
              <button type="button" id="btn-sim-reset" class="btn btn-outline btn-sm" ${cutNames.length === 0 ? "disabled" : ""}>重置模拟</button>
            </div>
          </div>
          ${
            recurringItems.length > 0
              ? `<div class="sim-list">${simCardsHtml}</div>`
              : `<p class="muted-note" style="margin:0">当前 ${esc(cur)} 账单里还没有标为月付/年付/季付的周期订阅。点开账单设好计费周期即可在这里玩省钱模拟！</p>`
          }
        </div>
      </div>
    `;

    // Bind currency switch
    pane.querySelectorAll("[data-ins-cur]").forEach((btn) => {
      btn.addEventListener("click", () => {
        state.insightsCurrency = btn.getAttribute("data-ins-cur");
        renderInsightsPane(ws);
      });
    });

    // Donut hover & click-to-filter
    pane.querySelectorAll("[data-cat-slice]").forEach((circle) => {
      circle.addEventListener("mouseenter", () => {
        $("donut-center-label").textContent = `${circle.getAttribute("data-cat-slice")} (${circle.getAttribute("data-cat-pct")}%)`;
        $("donut-center-val").textContent = formatMoneyShort(cur, circle.getAttribute("data-cat-amt"));
      });
      circle.addEventListener("mouseleave", () => {
        $("donut-center-label").textContent = `${cur} 总额`;
        $("donut-center-val").textContent = formatMoneyShort(cur, centsToText(totalCatCents));
      });
      circle.addEventListener("click", () => {
        state.categoryFilter = circle.getAttribute("data-cat-slice");
        state.currencyFilter = cur;
        switchMainTab("ledger");
        renderWorkspace();
        showToast(`已筛选分类：${state.categoryFilter}`);
      });
    });
    pane.querySelectorAll("[data-jump-cat]").forEach((row) => {
      row.addEventListener("click", () => {
        state.categoryFilter = row.getAttribute("data-jump-cat");
        state.currencyFilter = cur;
        switchMainTab("ledger");
        renderWorkspace();
      });
    });
    pane.querySelectorAll("[data-jump-acc]").forEach((row) => {
      row.addEventListener("click", () => {
        const acc = row.getAttribute("data-jump-acc");
        state.accountFilter = acc === "未分组" ? "" : acc;
        switchMainTab("ledger");
        renderWorkspace();
      });
    });
    pane.querySelectorAll("[data-jump-vendor]").forEach((tile) => {
      tile.addEventListener("click", () => {
        const v = tile.getAttribute("data-jump-vendor") || "";
        state.searchQuery = v;
        $("search-box").value = v;
        setLedgerFilter("all");
        switchMainTab("ledger");
        renderWorkspace();
        showToast(`已筛选商家：${v}`);
      });
    });

    // Simulator toggles
    pane.querySelectorAll("[data-sim-sid]").forEach((chk) => {
      chk.addEventListener("change", () => {
        const sid = chk.getAttribute("data-sim-sid");
        if (chk.checked) state.simCutIds.add(sid);
        else state.simCutIds.delete(sid);
        renderInsightsPane(ws);
      });
    });
    const resetBtn = $("btn-sim-reset");
    if (resetBtn) {
      resetBtn.addEventListener("click", () => {
        state.simCutIds.clear();
        renderInsightsPane(ws);
      });
    }
    const copyBtn = $("btn-sim-copy");
    if (copyBtn) {
      copyBtn.addEventListener("click", async () => {
        const text = `【SubVault 待精简订阅清单（${cur}）】\n预计每年可省：${formatMoney(cur, centsToText(yearlySaveCents))}\n` +
          cutNames.map((n, i) => `${i + 1}. ${n}`).join("\n");
        try {
          await navigator.clipboard.writeText(text);
          showToast("待精简清单已复制到剪贴板！", "ok");
        } catch (_) {
          downloadTextFile("subvault-cancel-plan.txt", text);
        }
      });
    }
  }

  function buildMiniSparkline(history, strokeColor) {
    if (!history || history.length < 2) {
      return `<span style="font-size:11px;color:var(--text-muted)">单期记录</span>`;
    }
    const vals = history.slice(-8).map((h) => h.cents || 0);
    const min = Math.min(...vals);
    const max = Math.max(...vals);
    const span = max - min || 1;
    const w = 68;
    const h = 24;
    const pts = vals
      .map((v, i) => {
        const x = vals.length === 1 ? w / 2 : (i / (vals.length - 1)) * w;
        const y = h - 3 - ((v - min) / span) * (h - 6);
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
    const lastPt = pts.split(" ").pop().split(",");
    return `
      <svg class="sparkline-svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" aria-label="价格变化轨迹">
        <polyline fill="none" stroke="${strokeColor}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" points="${pts}" />
        <circle cx="${lastPt[0]}" cy="${lastPt[1]}" r="3" fill="${strokeColor}" />
      </svg>
    `;
  }

  function buildFunEquivalence(cur, cents) {
    if (cents <= 0) return "拨动下方任意订阅开关，立刻测算停掉后一年能省下多少预算！";
    const val = cents / 100;
    if (cur === "CNY") {
      const coffees = Math.max(1, Math.round(val / 30));
      const hotpots = Math.round(val / 180);
      return hotpots >= 2
        ? `相当于省下约 ${coffees} 杯精品咖啡，或 ${hotpots} 顿双人火锅！`
        : `相当于省下约 ${coffees} 杯精品咖啡！`;
    }
    if (cur === "USD" || cur === "EUR" || cur === "GBP") {
      const coffees = Math.max(1, Math.round(val / 5));
      const games = Math.round(val / 60);
      return games >= 1
        ? `大约相当于 ${coffees} 杯咖啡，或 ${games} 款 3A 大作游戏！`
        : `大约相当于 ${coffees} 杯精品咖啡！`;
    }
    return `一年可减少 ${formatMoney(cur, centsToText(cents))} 的周期固定支出。`;
  }

  // --- View 3: ⚡ Quick Triage Mode ---
  function getTriageQueue(ws) {
    return (ws.items || []).filter(
      (i) =>
        (i.status_flags.needs_manual_entry || i.status_flags.has_conflict) &&
        !state.triageSkippedIds.has(i.source_id)
    );
  }

  function renderTriagePane(ws) {
    const pane = $("pane-triage");
    const queue = getTriageQueue(ws);
    if (queue.length === 0) {
      const skipped = state.triageSkippedIds.size;
      pane.innerHTML = `
        <div class="card triage-done-card">
          <div class="triage-done-emoji">✨</div>
          <h3>太棒了，所有待补录和冲突账单都已整理干净！</h3>
          <p class="muted-note">下次拖入扫描件 PDF、收据截图或出现多金额冲突的账单时，可以随时回到这里极速过一遍。</p>
          ${
            skipped > 0
              ? `<button type="button" id="btn-reset-skip" class="btn btn-outline btn-sm">重新查看刚才跳过的 ${skipped} 项</button>`
              : `<button type="button" id="btn-back-ledger" class="btn btn-primary btn-sm">回到我的账单</button>`
          }
        </div>
      `;
      if ($("btn-reset-skip")) {
        $("btn-reset-skip").addEventListener("click", () => {
          state.triageSkippedIds.clear();
          renderTriagePane(ws);
        });
      }
      if ($("btn-back-ledger")) {
        $("btn-back-ledger").addEventListener("click", () => switchMainTab("ledger"));
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
      previewHtml = `<iframe sandbox="" src="${esc(rawUrl)}" title="安全静态 HTML"></iframe>`;
    } else {
      previewHtml = `<pre id="triage-text-preview" class="text-preview" style="margin:0;border:none">加载原件文本中…</pre>`;
    }

    const curs = (state.bootstrap?.currencies || ["CNY", "USD", "EUR", "GBP", "JPY", "HKD"])
      .map((c) => `<option value="${esc(c)}">${esc(c)} ${esc(CURRENCY_SYMBOLS[c] || "")}</option>`)
      .join("");

    pane.innerHTML = `
      <div class="card triage-progress-bar">
        <div>
          <strong>⚡ 极速整理模式</strong> · 剩余 <strong>${queue.length}</strong> 份待补录/核对
          <span class="muted-note" style="margin:0 0 0 10px">（填完按 Enter 保存并自动切下一张）</span>
        </div>
        <button type="button" id="btn-triage-skip" class="btn btn-ghost btn-sm">暂时跳过这张</button>
      </div>
      <div class="triage-shell">
        <div class="card triage-panel">
          <div style="font-size:12px;color:var(--text-muted);margin-bottom:10px">
            原件对照：<strong>${esc(item.display_name)}</strong>（${formatBytes(item.byte_length)}）
          </div>
          <div class="preview-box" style="margin-bottom:0">${previewHtml}</div>
        </div>
        <div class="card triage-panel">
          <h3 style="margin:0 0 12px;font-size:15px">快速登记这张账单</h3>
          ${
            item.status_flags.has_conflict
              ? `<div class="warning-banner" style="margin-bottom:12px">检测到多个候选金额，请对照左侧原件确认真实实付金额。</div>`
              : ""
          }
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
                <span>实付金额</span>
                <input id="tr-amount" type="text" placeholder="如 98.00" required>
              </label>
            </div>
            <div class="quick-chips-row">
              <span class="quick-chip-label">周期：</span>
              ${["monthly", "yearly", "quarterly", "one_time"]
                .map((c) => `<button type="button" class="qchip ${ann.billing_cycle === c ? "active" : ""}" data-tr-cycle="${c}">${CYCLE_LABELS[c]}</button>`)
                .join("")}
            </div>
            <div class="quick-chips-row">
              <span class="quick-chip-label">账号：</span>
              ${["个人", "公司工作", "家庭共享"]
                .map((a) => `<button type="button" class="qchip ${ann.account_label === a ? "active" : ""}" data-tr-acc="${a}">${a}</button>`)
                .join("")}
            </div>
            <div class="quick-chips-row">
              <span class="quick-chip-label">分类：</span>
              ${["AI 工具", "云存储", "开发工具", "办公效率", "影音娱乐"]
                .map((k) => `<button type="button" class="qchip ${ann.category === k ? "active" : ""}" data-tr-cat="${k}">${k}</button>`)
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
            <button id="btn-triage-save" class="btn btn-primary btn-block" type="submit">保存并进入下一张 ↵</button>
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
          showToast("🎉 全部待整理账单已清零！", "ok");
        } else {
          showToast("已保存，已切到下一张！", "ok");
        }
      } catch (err) {
        showToast(err.message, "error");
        btn.disabled = false;
        btn.textContent = "保存并进入下一张 ↵";
      }
    });

    $("tr-vendor").focus();
  }

  // --- View 4: Attention & Dedup ---
  function renderAttentionPane(ws) {
    const pane = $("pane-attention");
    const items = ws.items || [];
    const needs = items.filter(
      (i) => i.status_flags.needs_manual_entry || i.status_flags.has_conflict || i.status_flags.is_credit_or_order_warning
    );
    const dups = items.filter((i) => i.status_flags.has_duplicate);

    const needsHtml = needs.length === 0
      ? `<p class="muted-note">目前没有待补录或冲突账单，全部干干净净。</p>`
      : needs
          .map((item) => {
            const ann = item.annotation || {};
            const reason = item.status_flags.has_conflict
              ? "同一份文件出现多个不同金额，系统未盲猜，请点击核对"
              : item.status_flags.is_credit_or_order_warning
                ? "疑似退款单或未支付订单，系统未计入花费"
                : "未自动提取到金额（扫描件或截图），点击补录";
            return `
              <div class="attention-row">
                <div>
                  <strong>${esc(ann.vendor || item.display_name)}</strong>
                  <div style="font-size:12px;color:var(--text-muted)">${esc(reason)}</div>
                </div>
                <button type="button" class="btn btn-outline btn-sm" data-open-sid="${esc(item.source_id)}">去核对 / 补录</button>
              </div>
            `;
          })
          .join("");

    const dupsHtml = dups.length === 0
      ? `<p class="muted-note">暂未检测到重复导入的文件。以后哪怕把同一份账单改名再拖进来，系统也会按原件 SHA-256 自动去重。</p>`
      : dups
          .map((item) => {
            const names = (item.occurrences || []).map((o) => o.display_name).join("、");
            return `
              <div class="attention-row">
                <div>
                  <strong>${esc(item.annotation?.vendor || item.display_name)}</strong>
                  <div style="font-size:12px;color:var(--text-muted)">共导入 ${item.occurrence_count} 次（已拦截重复 ${item.duplicate_hits} 次，金额仅计入一次）· 文件名：${esc(names)}</div>
                </div>
                <button type="button" class="btn btn-outline btn-sm" data-open-sid="${esc(item.source_id)}">查看凭证</button>
              </div>
            `;
          })
          .join("");

    pane.innerHTML = `
      <div class="attention-section">
        <div class="card attention-card">
          <h3>待补录与冲突核对 <span class="badge badge-amber">${needs.length}</span></h3>
          ${needsHtml}
        </div>
        <div class="card attention-card">
          <h3>已自动拦截的重复账单 <span class="badge badge-emerald">${dups.length}</span></h3>
          ${dupsHtml}
        </div>
      </div>
    `;
    pane.querySelectorAll("[data-open-sid]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const found = items.find((i) => i.source_id === btn.getAttribute("data-open-sid"));
        if (found) openDrawer(found);
      });
    });
  }

  // --- Detail Drawer ---
  function openDrawer(item, defaultTab) {
    state.selectedSourceId = item.source_id;
    state.textViewActive = false;
    state.lastRedactedText = "";
    $("redact-result-wrap").classList.add("hidden");
    $("redact-custom-terms").value = "";
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
    if (tab === "evidence" && state.selectedSourceId) {
      loadEvidencePreview(state.selectedSourceId);
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
    $("drawer-stamp").textContent = item.object_intact ? "SHA-256 VERIFIED" : "CHECK ALERT";

    const warns = [];
    if (item.status_flags.has_conflict) warns.push("该文件包含多个不同的金额候选，系统未自动采纳。请核对原件后在下方保存正确金额。");
    if (item.status_flags.is_credit_or_order_warning) warns.push("疑似退款单、红字发票或未支付订单，未自动计入花费。");
    if (item.status_flags.needs_manual_entry) warns.push("尚未记录金额（常见于扫描版 PDF 或截图）。原件已安全加密留存，直接补充商家与金额即可。");
    $("drawer-warnings").innerHTML = warns.map((w) => `<div class="warning-banner">${esc(w)}</div>`).join("");

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
      <div><strong>原件 SHA-256：</strong><code>${esc(item.content_sha256)}</code></div>
      <div><strong>完整性状态：</strong>${item.object_intact ? "已实时解密核验一致，原件零改动" : "校验异常，请检查账本"}</div>
    `;
    $("btn-download-raw").href = tokenUrl(`/api/sources/${encodeURIComponent(item.source_id)}/raw?download=1`);

    $("evidence-occurrences").innerHTML = `
      <div style="font-size:12px;font-weight:600;color:var(--text-secondary);margin-bottom:4px">导入记录（共 ${item.occurrence_count} 次）</div>
      ${(item.occurrences || [])
        .map(
          (o) => `<div class="occ-item"><strong>${esc(o.display_name)}</strong> · ${formatTime(o.recorded_at)} ${
            o.storage_reused
              ? `<span class="badge badge-slate" style="margin-left:6px">重复自动复用</span>`
              : `<span class="badge badge-emerald" style="margin-left:6px">首次入库</span>`
          }</div>`
        )
        .join("")}
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
      box.innerHTML = `<iframe src="${esc(rawUrl)}" title="PDF 原件预览"></iframe>`;
      box.classList.remove("hidden");
    } else if (item.media_type.startsWith("image/")) {
      box.innerHTML = `<img src="${esc(rawUrl)}" alt="${esc(item.display_name)}">`;
      box.classList.remove("hidden");
    } else if (item.media_type.startsWith("text/html")) {
      box.innerHTML = `<iframe sandbox="" src="${esc(rawUrl)}" title="安全静态 HTML 预览"></iframe>`;
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

    container.innerHTML = rows
      .slice(0, 150)
      .map((r, idx) => {
        const dirBadge = r.direction === "expense"
          ? `<span class="badge badge-slate">支出</span>`
          : r.direction === "refund"
            ? `<span class="badge badge-amber">退款/关闭</span>`
            : `<span class="badge badge-emerald">收入/其他</span>`;
        return `
          <div class="csv-row-card">
            <div class="csv-row-info">
              <strong>${esc(r.merchant || "未标注商户")} · <span class="sensitive">${esc(formatMoney(r.currency || "CNY", r.amount_text))}</span></strong>
              <small>${esc(r.date || "")} · ${esc(r.description || "")} ${dirBadge}</small>
            </div>
            <button type="button" class="btn btn-outline btn-sm" data-csv-promote="${idx}">登记为账单</button>
          </div>
        `;
      })
      .join("");

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
          showToast(
            res.storage_reused ? "这条流水之前已登记过，已自动去重" : `已将「${r.merchant || "该笔流水"}」登记进主账本`,
            "ok"
          );
          await refreshWorkspace();
        } catch (err) {
          showToast(err.message, "error");
        } finally {
          btn.disabled = false;
          btn.textContent = "登记为账单";
        }
      });
    });
  }

  function renderHistoryTimeline(item) {
    const el = $("history-timeline");
    const blocks = [];
    for (const fact of [item.amount_fact, item.period_fact].filter(Boolean)) {
      const decisions = fact.decisions || [];
      const factLabel = fact.fact_type === "stated_amount_paid" ? "账单金额" : "服务周期";
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
          ? (val.amount_text ? formatMoney(val.currency || "USD", val.amount_text) : "已清空（待确认）")
          : (val.period_start ? `${val.period_start} ~ ${val.period_end}` : "已清空");
        const canUndo = isLatest && dec.kind !== "rule_adopt";
        blocks.push(`
          <div class="timeline-item">
            <div>
              <div><strong>${esc(factLabel)} · ${esc(kindLabel)}</strong> → <span class="sensitive">${esc(valText)}</span></div>
              <div style="font-size:12px;color:var(--text-muted)">版本 #${idx + 1} · ${formatTime(dec.recorded_at)}</div>
            </div>
            ${canUndo ? `<button type="button" class="btn btn-outline btn-sm" data-undo-dec="${esc(dec.decision_id)}" data-undo-rev="${fact.revision}">撤销此步</button>` : ""}
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
          showToast("已撤销上一步修改，完整历史已保留。", "ok");
        } catch (err) {
          showToast(err.message, "error");
        }
      });
    });
  }

  // --- Command Palette (Ctrl+K) ---
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
      { label: "＋ 手动记一笔资产 / 账单", sub: "订阅 / 买断密钥 / 硬件保修 / 报销", run: () => openManualModal() },
      { label: "📥 导入本地账单文件", sub: "PDF / HTML / CSV / 图片", run: () => $("file-picker-input").click() },
      { label: "🔑 查看买断授权与密钥库", sub: "一键复制加密保存的激活码", run: () => { switchMainTab("ledger"); setLedgerFilter("license"); renderLedgerStage(state.workspace); } },
      { label: "🛡️ 查看硬件保修与日均成本", sub: "保修倒计时 + 每日持有成本", run: () => { switchMainTab("ledger"); setLedgerFilter("hardware"); renderLedgerStage(state.workspace); } },
      { label: "🧾 查看报销凭证与待报销进度", sub: "一键切换待报销 / 已提交 / 已到账", run: () => { switchMainTab("ledger"); setLedgerFilter("reimbursable"); renderLedgerStage(state.workspace); } },
      { label: "⚡ 进入极速整理模式", sub: "左右分屏快填待补录项", run: () => switchMainTab("triage") },
      { label: "📊 打开洞察图表、商家档案与省钱沙盘", sub: "分类环图 + 商家涨价轨迹 + 省钱模拟器", run: () => switchMainTab("insights") },
      { label: "📅 切换到续费与保修日历", sub: "按月查看账单、续费与保修截止日", run: () => { switchMainTab("ledger"); state.viewMode = "calendar"; renderLedgerStage(state.workspace); } },
      { label: "🎴 切换到票据卡片画廊", sub: "带品牌徽标与上下文资产条", run: () => { switchMainTab("ledger"); state.viewMode = "cards"; renderLedgerStage(state.workspace); } },
      { label: "📑 切换到资产台账列表", sub: "高密度列表核对", run: () => { switchMainTab("ledger"); state.viewMode = "table"; renderLedgerStage(state.workspace); } },
      { label: "🎨 切换外观主题（纸感 / 深海暗黑 / 极简）", sub: `当前：${state.theme}`, run: () => cycleTheme() },
      { label: "👁 开关隐私防窥模糊模式", sub: "快捷键 P（同时模糊金额、账号与密钥）", run: () => togglePrivacy() },
      { label: "📊 立即下载 Excel 账单表 (.csv)", sub: "UTF-8 BOM 不乱码（不含敏感密钥）", run: () => { window.location.href = tokenUrl("/api/export/csv"); } },
      { label: "🔒 锁定当前账本", sub: "清空内存密钥", run: () => $("btn-lock-vault").click() },
    ];

    const billActions = (state.workspace?.items || []).map((item) => {
      const ann = item.annotation || {};
      const amt = item.amount_fact?.value;
      return {
        label: `🧾 ${ann.vendor || item.display_name}`,
        sub: [ann.plan_name, amt ? formatMoney(amt.currency, amt.amount_text) : "待补录", ann.account_label].filter(Boolean).join(" · "),
        run: () => openDrawer(item),
      };
    });

    const all = [...baseActions, ...billActions].filter(
      (a) => !q || a.label.toLowerCase().includes(q) || a.sub.toLowerCase().includes(q)
    );
    state.cmdActions = all.slice(0, 12);
    state.cmdIndex = Math.min(state.cmdIndex, Math.max(0, state.cmdActions.length - 1));

    const el = $("cmd-list");
    if (state.cmdActions.length === 0) {
      el.innerHTML = `<div style="padding:20px;text-align:center;color:var(--text-muted)">没有匹配的账单或快捷指令</div>`;
      return;
    }
    el.innerHTML = state.cmdActions
      .map(
        (act, idx) => `
          <button type="button" class="cmd-item ${idx === state.cmdIndex ? "selected" : ""}" data-cmd-idx="${idx}">
            <span>${esc(act.label)}</span>
            <span class="cmd-item-sub">${esc(act.sub)}</span>
          </button>
        `
      )
      .join("");
    el.querySelectorAll("[data-cmd-idx]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const act = state.cmdActions[Number(btn.getAttribute("data-cmd-idx"))];
        closeCmdPalette();
        if (act) act.run();
      });
    });
  }

  // --- Progressive Disclosure for Asset Kinds (Drawer & Manual Modal) ---
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
      if (state.editKind === "hardware" || state.editKind === "license") {
        $("ed-cycle").value = "one_time";
      } else if (state.editKind === "subscription" && ["unknown", "one_time"].includes($("ed-cycle").value)) {
        $("ed-cycle").value = "monthly";
      } else if (state.editKind === "reimbursable" && $("ed-reimburse-status").value === "none") {
        $("ed-reimburse-status").value = "pending";
      }
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
    if (w) pills.push(`<span class="badge ${w.level === "warn" ? "badge-amber" : w.level === "expired" ? "badge-slate" : "badge-violet"}">🛡️ ${esc(w.statusLabel)}</span>`);
    const cpd = costPerDayInfo(buy, cur, amt);
    if (cpd) pills.push(`<span class="cpd-pill sensitive">已陪伴 ${cpd.daysUsed} 天 · 日均 ${esc(cpd.perDayText)}/天</span>`);
    if (pills.length > 0) {
      box.innerHTML = pills.join("");
      box.classList.remove("hidden");
    } else {
      box.classList.add("hidden");
    }
  }

  function setManualAssetKind(kind) {
    state.manualKind = kind || "subscription";
    document.querySelectorAll("[data-man-kind]").forEach((b) => {
      b.classList.toggle("active", b.getAttribute("data-man-kind") === state.manualKind);
    });
    $("man-panel-license").classList.toggle("hidden", state.manualKind !== "license");
    $("man-panel-hardware").classList.toggle("hidden", state.manualKind !== "hardware");
    $("man-panel-reimb").classList.toggle("hidden", state.manualKind !== "reimbursable");
    $("man-cycle-row").classList.toggle("hidden", state.manualKind === "hardware" || state.manualKind === "license");

    if (state.manualKind === "hardware" || state.manualKind === "license") {
      state.manualCycle = "one_time";
    } else if (state.manualKind === "subscription" && state.manualCycle === "one_time") {
      state.manualCycle = "monthly";
    }
    document.querySelectorAll("[data-man-cycle]").forEach((x) => {
      x.classList.toggle("active", x.getAttribute("data-man-cycle") === state.manualCycle);
    });
  }

  // --- Manual Entry Modal ---
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
      prog.textContent = `正在加密入库 ${i + 1} / ${valid.length}：${f.name}`;
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
    if (reused > 0) parts.push(`SHA-256 防重拦截 ${reused} 份`);
    if (failed > 0) parts.push(`跳过 ${failed} 份（${lastError}）`);
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
    $("btn-theme-cycle").addEventListener("click", cycleTheme);
    $("btn-privacy-toggle").addEventListener("click", () => togglePrivacy());
    $("btn-cmd-open").addEventListener("click", openCmdPalette);
    $("btn-open-manual").addEventListener("click", openManualModal);
    $("btn-close-manual").addEventListener("click", closeManualModal);
    $("btn-cancel-manual").addEventListener("click", closeManualModal);
    $("btn-quick-triage-cta").addEventListener("click", () => switchMainTab("triage"));

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
          switchMainTab("ledger");
          $("search-box").focus();
        } else if (e.key.toLowerCase() === "p") {
          togglePrivacy();
        }
      }
    });

    $("cmd-input").addEventListener("input", (e) => {
      state.cmdIndex = 0;
      renderCmdList(e.target.value);
    });
    $("modal-cmd").addEventListener("click", (e) => {
      if (e.target === $("modal-cmd")) closeCmdPalette();
    });

    // Native OS folder picker buttons
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

    // Lobby controls
    document.querySelectorAll("[data-unlock-mode]").forEach((btn) => {
      btn.addEventListener("click", () => setUnlockMode(btn.getAttribute("data-unlock-mode")));
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
      btn.textContent = "正在解锁…";
      try {
        const payload = { path: $("unlock-path").value.trim() };
        if (state.unlockMode === "passphrase") payload.passphrase = $("unlock-passphrase").value;
        else if (state.unlockMode === "recovery") payload.recovery_key = $("unlock-recovery").value.trim();
        const snap = await api("POST", "/api/vault/unlock", payload);
        $("unlock-passphrase").value = "";
        $("unlock-recovery").value = "";
        state.workspace = snap;
        showWorkspaceScreen();
        showToast("账本已解锁，欢迎回来！", "ok");
      } catch (err) {
        errBox.textContent = err.message;
        errBox.classList.remove("hidden");
      } finally {
        btn.disabled = false;
        btn.textContent = "解锁并打开票据馆";
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
      btn.textContent = "正在初始化加密账本…";
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
            showToast("新加密账本已就绪，把账单拖进来吧！", "ok");
          });
        } else {
          showWorkspaceScreen();
          showToast("新本地账本已就绪，把账单拖进来吧！", "ok");
        }
      } catch (err) {
        errBox.textContent = err.message;
        errBox.classList.remove("hidden");
      } finally {
        btn.disabled = false;
        btn.textContent = "创建并开始使用";
      }
    });

    // Recovery modal
    $("chk-recovery-saved").addEventListener("change", (e) => {
      $("btn-confirm-recovery").disabled = !e.target.checked;
    });
    $("btn-copy-recovery").addEventListener("click", async () => {
      if (!state.pendingRecoveryKey) return;
      try {
        await navigator.clipboard.writeText(state.pendingRecoveryKey);
        showToast("恢复密钥已复制", "ok");
      } catch (_) {
        showToast("请点下载为 .txt 备份文件", "error");
      }
    });
    $("btn-download-recovery").addEventListener("click", () => {
      if (!state.pendingRecoveryKey) return;
      downloadTextFile("SubVault-离线恢复密钥.txt", `# SubVault 离线恢复密钥（请离线妥善保管）\n${state.pendingRecoveryKey}\n`);
    });

    // Manual entry asset kind, quick chips & submit
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
        showToast("已加密登记进我的资产库！", "ok");
      } catch (err) {
        showToast(err.message, "error");
      } finally {
        btn.disabled = false;
      }
    });

    // Topbar file pickers & lock/shutdown
    $("file-picker-input").addEventListener("change", (e) => { importFileList(e.target.files); e.target.value = ""; });
    $("folder-picker-input").addEventListener("change", (e) => { importFileList(e.target.files); e.target.value = ""; });
    $("btn-lock-vault").addEventListener("click", async () => {
      await api("POST", "/api/vault/lock", {});
      state.workspace = null;
      const info = await api("GET", "/api/bootstrap");
      renderRecentVaults(info.recent_vaults || []);
      showLobbyScreen();
      showToast("账本已锁定，内存密钥已清空。", "ok");
    });
    $("btn-shutdown-app").addEventListener("click", async () => {
      try { await api("POST", "/api/shutdown", {}); } catch (_) {}
      document.body.innerHTML = `
        <div style="max-width:480px;margin:120px auto;text-align:center;font-family:var(--font-sans);padding:24px">
          <h2>SubVault 已安全锁定并退出</h2>
          <p style="color:var(--text-secondary)">内存中的所有密钥已清除，本地服务已停止。您现在可以安全关闭此窗口。</p>
        </div>
      `;
    });

    // Full-screen magnetic drag & drop overlay
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

    // Main tabs & view switcher
    document.querySelectorAll(".tab-item").forEach((btn) => {
      btn.addEventListener("click", () => switchMainTab(btn.getAttribute("data-tab")));
    });
    document.querySelectorAll(".vsw-btn").forEach((btn) => {
      btn.addEventListener("click", () => {
        state.viewMode = btn.getAttribute("data-view-mode");
        if (state.workspace) renderLedgerStage(state.workspace);
      });
    });

    // Ledger filters
    $("ledger-filters").querySelectorAll(".pill").forEach((p) => {
      p.addEventListener("click", () => {
        state.ledgerFilter = p.getAttribute("data-filter");
        $("ledger-filters").querySelectorAll(".pill").forEach((x) => x.classList.toggle("active", x === p));
        renderLedgerStage(state.workspace);
      });
    });
    $("btn-clear-cat").addEventListener("click", () => {
      state.categoryFilter = "";
      renderWorkspace();
    });
    $("filter-account").addEventListener("change", (e) => { state.accountFilter = e.target.value; renderLedgerStage(state.workspace); });
    $("filter-currency").addEventListener("change", (e) => { state.currencyFilter = e.target.value; renderLedgerStage(state.workspace); });
    $("search-box").addEventListener("input", (e) => { state.searchQuery = e.target.value; renderLedgerStage(state.workspace); });

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

    // Drawer asset kind progressive disclosure + live hardware metrics + license key controls
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
        if (state.editKind === "license") {
          payload.license_key = $("ed-license-key").value;
        }
        const snap = await api("POST", "/api/sources/save", payload);
        state.workspace = snap;
        renderWorkspace();
        const refreshed = (snap.items || []).find((i) => i.source_id === state.selectedSourceId);
        if (refreshed) renderDrawer(refreshed);
        showToast("已保存，原件未改动且修改记录已入账。", "ok");
      } catch (err) {
        showToast(err.message, "error");
      } finally {
        btn.disabled = false;
        btn.textContent = "保存修改（保留原件并记录修订历史）";
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
        $("redact-stats").textContent = `已完成脱敏：共替换 ${res.total_redactions || 0} 处敏感信息（不影响原件）。`;
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
        showToast("脱敏模板已复制到剪贴板", "ok");
      } catch (_) {
        showToast("请点下载为 .txt", "error");
      }
    });
    $("btn-dl-redact").addEventListener("click", () => downloadTextFile("subvault-redacted-template.txt", state.lastRedactedText));

    // Exports & Rotate
    $("btn-dl-excel").addEventListener("click", () => { window.location.href = tokenUrl("/api/export/csv"); });
    $("btn-dl-zip").addEventListener("click", () => {
      const enc = state.workspace?.vault?.encrypted;
      if (enc && !$("chk-zip-ack").checked) {
        showToast("导出加密账本的明文备份包前，请先勾选上方风险确认框。", "error");
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
      if (!outDir) { showToast("请先填写导出文件夹路径", "error"); return; }
      if (enc && !ack) { showToast("导出加密账本目录前请先勾选明文确认框", "error"); return; }
      try {
        const res = await api("POST", "/api/export/package", { out_dir: outDir, acknowledge_plaintext: Boolean(ack) });
        resBox.className = "result-box";
        resBox.textContent = res.verified
          ? `导出并独立校验通过！共校验 ${res.verification?.files_verified ?? "?"} 个文件，目录：${outDir}`
          : `已导出到 ${outDir}，但校验存在提醒。`;
        resBox.classList.remove("hidden");
        $("verify-dir-path").value = outDir;
      } catch (err) {
        resBox.className = "result-box error";
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
        resBox.className = res.verified ? "result-box" : "result-box error";
        resBox.textContent = res.verified
          ? `完整性校验全部通过！共 ${res.report?.files_verified ?? "?"} 个文件 SHA-256 完全一致。`
          : `校验未通过：${res.report?.reason || "发现文件缺失或被修改"}`;
        resBox.classList.remove("hidden");
      } catch (err) {
        resBox.className = "result-box error";
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
        if (res.recovery_key) openRecoveryModal(res.recovery_key, () => showToast("主密码与离线恢复密钥已更新。", "ok"));
        else showToast("账本主密码已更新。", "ok");
      } catch (err) {
        showToast(err.message, "error");
      }
    });
  }

  document.addEventListener("DOMContentLoaded", boot);
})();
