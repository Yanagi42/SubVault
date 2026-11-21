(() => {
  "use strict";

  // --- Session token bootstrapping (scrubbed immediately from URL bar) ---
  let SESSION_TOKEN = "";
  const hashMatch = window.location.hash.match(/token=([0-9a-fA-F]+)/);
  if (hashMatch) {
    SESSION_TOKEN = hashMatch[1];
    try {
      window.history.replaceState(null, "", window.location.pathname);
    } catch (_) {}
  }

  const state = {
    bootstrap: null,
    workspace: null,
    unlockMode: "passphrase",
    activeTab: "ledger",
    ledgerFilter: "all",
    accountFilter: "",
    currencyFilter: "",
    searchQuery: "",
    selectedSourceId: null,
    activeDrawerTab: "edit",
    pendingRecoveryKey: null,
    textViewActive: false,
    lastRedactedText: "",
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

  const $ = (id) => document.getElementById(id);

  function esc(str) {
    return String(str ?? "")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
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
      err.detail = data.detail;
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

  // --- Bootstrap & Lobby ---
  async function boot() {
    bindEvents();
    if (!SESSION_TOKEN) {
      showToast("缺少会话凭证，请通过双击 subvault-app 打开本页面。", "error");
      return;
    }
    try {
      const info = await api("GET", "/api/bootstrap");
      state.bootstrap = info;
      populateCurrencySelect(info.currencies || ["CNY", "USD", "EUR", "GBP", "JPY", "HKD"]);
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

  function populateCurrencySelect(list) {
    const sel = $("ed-currency");
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
        const idx = Number(btn.getAttribute("data-recent-idx"));
        const target = recent[idx];
        if (target) selectRecentVault(target);
      });
    });
  }

  function selectRecentVault(v) {
    $("unlock-path").value = v.path || "";
    setUnlockMode(v.needs_passphrase ? "passphrase" : "os-disk");
    if (v.needs_passphrase) {
      $("unlock-passphrase").focus();
    }
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
    $("vault-status-chip").textContent = "纯本地 · 不联网";
    $("vault-status-chip").className = "chip chip-neutral";
    $("vault-path-sub").textContent = "零银行登录 · 零邮箱授权 · 账单原件防篡改留存";
    closeDrawer();
  }

  function showWorkspaceScreen() {
    $("screen-lobby").classList.add("hidden");
    $("screen-workspace").classList.remove("hidden");
    $("topbar-workspace-actions").classList.remove("hidden");
    renderWorkspace();
  }

  async function refreshWorkspace() {
    const snap = await api("GET", "/api/workspace");
    state.workspace = snap;
    renderWorkspace();
    if (state.selectedSourceId) {
      const stillThere = (snap.items || []).find((i) => i.source_id === state.selectedSourceId);
      if (stillThere) {
        renderDrawer(stillThere);
      } else {
        closeDrawer();
      }
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

    // Export ack checkboxes & security rotate card visibility
    $("zip-ack-row").classList.toggle("hidden", !v.encrypted);
    $("dir-ack-row").classList.toggle("hidden", !v.encrypted);
    $("card-security-rotate").classList.toggle("hidden", !v.encrypted);

    // Tab counts
    const items = ws.items || [];
    $("count-tab-ledger").textContent = String(items.length);
    const attentionTotal = v.duplicate_count + v.conflict_count + v.needs_entry_count;
    $("count-tab-attention").textContent = String(attentionTotal);

    renderSummary(ws);
    syncFilterDropdowns(ws);
    renderLedgerTable(ws);
    renderAttentionPane(ws);
  }

  function renderSummary(ws) {
    const grid = $("summary-section");
    const v = ws.vault;
    const curList = ws.summary_by_currency || [];
    const cards = [];

    if (curList.length === 0) {
      cards.push(`
        <div class="kpi-card">
          <div class="kpi-label"><span>已登记账单总额</span><span class="badge badge-slate">暂无金额</span></div>
          <div class="kpi-value">0.00</div>
          <div class="kpi-sub">拖入 PDF / HTML / CSV 或截图开始整理</div>
        </div>
      `);
    } else {
      for (const c of curList) {
        const recurLine = c.recurring_count > 0
          ? `周期订阅折算约 ${esc(formatMoneyShort(c.currency, c.monthly_equiv_recurring))}/月 · ${esc(formatMoneyShort(c.currency, c.yearly_equiv_recurring))}/年（${c.recurring_count} 项）`
          : `已登记 ${c.bill_count} 笔账单（标记计费周期后可看月均/年均）`;
        cards.push(`
          <div class="kpi-card">
            <div class="kpi-label">
              <span>${esc(c.currency)} 账单汇总</span>
              <span class="badge badge-emerald">${c.bill_count} 笔</span>
            </div>
            <div class="kpi-value">${esc(formatMoneyShort(c.currency, c.total_recorded))} <small style="font-size:13px;font-weight:600;color:var(--text-muted)">${esc(c.currency)}</small></div>
            <div class="kpi-sub">${recurLine}</div>
          </div>
        `);
      }
    }

    cards.push(`
      <div class="kpi-card">
        <div class="kpi-label">
          <span>原件存证与自动去重</span>
          <span class="badge badge-slate">SHA-256</span>
        </div>
        <div class="kpi-value">${v.source_count} 份原件</div>
        <div class="kpi-sub">已自动拦截重复导入 ${v.duplicate_count} 项 · 待补录/核对 ${v.needs_entry_count + v.conflict_count} 项</div>
      </div>
    `);

    grid.innerHTML = cards.join("");
  }

  function syncFilterDropdowns(ws) {
    const accSel = $("filter-account");
    const prevAcc = state.accountFilter;
    accSel.innerHTML = `<option value="">所有账号</option>` +
      (ws.accounts || []).map((a) => `<option value="${esc(a)}">${esc(a)}</option>`).join("");
    accSel.value = prevAcc;

    const curSel = $("filter-currency");
    const prevCur = state.currencyFilter;
    const curs = (ws.summary_by_currency || []).map((c) => c.currency);
    curSel.innerHTML = `<option value="">所有币种</option>` +
      curs.map((c) => `<option value="${esc(c)}">${esc(c)}</option>`).join("");
    curSel.value = prevCur;
  }

  function filteredItems(ws) {
    const q = state.searchQuery.trim().toLowerCase();
    return (ws.items || []).filter((item) => {
      const ann = item.annotation || {};
      const amtVal = item.amount_fact ? item.amount_fact.value : null;
      const flags = item.status_flags || {};

      if (state.ledgerFilter === "recurring") {
        if (!["monthly", "yearly", "quarterly", "weekly"].includes(ann.billing_cycle)) return false;
      } else if (state.ledgerFilter === "needs") {
        if (!flags.needs_manual_entry && !flags.has_conflict && !flags.is_credit_or_order_warning) return false;
      } else if (state.ledgerFilter === "duplicates") {
        if (!flags.has_duplicate) return false;
      } else if (state.ledgerFilter === "csv") {
        if (!flags.is_csv_statement) return false;
      }

      if (state.accountFilter && (ann.account_label || "") !== state.accountFilter) return false;
      if (state.currencyFilter && (!amtVal || amtVal.currency !== state.currencyFilter)) return false;

      if (q) {
        const hay = [
          ann.vendor,
          ann.plan_name,
          ann.account_label,
          ann.category,
          ann.notes,
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

  function renderLedgerTable(ws) {
    const wrap = $("ledger-list-wrap");
    const list = filteredItems(ws);
    if (list.length === 0) {
      const isEmptyVault = (ws.items || []).length === 0;
      wrap.innerHTML = `
        <div class="empty-state">
          <h4>${isEmptyVault ? "这本账本还是空的" : "没有符合筛选条件的账单"}</h4>
          <p>${isEmptyVault ? "把你的 PDF 发票、邮件 HTML、支付宝/微信 CSV 或收据截图拖到上方，马上开始整理。" : "可以尝试清空搜索词或切换上方筛选条件。"}</p>
          ${
            isEmptyVault
              ? `<div style="margin-top:14px"><button type="button" id="btn-load-demo-pack" class="btn btn-outline btn-sm">手边暂时没账单？载入 4 份中英文示例账单体验一下</button></div>`
              : ""
          }
        </div>
      `;
      const demoBtn = $("btn-load-demo-pack");
      if (demoBtn) {
        demoBtn.addEventListener("click", async () => {
          demoBtn.disabled = true;
          demoBtn.textContent = "正在导入示例账单…";
          try {
            const snap = await api("POST", "/api/import-demo-pack", {});
            state.workspace = snap;
            renderWorkspace();
            showToast("已导入 4 份示例账单（含中文电子发票、英文邮件、重复去重演示与支付宝流水）！", "ok");
          } catch (err) {
            showToast(err.message, "error");
            demoBtn.disabled = false;
            demoBtn.textContent = "手边暂时没账单？载入 4 份中英文示例账单体验一下";
          }
        });
      }
      return;
    }

    const rowsHtml = list
      .map((item) => {
        const ann = item.annotation || {};
        const amtVal = item.amount_fact ? item.amount_fact.value : null;
        const perVal = item.period_fact ? item.period_fact.value : null;
        const flags = item.status_flags || {};

        const title = ann.vendor || item.display_name;
        const subtitleParts = [];
        if (ann.plan_name) subtitleParts.push(ann.plan_name);
        if (ann.vendor && ann.vendor !== item.display_name) subtitleParts.push(item.display_name);
        const subtitle = subtitleParts.join(" · ");

        const cycleText = CYCLE_LABELS[ann.billing_cycle] || "未标注";
        const accountText = ann.account_label || "未分组";

        let amountHtml = "";
        if (amtVal && amtVal.amount_text) {
          amountHtml = `<span class="amount-cell">${esc(formatMoney(amtVal.currency, amtVal.amount_text))}</span>`;
        } else if (flags.is_csv_statement) {
          amountHtml = `<span class="badge badge-slate">CSV 流水（${item.csv_row_count} 条）</span>`;
        } else if (flags.is_credit_or_order_warning) {
          amountHtml = `<span class="badge badge-amber">退款/未付订单</span>`;
        } else {
          amountHtml = `<span class="amount-missing">点击补录金额 →</span>`;
        }

        const dateText = perVal && perVal.period_start
          ? `${perVal.period_start} ~ ${perVal.period_end}`
          : ann.bill_date || "—";

        const badges = [];
        if (item.object_intact) badges.push(`<span class="badge badge-emerald">原件完整</span>`);
        if (flags.has_duplicate) badges.push(`<span class="badge badge-slate">去重 ×${item.occurrence_count}</span>`);
        if (flags.has_conflict) badges.push(`<span class="badge badge-amber">金额冲突</span>`);
        if (item.amount_fact && item.amount_fact.revision > 1) badges.push(`<span class="badge badge-slate">已修正</span>`);
        if (ann.category) badges.push(`<span class="badge badge-slate">${esc(ann.category)}</span>`);

        return `
          <tr data-source-id="${esc(item.source_id)}">
            <td class="vendor-cell">
              <strong>${esc(title)}</strong>
              ${subtitle ? `<small>${esc(subtitle)}</small>` : ""}
            </td>
            <td>${esc(accountText)}</td>
            <td>${esc(cycleText)}</td>
            <td>${amountHtml}</td>
            <td style="white-space:nowrap;font-size:12px;color:var(--text-secondary)">${esc(dateText)}</td>
            <td><div class="tags-cell">${badges.join("")}</div></td>
          </tr>
        `;
      })
      .join("");

    wrap.innerHTML = `
      <table class="ledger-table">
        <thead>
          <tr>
            <th>商家与账单</th>
            <th>归属账号</th>
            <th>周期</th>
            <th>金额</th>
            <th>日期 / 服务周期</th>
            <th>凭证状态</th>
          </tr>
        </thead>
        <tbody>${rowsHtml}</tbody>
      </table>
    `;

    wrap.querySelectorAll("tbody tr[data-source-id]").forEach((tr) => {
      tr.addEventListener("click", () => {
        const sid = tr.getAttribute("data-source-id");
        const found = (state.workspace.items || []).find((i) => i.source_id === sid);
        if (found) openDrawer(found);
      });
    });
  }

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
              ? "同一份文件里出现多个不同金额，系统未盲猜，请点击核对"
              : item.status_flags.is_credit_or_order_warning
                ? "疑似退款单或未支付订单，系统未计入花费"
                : "未自动提取到金额（可能是扫描件或截图），点击补录";
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
        const sid = btn.getAttribute("data-open-sid");
        const found = items.find((i) => i.source_id === sid);
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

    $("drawer-title").textContent = ann.vendor || item.display_name;
    $("drawer-subtitle").textContent = `${item.display_name} · ${formatBytes(item.byte_length)} · ${item.media_type}`;

    // Warnings
    const warns = [];
    if (item.status_flags.has_conflict) {
      warns.push("该文件包含多个不同的金额候选，系统为避免误记未自动采纳。请对照「查看原件」后在下方手动填写正确金额并保存。");
    }
    if (item.status_flags.is_credit_or_order_warning) {
      warns.push("系统检测到该文件可能是退款单、红字发票或未支付订单，未自动计入花费。如果确认已实付，可手动补录金额。");
    }
    if (item.status_flags.needs_manual_entry) {
      warns.push("尚未记录金额（常见于扫描版 PDF、截图或非标准账单）。原件已加密留存，请直接补充商家与金额。");
    }
    $("drawer-warnings").innerHTML = warns
      .map((w) => `<div class="warning-banner">${esc(w)}</div>`)
      .join("");

    // Edit form
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

    // Evidence metadata & occurrences
    $("evidence-meta").innerHTML = `
      <div><strong>文件名：</strong>${esc(item.display_name)}（${formatBytes(item.byte_length)}）</div>
      <div><strong>原件 SHA-256：</strong><code>${esc(item.content_sha256)}</code></div>
      <div><strong>完整性状态：</strong>${item.object_intact ? "已实时解密核验一致，字节未改动" : "校验异常，请检查账本"}</div>
    `;
    $("btn-download-raw").href = tokenUrl(`/api/sources/${encodeURIComponent(item.source_id)}/raw?download=1`);

    $("evidence-occurrences").innerHTML = `
      <div style="font-size:12px;font-weight:600;color:var(--text-secondary);margin-bottom:4px">导入记录（共 ${item.occurrence_count} 次）</div>
      ${(item.occurrences || [])
        .map(
          (o) => `
          <div class="occ-item">
            <strong>${esc(o.display_name)}</strong> · ${formatTime(o.recorded_at)}
            ${o.storage_reused ? `<span class="badge badge-slate" style="margin-left:6px">重复文件自动复用原件</span>` : `<span class="badge badge-emerald" style="margin-left:6px">首次入库</span>`}
          </div>
        `
        )
        .join("")}
    `;

    // CSV picker rows
    if (item.status_flags.is_csv_statement) {
      renderCsvRows(item);
    }

    // History timeline
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
              <strong>${esc(r.merchant || "未标注商户")} · ${esc(formatMoney(r.currency || "CNY", r.amount_text))}</strong>
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
              <div><strong>${esc(factLabel)} · ${esc(kindLabel)}</strong> → ${esc(valText)}</div>
              <div style="font-size:12px;color:var(--text-muted)">版本 #${idx + 1} · ${formatTime(dec.recorded_at)}</div>
            </div>
            ${
              canUndo
                ? `<button type="button" class="btn btn-outline btn-sm" data-undo-dec="${esc(dec.decision_id)}" data-undo-rev="${fact.revision}">撤销此步</button>`
                : ""
            }
          </div>
        `);
      });
    }
    el.innerHTML = blocks.length
      ? blocks.join("")
      : `<p class="muted-note">暂无金额或周期修订记录。保存上方表单后会自动记录在这里。</p>`;

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

  // --- Batch File & Folder Drag/Drop Upload ---
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
      prog.textContent = `正在入库 ${i + 1} / ${valid.length}：${f.name}`;
      try {
        const buf = await f.arrayBuffer();
        const b64 = arrayBufferToBase64(buf);
        const res = await api("POST", "/api/import", {
          filename: f.name,
          content_base64: b64,
          batch_id: batchId,
        });
        if (res.reused) {
          reused += 1;
        } else {
          added += 1;
        }
      } catch (err) {
        failed += 1;
        lastError = `${f.name}: ${err.message}`;
      }
    }

    prog.classList.add("hidden");
    await refreshWorkspace();

    const parts = [];
    if (added > 0) parts.push(`新入库 ${added} 份`);
    if (reused > 0) parts.push(`自动去重拦截 ${reused} 份`);
    if (failed > 0) parts.push(`跳过/失败 ${failed} 份（${lastError}）`);
    showToast(parts.join("，") || "导入完成", failed > 0 && added === 0 ? "error" : "ok");
  }

  async function collectDroppedFiles(dataTransfer) {
    const out = [];
    const items = dataTransfer.items ? Array.from(dataTransfer.items) : [];
    const hasEntries = items.some((it) => typeof it.webkitGetAsEntry === "function");
    if (!hasEntries) {
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
          for (const child of batch) {
            await walkEntry(child);
          }
        }
      }
    }
    for (const item of items) {
      const entry = item.webkitGetAsEntry();
      if (entry) await walkEntry(entry);
    }
    return out;
  }

  // --- Recovery Modal ---
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

    // Keyboard shortcuts: Esc closes drawer, / focuses search
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && state.selectedSourceId) {
        closeDrawer();
      } else if (
        e.key === "/" &&
        !["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName) &&
        !$("screen-workspace").classList.contains("hidden")
      ) {
        e.preventDefault();
        $("search-box").focus();
      }
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

    // Lobby unlock mode toggle
    document.querySelectorAll("[data-unlock-mode]").forEach((btn) => {
      btn.addEventListener("click", () => setUnlockMode(btn.getAttribute("data-unlock-mode")));
    });

    // Lobby create protection toggle
    document.querySelectorAll('input[name="create-protection"]').forEach((radio) => {
      radio.addEventListener("change", () => {
        const isPass = radio.value === "passphrase";
        $("opt-passphrase").classList.toggle("active", isPass);
        $("opt-os-disk").classList.toggle("active", !isPass);
        $("create-passphrase-fields").classList.toggle("hidden", !isPass);
      });
    });

    // Unlock vault form
    $("form-unlock-vault").addEventListener("submit", async (e) => {
      e.preventDefault();
      const errBox = $("unlock-error");
      errBox.classList.add("hidden");
      const btn = $("btn-unlock-submit");
      btn.disabled = true;
      btn.textContent = "正在解锁…";
      try {
        const payload = { path: $("unlock-path").value.trim() };
        if (state.unlockMode === "passphrase") {
          payload.passphrase = $("unlock-passphrase").value;
        } else if (state.unlockMode === "recovery") {
          payload.recovery_key = $("unlock-recovery").value.trim();
        }
        const snap = await api("POST", "/api/vault/unlock", payload);
        $("unlock-passphrase").value = "";
        $("unlock-recovery").value = "";
        state.workspace = snap;
        showWorkspaceScreen();
        showToast("账本已解锁，所有数据仅在本地处理。", "ok");
      } catch (err) {
        errBox.textContent = err.message;
        errBox.classList.remove("hidden");
      } finally {
        btn.disabled = false;
        btn.textContent = "解锁并进入账本";
      }
    });

    // Create vault form
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
            showToast("新加密账本已创建，直接把账单拖进来吧！", "ok");
          });
        } else {
          showWorkspaceScreen();
          showToast("新本地账本已创建，直接把账单拖进来吧！", "ok");
        }
      } catch (err) {
        errBox.textContent = err.message;
        errBox.classList.remove("hidden");
      } finally {
        btn.disabled = false;
        btn.textContent = "创建并立即使用";
      }
    });

    // Recovery modal buttons
    $("chk-recovery-saved").addEventListener("change", (e) => {
      $("btn-confirm-recovery").disabled = !e.target.checked;
    });
    $("btn-copy-recovery").addEventListener("click", async () => {
      if (!state.pendingRecoveryKey) return;
      try {
        await navigator.clipboard.writeText(state.pendingRecoveryKey);
        showToast("恢复密钥已复制到剪贴板", "ok");
      } catch (_) {
        showToast("复制失败，请手动选中或点击下载为 .txt", "error");
      }
    });
    $("btn-download-recovery").addEventListener("click", () => {
      if (!state.pendingRecoveryKey) return;
      downloadTextFile(
        "SubVault-离线恢复密钥.txt",
        `# SubVault 离线恢复密钥（请离线妥善保管）\n${state.pendingRecoveryKey}\n`
      );
    });

    // Topbar actions
    $("file-picker-input").addEventListener("change", (e) => {
      importFileList(e.target.files);
      e.target.value = "";
    });
    $("folder-picker-input").addEventListener("change", (e) => {
      importFileList(e.target.files);
      e.target.value = "";
    });
    $("btn-lock-vault").addEventListener("click", async () => {
      await api("POST", "/api/vault/lock", {});
      state.workspace = null;
      const info = await api("GET", "/api/bootstrap");
      renderRecentVaults(info.recent_vaults || []);
      showLobbyScreen();
      showToast("账本已锁定，内存密钥已清空。", "ok");
    });
    $("btn-shutdown-app").addEventListener("click", async () => {
      try {
        await api("POST", "/api/shutdown", {});
      } catch (_) {}
      document.body.innerHTML = `
        <div style="max-width:480px;margin:120px auto;text-align:center;font-family:var(--font-sans);padding:24px">
          <h2>SubVault 已安全锁定并退出</h2>
          <p style="color:#475569">内存中的所有密钥已清除，本地服务已停止。您现在可以安全关闭此页面。</p>
        </div>
      `;
    });

    // Drag & Drop across entire workspace
    const dz = $("drop-zone");
    ["dragenter", "dragover"].forEach((ev) => {
      window.addEventListener(ev, (e) => {
        if ($("screen-workspace").classList.contains("hidden")) return;
        e.preventDefault();
        dz.classList.add("drag-over");
      });
    });
    ["dragleave", "drop"].forEach((ev) => {
      window.addEventListener(ev, (e) => {
        if (ev === "drop") e.preventDefault();
        dz.classList.remove("drag-over");
      });
    });
    window.addEventListener("drop", async (e) => {
      if ($("screen-workspace").classList.contains("hidden")) return;
      e.preventDefault();
      const files = await collectDroppedFiles(e.dataTransfer);
      importFileList(files);
    });

    // Main workspace tabs
    document.querySelectorAll(".tab-item").forEach((btn) => {
      btn.addEventListener("click", () => {
        const tab = btn.getAttribute("data-tab");
        state.activeTab = tab;
        document.querySelectorAll(".tab-item").forEach((b) => {
          b.classList.toggle("active", b.getAttribute("data-tab") === tab);
        });
        ["ledger", "attention", "export"].forEach((name) => {
          $(`pane-${name}`).classList.toggle("hidden", name !== tab);
        });
      });
    });

    // Ledger filters
    $("ledger-filters").querySelectorAll(".pill").forEach((p) => {
      p.addEventListener("click", () => {
        state.ledgerFilter = p.getAttribute("data-filter");
        $("ledger-filters").querySelectorAll(".pill").forEach((x) => x.classList.toggle("active", x === p));
        renderLedgerTable(state.workspace);
      });
    });
    $("filter-account").addEventListener("change", (e) => {
      state.accountFilter = e.target.value;
      renderLedgerTable(state.workspace);
    });
    $("filter-currency").addEventListener("change", (e) => {
      state.currencyFilter = e.target.value;
      renderLedgerTable(state.workspace);
    });
    $("search-box").addEventListener("input", (e) => {
      state.searchQuery = e.target.value;
      renderLedgerTable(state.workspace);
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

    // Save item edit / manual entry
    $("form-edit-item").addEventListener("submit", async (e) => {
      e.preventDefault();
      if (!state.selectedSourceId) return;
      const btn = $("btn-save-item");
      btn.disabled = true;
      btn.textContent = "保存中…";
      try {
        const snap = await api("POST", "/api/sources/save", {
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
        });
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

    // Redact inside drawer
    $("btn-run-redact").addEventListener("click", async () => {
      if (!state.selectedSourceId) return;
      const terms = ($("redact-custom-terms").value || "")
        .split(/[,，\n]/)
        .map((s) => s.trim())
        .filter(Boolean);
      const btn = $("btn-run-redact");
      btn.disabled = true;
      try {
        const res = await api("POST", "/api/sources/redact", {
          source_id: state.selectedSourceId,
          redact_terms: terms,
        });
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
        showToast("复制失败，请点击下载 .txt", "error");
      }
    });
    $("btn-dl-redact").addEventListener("click", () => {
      downloadTextFile("subvault-redacted-template.txt", state.lastRedactedText);
    });

    // Export tab actions
    $("btn-dl-excel").addEventListener("click", () => {
      window.location.href = tokenUrl("/api/export/csv");
    });
    $("btn-dl-zip").addEventListener("click", () => {
      const enc = state.workspace?.vault?.encrypted;
      if (enc && !$("chk-zip-ack").checked) {
        showToast("导出加密账本的明文备份包前，请先勾选上方风险确认框。", "error");
        return;
      }
      const q = enc ? "?acknowledge_plaintext=yes" : "";
      window.location.href = tokenUrl(`/api/export/zip${q}`);
    });
    $("btn-export-dir").addEventListener("click", async () => {
      const outDir = $("export-dir-path").value.trim();
      const enc = state.workspace?.vault?.encrypted;
      const ack = $("chk-dir-ack").checked;
      const resBox = $("export-dir-result");
      resBox.classList.add("hidden");
      if (!outDir) {
        showToast("请先填写导出文件夹路径", "error");
        return;
      }
      if (enc && !ack) {
        showToast("导出加密账本目录前请先勾选明文确认框", "error");
        return;
      }
      try {
        const res = await api("POST", "/api/export/package", {
          out_dir: outDir,
          acknowledge_plaintext: Boolean(ack),
        });
        resBox.className = "result-box";
        resBox.textContent = res.verified
          ? `导出并独立校验通过！共校验 ${res.verification?.files_verified ?? "?"} 个文件，目录：${outDir}`
          : `已导出到 ${outDir}，但校验存在提醒，请查看详情。`;
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
      if (!dir) {
        showToast("请填写备份包目录路径", "error");
        return;
      }
      try {
        const res = await api("POST", "/api/verify-package", { package_dir: dir });
        resBox.className = res.verified ? "result-box" : "result-box error";
        resBox.textContent = res.verified
          ? `完整性校验全部通过！所有原件与表格 SHA-256 与清单完全一致（共 ${res.report?.files_verified ?? "?"} 个文件）。`
          : `校验未通过：${res.report?.reason || "发现文件缺失或被修改"}`;
        resBox.classList.remove("hidden");
      } catch (err) {
        resBox.className = "result-box error";
        resBox.textContent = err.message;
        resBox.classList.remove("hidden");
      }
    });

    // Rotate passphrase
    $("form-rotate-pass").addEventListener("submit", async (e) => {
      e.preventDefault();
      const p1 = $("rotate-new-pass").value;
      const p2 = $("rotate-new-pass-confirm").value;
      const rotateRec = $("chk-rotate-recovery").checked;
      if (p1.length < 6) {
        showToast("新密码至少需要 6 位字符", "error");
        return;
      }
      if (p1 !== p2) {
        showToast("两次输入的新密码不一致", "error");
        return;
      }
      try {
        const res = await api("POST", "/api/vault/rotate-passphrase", {
          new_passphrase: p1,
          rotate_recovery_key: rotateRec,
        });
        $("rotate-new-pass").value = "";
        $("rotate-new-pass-confirm").value = "";
        $("chk-rotate-recovery").checked = false;
        if (res.recovery_key) {
          openRecoveryModal(res.recovery_key, () => {
            showToast("主密码与离线恢复密钥已更新。", "ok");
          });
        } else {
          showToast("账本主密码已更新，下次解锁请使用新密码。", "ok");
        }
      } catch (err) {
        showToast(err.message, "error");
      }
    });
  }

  document.addEventListener("DOMContentLoaded", boot);
})();
