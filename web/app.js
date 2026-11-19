import { QUICK_SAMPLE_SCENARIOS } from "./demo-data.js";
import { SubVaultStore } from "./engine.js";

const STORAGE_KEY = "subvault_demo_state_v1";
const store = new SubVaultStore();

const state = {
  view: "ledger",
  filter: "all",
  search: "",
  selectedSourceId: null,
  importCounter: 1,
  persistEnabled: true
};

function persistVault() {
  if (!state.persistEnabled) return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(store.toSnapshot()));
  } catch {
    // Ignore quota / privacy-mode localStorage errors
  }
}

function clearPersistedVault() {
  try {
    window.localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Ignore
  }
}

function restorePersistedVault() {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return false;
    return store.fromSnapshot(JSON.parse(raw));
  } catch {
    return false;
  }
}

function downloadBinaryFile(filename, uint8, mime = "application/zip") {
  const blob = new Blob([uint8], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function esc(value) {
  return String(value ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function showToast(message, isWarn = false) {
  const el = document.getElementById("status-toast");
  const txt = document.getElementById("status-toast-text");
  txt.textContent = message;
  el.classList.toggle("warn", isWarn);
  el.classList.add("visible");
}

function hideToast() {
  document.getElementById("status-toast").classList.remove("visible");
}

function downloadTextFile(filename, content, mime = "text/plain;charset=utf-8") {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function initQuickSamples() {
  const bar = document.getElementById("quick-sample-bar");
  for (const sample of QUICK_SAMPLE_SCENARIOS) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "btn btn-sm";
    btn.title = sample.description;
    btn.textContent = `+ ${sample.label}`;
    btn.addEventListener("click", () => {
      const res = store.ingestDocument({
        ...sample.doc,
        commandId: `cmd_quick_${sample.id}_${state.importCounter++}`,
        batchId: `b_quick_${state.importCounter}`
      });
      state.selectedSourceId = res.source.sourceId;
      renderAll();
      if (res.storageReused) {
        showToast(
          `Duplicate detected via SHA-256! "${sample.doc.displayName}" matched existing ${res.source.objectId} — logged occurrence #${res.source.occurrenceIds.length} with zero duplicate charges.`,
          false
        );
      } else if (res.source.extractionStatus === "unsupported") {
        showToast(
          `Preserved "${sample.doc.displayName}" verbatim (${res.source.byteLength} B). Format 4 skipped CSS extraction by design — raw bytes stay verified.`,
          true
        );
      } else {
        showToast(
          `Imported "${sample.doc.displayName}" as ${res.source.objectId} (${res.source.contentSha256.slice(0, 12)}…) — extracted stated facts cleanly.`,
          false
        );
      }
    });
    bar.appendChild(btn);
  }
}

function setView(viewName) {
  state.view = viewName;
  for (const btn of document.querySelectorAll(".tab-btn")) {
    const active = btn.dataset.view === viewName;
    btn.classList.toggle("active", active);
    btn.setAttribute("aria-selected", String(active));
  }
  document.getElementById("view-ledger").classList.toggle("hidden", viewName !== "ledger");
  document.getElementById("view-audit").classList.toggle("hidden", viewName !== "audit");
  document.getElementById("view-export").classList.toggle("hidden", viewName !== "export");
  renderAll();
}

function renderKpis() {
  const stats = store.getSummaryStats();
  const audit = store.getAuditReport();
  const grid = document.getElementById("kpi-grid");
  grid.innerHTML = `
    <div class="kpi-card kpi-accent-emerald">
      <div class="kpi-label">
        <span>Preserved Originals</span>
        <span class="badge badge-emerald">SHA-256</span>
      </div>
      <div class="kpi-value">${stats.uniqueObjects} <span style="font-size:14px;font-weight:600;color:var(--ink-muted)">objects</span></div>
      <div class="kpi-sub">${stats.totalOccurrences} import occurrence(s) · 0 bytes normalized</div>
    </div>
    <div class="kpi-card">
      <div class="kpi-label">
        <span>Duplicate Detection</span>
        <span class="badge badge-blue">Content Hash</span>
      </div>
      <div class="kpi-value">${stats.duplicateHits} <span style="font-size:14px;font-weight:600;color:var(--ink-muted)">reused</span></div>
      <div class="kpi-sub">${audit.duplicates.length} source(s) with renamed copies · 0 duplicate spend</div>
    </div>
    <div class="kpi-card">
      <div class="kpi-label">
        <span>Stated Paid Facts</span>
        <span class="badge badge-slate">Unverified</span>
      </div>
      <div class="kpi-value mono">$${esc(stats.statedUsdTotal)}</div>
      <div class="kpi-sub">${stats.paidFactCount} adopted fact(s) · ${stats.humanCorrections} human correction(s)</div>
    </div>
    <div class="kpi-card kpi-accent-amber">
      <div class="kpi-label">
        <span>Needs Human Call</span>
        <span class="badge badge-amber">No Guessing</span>
      </div>
      <div class="kpi-value">${stats.needsHumanCall} <span style="font-size:14px;font-weight:600;color:var(--ink-muted)">items</span></div>
      <div class="kpi-sub">${stats.conflictCount} amount conflict · ${stats.maskedCount} masked email · ${audit.nonPaymentTotals.length} non-paid totals</div>
    </div>
  `;

  document.getElementById("tab-count-ledger").textContent = String(store.sources.length);
  document.getElementById("tab-count-audit").textContent = String(
    audit.duplicates.length + audit.conflicts.length + audit.maskedUnassigned.length
  );
}

function getFilteredSources() {
  const audit = store.getAuditReport();
  const conflictIds = new Set(
    audit.conflicts.flatMap((c) => [c.invoiceSource.sourceId, c.csvSource.sourceId])
  );
  const maskedIds = new Set(audit.maskedUnassigned.map((s) => s.sourceId));
  const nonPaidIds = new Set(audit.nonPaymentTotals.map((s) => s.sourceId));
  const q = state.search.trim().toLowerCase();

  return store.sources.filter((s) => {
    const occs = store.getOccurrencesForSource(s.sourceId);
    if (state.filter === "subscriptions" && !["monthly", "annual", "cancellation"].includes(s.billingCategory)) {
      return false;
    }
    if (state.filter === "duplicates" && occs.length <= 1) {
      return false;
    }
    if (
      state.filter === "review" &&
      !conflictIds.has(s.sourceId) &&
      !maskedIds.has(s.sourceId) &&
      !nonPaidIds.has(s.sourceId) &&
      s.extractionStatus !== "unsupported" &&
      s.extractionStatus !== "conflict"
    ) {
      return false;
    }
    if (
      state.filter === "other" &&
      s.billingCategory !== "one_time" &&
      !s.mediaType.includes("html")
    ) {
      return false;
    }
    if (q) {
      const hay = [
        s.merchant,
        s.accountLabel,
        s.accountEmail,
        s.displayName,
        s.externalPath,
        s.orderId || "",
        s.contentSha256,
        s.sourceId,
        s.objectId
      ]
        .join(" ")
        .toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });
}

function categoryBadge(cat, mediaType) {
  if (mediaType.includes("html")) return '<span class="badge badge-blue">HTML Bill</span>';
  switch (cat) {
    case "monthly":
      return '<span class="badge badge-slate">Monthly</span>';
    case "annual":
      return '<span class="badge badge-slate">Annual</span>';
    case "one_time":
      return '<span class="badge badge-emerald">One-Time</span>';
    case "credit_note":
      return '<span class="badge badge-amber">Credit Note</span>';
    case "order_confirm":
      return '<span class="badge badge-slate">Order Total</span>';
    case "csv_export":
      return '<span class="badge badge-amber">Store CSV</span>';
    case "cancellation":
      return '<span class="badge badge-amber">Cancellation</span>';
    default:
      return '<span class="badge badge-slate">Document</span>';
  }
}

function renderLedger() {
  const container = document.getElementById("ledger-table-container");
  if (store.sources.length === 0) {
    container.innerHTML = `
      <div class="empty-state">
        <h3>Your local SubVault is empty</h3>
        <p>Click <strong>+ Import Sample Batch</strong> to load the 12-document synthetic corpus (covering monthly/annual bills, duplicate detection, and CSV conflicts), or drop local files anywhere on this page.</p>
        <button id="empty-import-btn" class="btn btn-emerald" type="button">+ Import Sample Batch Now</button>
      </div>
    `;
    document.getElementById("empty-import-btn").addEventListener("click", handleImportSampleBatch);
    renderInspector();
    return;
  }

  const rows = getFilteredSources();
  if (!rows.some((s) => s.sourceId === state.selectedSourceId)) {
    state.selectedSourceId = rows[0]?.sourceId || store.sources[0]?.sourceId || null;
  }

  const audit = store.getAuditReport();
  const conflictIds = new Set(
    audit.conflicts.flatMap((c) => [c.invoiceSource.sourceId, c.csvSource.sourceId])
  );

  if (rows.length === 0) {
    container.innerHTML = `<div class="empty-state"><h3>No matching sources</h3><p>Try clearing your search query or switching filter pills.</p></div>`;
    renderInspector();
    return;
  }

  const bodyHtml = rows
    .map((s) => {
      const facts = store.getFactsForSource(s.sourceId);
      const occs = store.getOccurrencesForSource(s.sourceId);
      const amtFact = facts.find((f) => f.factType === "stated_amount_paid");
      const perFact = facts.find((f) => f.factType === "stated_service_period");
      const isSelected = s.sourceId === state.selectedSourceId;
      const isConflict = conflictIds.has(s.sourceId);
      const isMasked = s.accountEmail.includes("*");

      let amountHtml = `<span style="color:var(--ink-muted)">—</span>`;
      if (amtFact) {
        const corrected = amtFact.value.amount_text !== amtFact.originalValue.amount_text;
        amountHtml = `
          <div class="amount-main mono">USD ${esc(amtFact.value.amount_text)}</div>
          <div class="amount-note ${corrected ? "corrected" : ""}">
            ${corrected ? `Edited · orig ${esc(amtFact.originalValue.amount_text)}` : "Source-stated"}
          </div>
        `;
      } else {
        const totalCand = s.candidates.find(
          (c) => c.kind === "credit_note_total" || c.kind === "unjudged_order_total" || c.kind === "csv_charged_amount"
        );
        if (totalCand) {
          amountHtml = `
            <div class="amount-main mono" style="color:var(--amber)">USD ${esc(totalCand.value.amount_text)}</div>
            <div class="amount-note">${esc(totalCand.kind.replace(/_/g, " "))}</div>
          `;
        }
      }

      let scopeHtml = `<span style="color:var(--ink-muted)">Not stated</span>`;
      if (perFact) {
        scopeHtml = `<div class="mono" style="font-size:12px">${esc(perFact.value.period_start)} → ${esc(perFact.value.period_end)}</div><div class="amount-note">Current status: unknown</div>`;
      } else if (s.metadata?.usableThrough) {
        scopeHtml = `<div class="mono" style="font-size:12px">Usable thru ${esc(s.metadata.usableThrough)}</div><div class="amount-note">Auto-renewal cancelled</div>`;
      } else if (s.metadata?.licenseScope) {
        scopeHtml = `<div style="font-size:12px">Perpetual · up to 2 devices</div><div class="amount-note">No subscription term</div>`;
      }

      const badges = [];
      if (occs.length > 1) {
        badges.push(`<span class="badge badge-blue">Deduped ×${occs.length}</span>`);
      }
      if (isConflict) {
        badges.push(`<span class="badge badge-rose">Conflict $99 vs $107.91</span>`);
      }
      if (isMasked) {
        badges.push(`<span class="badge badge-amber">Unassigned Mask</span>`);
      }
      if (s.extractionStatus === "unsupported") {
        badges.push(`<span class="badge badge-amber">CSS Skipped · Safe</span>`);
      }
      if (badges.length === 0) {
        badges.push(`<span class="badge badge-emerald">Preserved</span>`);
      }

      return `
        <tr data-source-id="${esc(s.sourceId)}" class="${isSelected ? "selected" : ""}">
          <td>
            <div class="vendor-name">
              <span>${esc(s.merchant)}</span>
              ${categoryBadge(s.billingCategory, s.mediaType)}
            </div>
            <div class="file-meta mono">${esc(s.displayName)} · ${esc(s.objectId)}</div>
          </td>
          <td>
            <div style="font-weight:600;font-size:12.5px">${esc(s.accountLabel)}</div>
            <div class="file-meta">${esc(s.accountEmail)}</div>
          </td>
          <td>${amountHtml}</td>
          <td>${scopeHtml}</td>
          <td><div class="stack-badges">${badges.join("")}</div></td>
        </tr>
      `;
    })
    .join("");

  container.innerHTML = `
    <table class="ledger-table">
      <thead>
        <tr>
          <th>Vendor &amp; Source</th>
          <th>Account</th>
          <th>Stated Amount</th>
          <th>Period / Scope</th>
          <th>Verification &amp; Flags</th>
        </tr>
      </thead>
      <tbody>${bodyHtml}</tbody>
    </table>
  `;

  for (const tr of container.querySelectorAll("tbody tr")) {
    tr.addEventListener("click", () => {
      state.selectedSourceId = tr.dataset.sourceId;
      renderLedger();
    });
  }

  renderInspector();
}

function highlightSourceContent(rawContent, candidates) {
  let escaped = esc(rawContent);
  const excerpts = [...new Set(candidates.map((c) => c.excerpt).filter(Boolean))];
  for (const ex of excerpts) {
    const exEsc = esc(ex);
    if (exEsc.length > 0 && escaped.includes(exEsc)) {
      escaped = escaped.replace(exEsc, `<mark class="hl-excerpt">${exEsc}</mark>`);
    }
  }
  return escaped;
}

function renderInspector() {
  const body = document.getElementById("inspector-body");
  const badge = document.getElementById("inspector-integrity-badge");
  const source = store.sources.find((s) => s.sourceId === state.selectedSourceId);

  if (!source) {
    badge.textContent = "No Selection";
    body.innerHTML = `<div class="empty-state" style="padding:28px 12px"><p>Select any source row to inspect its SHA-256 original, occurrences, and append-only decision history.</p></div>`;
    return;
  }

  const obj = store.getObjectForSource(source);
  const occs = store.getOccurrencesForSource(source.sourceId);
  const facts = store.getFactsForSource(source.sourceId);
  const amtFact = facts.find((f) => f.factType === "stated_amount_paid");
  const decisions = amtFact ? store.getDecisionsForFact(amtFact.factId) : [];
  const latestDec = decisions[decisions.length - 1];
  const canUndo = latestDec && latestDec.kind !== "rule_adopt";

  badge.textContent = occs.length > 1 ? `SHA-256 Verified · ${occs.length} Occurrences` : "SHA-256 Verified";

  const occHtml = occs
    .map(
      (o, idx) => `
      <div class="occ-item">
        <div>
          <div class="mono" style="font-weight:600">${esc(o.externalPath)}</div>
          <div class="file-meta mono">${esc(o.occurrenceId)} · batch ${esc(o.batchId)}</div>
        </div>
        ${
          o.storageReused
            ? `<span class="badge badge-blue">Reused #${idx + 1}</span>`
            : `<span class="badge badge-emerald">Initial</span>`
        }
      </div>
    `
    )
    .join("");

  let decisionSectionHtml = "";
  if (amtFact) {
    const decItems = decisions
      .map(
        (d) => `
        <div class="decision-item">
          <div class="decision-top">
            <span class="badge ${d.kind === "human_correct" ? "badge-blue" : d.kind === "undo" ? "badge-amber" : "badge-slate"}">${esc(d.kind)}</span>
            <span class="mono" style="color:var(--ink-muted);font-size:11px">${esc(d.decisionId)}</span>
          </div>
          <div>
            ${
              d.priorValue
                ? `USD <span class="mono">${esc(d.priorValue.amount_text)}</span> → <strong>USD <span class="mono">${esc(d.newValue.amount_text)}</span></strong>`
                : `Adopted <strong>USD <span class="mono">${esc(d.newValue.amount_text)}</span></strong> via <span class="mono">${esc(d.ruleVersion)}</span>`
            }
          </div>
        </div>
      `
      )
      .join("");

    decisionSectionHtml = `
      <div class="inspector-section">
        <h4>
          <span>Append-Only Decision Chain (${decisions.length})</span>
          <span class="mono">rev ${amtFact.revision}</span>
        </h4>
        <div style="font-size:12px;color:var(--ink-secondary);margin-bottom:8px">
          Current: <strong class="mono">USD ${esc(amtFact.value.amount_text)}</strong> · Original bytes still say <strong class="mono">USD ${esc(amtFact.originalValue.amount_text)}</strong>. Re-importing never overwrites human edits.
        </div>
        <div class="decision-list">${decItems}</div>
        <form class="correct-form" id="correct-amount-form">
          <input id="correct-amount-input" class="correct-input" type="text" placeholder="e.g. 99.00" value="${esc(amtFact.value.amount_text)}" aria-label="Corrected USD amount">
          <button class="btn btn-sm btn-primary" type="submit">Correct Amount</button>
          ${canUndo ? `<button id="btn-undo-decision" class="btn btn-sm" type="button">Undo</button>` : ""}
        </form>
      </div>
    `;
  } else {
    decisionSectionHtml = `
      <div class="inspector-section">
        <h4><span>Extraction &amp; Epistemic Boundary</span><span class="mono">${esc(source.extractionRule)}</span></h4>
        <div style="font-size:12.5px;color:var(--ink-secondary)">
          ${
            source.extractionNote
              ? esc(source.extractionNote)
              : `No <code>stated_amount_paid</code> fact was auto-adopted. Non-payment totals (order confirmations, credit notes, CSV export rows, or masked notices) stay visible as un-adopted candidates until a human decides.`
          }
        </div>
      </div>
    `;
  }

  let maskedAssignHtml = "";
  if (source.originalAccountEmail && source.originalAccountEmail.includes("*")) {
    const acctDecs = store.getAccountDecisionsForSource(source.sourceId);
    const lastAcct = acctDecs[acctDecs.length - 1];
    const canUndoAcct = lastAcct && lastAcct.kind !== "undo";
    const knownMerchantEmails = [...new Set(
      store.sources
        .filter((s) => s.merchant === source.merchant && s.accountEmail && !s.accountEmail.includes("*") && s.accountEmail !== "Not stated")
        .map((s) => s.accountEmail)
    )];
    const presetBtns = knownMerchantEmails
      .map(
        (email) => `<button class="btn btn-sm btn-assign-preset" data-email="${esc(email)}" type="button">Assign → ${esc(email)}</button>`
      )
      .join("");
    maskedAssignHtml = `
      <div class="inspector-section">
        <h4>
          <span>Masked Account Attribution (${acctDecs.length})</span>
          <span class="mono">${source.accountEmail.includes("*") ? "Unassigned" : "Human Attributed"}</span>
        </h4>
        <div style="font-size:12px;color:var(--ink-secondary);margin-bottom:8px">
          Current: <strong class="mono">${esc(source.accountEmail)}</strong> · Original notice still says <strong class="mono">${esc(source.originalAccountEmail)}</strong>.
        </div>
        ${presetBtns ? `<div style="display:flex;gap:6px;flex-wrap:wrap;margin-bottom:8px">${presetBtns}</div>` : ""}
        <form class="correct-form" id="assign-account-form">
          <input id="assign-account-input" class="correct-input" style="width:200px" type="email" placeholder="account@domain.example" value="${source.accountEmail.includes("*") ? "" : esc(source.accountEmail)}" aria-label="Full account email">
          <button class="btn btn-sm btn-primary" type="submit">Assign</button>
          ${canUndoAcct ? `<button id="btn-undo-account" class="btn btn-sm" type="button">Undo</button>` : ""}
        </form>
      </div>
    `;
  }

  body.innerHTML = `
    <div class="inspector-section">
      <h4>
        <span>Immutable Object Identity</span>
        <span class="mono">${source.byteLength} bytes</span>
      </h4>
      <div class="kv-grid">
        <div class="kv-k">Source ID</div><div class="kv-v mono">${esc(source.sourceId)}</div>
        <div class="kv-k">Object ID</div><div class="kv-v mono">${esc(source.objectId)}</div>
        <div class="kv-k">SHA-256</div><div class="kv-v mono">${esc(source.contentSha256)}</div>
        <div class="kv-k">Media Type</div><div class="kv-v mono">${esc(source.mediaType)} (${esc(source.role)})</div>
        <div class="kv-k">Order / Doc</div><div class="kv-v mono">${esc(source.orderId || "Not stated")}</div>
      </div>
    </div>

    <div class="inspector-section">
      <h4>
        <span>Import Occurrences (${occs.length})</span>
        <span>${occs.length > 1 ? "Byte-Identical Reused" : "Single Path"}</span>
      </h4>
      <div class="occ-list">${occHtml}</div>
    </div>

    <div class="inspector-section">
      <h4>
        <span>Untouched Original Source (Static View)</span>
        <span>Never Executed</span>
      </h4>
      <pre class="source-preview">${highlightSourceContent(obj.rawContent, source.candidates)}</pre>
    </div>

    ${decisionSectionHtml}
    ${maskedAssignHtml}
  `;

  const form = document.getElementById("correct-amount-form");
  if (form && amtFact) {
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      const val = document.getElementById("correct-amount-input").value;
      try {
        store.correctAmount(amtFact.factId, val);
        renderAll();
        showToast(`Recorded human correction USD ${val.trim()} on ${source.displayName} — original bytes untouched.`, false);
      } catch (err) {
        showToast(err.message, true);
      }
    });
  }

  const undoBtn = document.getElementById("btn-undo-decision");
  if (undoBtn && amtFact) {
    undoBtn.addEventListener("click", () => {
      try {
        store.undoLastDecision(amtFact.factId);
        renderAll();
        showToast(`Appended compensating undo decision on ${source.displayName}.`, false);
      } catch (err) {
        showToast(err.message, true);
      }
    });
  }

  for (const btn of body.querySelectorAll(".btn-assign-preset")) {
    btn.addEventListener("click", () => {
      try {
        const email = btn.dataset.email;
        const label = /jordan|work/i.test(email) ? "Work (Attributed)" : "Personal (Attributed)";
        store.assignMaskedAccount(source.sourceId, email, label);
        renderAll();
        showToast(`Attributed masked notice to ${email} — original bytes still show ${source.originalAccountEmail}.`, false);
      } catch (err) {
        showToast(err.message, true);
      }
    });
  }

  const assignForm = document.getElementById("assign-account-form");
  if (assignForm) {
    assignForm.addEventListener("submit", (e) => {
      e.preventDefault();
      const email = document.getElementById("assign-account-input").value;
      try {
        store.assignMaskedAccount(source.sourceId, email, "Human Attributed");
        renderAll();
        showToast(`Attributed masked notice to ${email.trim()} — original bytes untouched.`, false);
      } catch (err) {
        showToast(err.message, true);
      }
    });
  }

  const undoAcctBtn = document.getElementById("btn-undo-account");
  if (undoAcctBtn) {
    undoAcctBtn.addEventListener("click", () => {
      try {
        store.undoAccountAssignment(source.sourceId);
        renderAll();
        showToast(`Undid account attribution on ${source.displayName} — restored ${source.accountEmail}.`, false);
      } catch (err) {
        showToast(err.message, true);
      }
    });
  }
}

function renderAudit() {
  const root = document.getElementById("view-audit");
  const audit = store.getAuditReport();

  const dupHtml =
    audit.duplicates.length === 0
      ? `<p style="color:var(--ink-secondary);font-size:13px">No duplicate imports recorded yet. Click <strong>+ Import Sample Batch</strong> or <strong>+ Renamed Duplicate Receipt</strong> to verify SHA-256 reuse.</p>`
      : audit.duplicates
          .map(
            ({ source, occurrences }) => `
            <div style="border:1px solid var(--emerald-border);background:var(--emerald-bg);border-radius:10px;padding:12px 14px;margin-bottom:10px">
              <div style="display:flex;justify-content:space-between;align-items:center;gap:8px;flex-wrap:wrap;margin-bottom:6px">
                <strong>${esc(source.merchant)} — ${esc(source.displayName)}</strong>
                <span class="badge badge-emerald">${occurrences.length} paths → 1 object (${esc(source.objectId)})</span>
              </div>
              <div class="mono" style="font-size:11.5px;color:#065f46;margin-bottom:8px;word-break:break-all">SHA-256: ${esc(source.contentSha256)}</div>
              <div class="occ-list">
                ${occurrences
                  .map(
                    (o, i) => `
                    <div class="occ-item">
                      <span class="mono">${esc(o.externalPath)}</span>
                      <span class="badge ${o.storageReused ? "badge-blue" : "badge-emerald"}">${o.storageReused ? `Duplicate #${i + 1} Reused` : "Original #1"}</span>
                    </div>
                  `
                  )
                  .join("")}
              </div>
            </div>
          `
          )
          .join("");

  const conflictHtml =
    audit.conflicts.length === 0
      ? `<p style="color:var(--ink-secondary);font-size:13px">No cross-source amount conflicts detected in the current vault.</p>`
      : audit.conflicts
          .map(
            (c) => `
            <div style="margin-bottom:12px">
              <div style="display:flex;justify-content:space-between;align-items:center;gap:8px;margin-bottom:4px">
                <strong>${esc(c.merchant)} · Order <span class="mono">${esc(c.orderId)}</span></strong>
                <span class="badge badge-rose">Refused Auto-Pick</span>
              </div>
              <div style="font-size:12.5px;color:var(--ink-secondary)">${esc(c.note)}</div>
              <div class="compare-box">
                <div class="compare-col good">
                  <div class="compare-label">Invoice PDF (${esc(c.invoiceSource.displayName)})</div>
                  <div class="compare-val mono">USD ${esc(c.invoiceAmount)}</div>
                  <div style="font-size:12px;color:var(--ink-secondary)">Adopted as <code>stated_amount_paid</code> (tax-inclusive invoice)</div>
                </div>
                <div class="compare-col warn">
                  <div class="compare-label">Store CSV (${esc(c.csvSource.displayName)})</div>
                  <div class="compare-val mono">USD ${esc(c.csvAmount)}</div>
                  <div style="font-size:12px;color:var(--ink-secondary)">Held as un-adopted <code>csv_charged_amount</code> candidate</div>
                </div>
              </div>
              <div style="margin-top:8px">
                <button class="btn btn-sm btn-jump-source" data-source-id="${esc(c.invoiceSource.sourceId)}" type="button">Inspect &amp; Resolve Invoice →</button>
              </div>
            </div>
          `
          )
          .join("");

  const sepHtml =
    audit.separatedAccounts.length === 0
      ? `<p style="color:var(--ink-secondary);font-size:13px">No multi-account same-vendor splits yet.</p>`
      : audit.separatedAccounts
          .map(
            (item) => `
            <div class="occ-item" style="margin-bottom:8px">
              <div>
                <strong>${esc(item.merchant)}</strong>
                <div class="file-meta">${item.emails.map((e) => esc(e)).join("  vs.  ")}</div>
              </div>
              <span class="badge badge-emerald">${item.emails.length} Separate Accounts</span>
            </div>
          `
          )
          .join("");

  const maskedHtml =
    audit.maskedUnassigned.length === 0
      ? `<p style="color:var(--ink-secondary);font-size:13px">No masked account notices present.</p>`
      : audit.maskedUnassigned
          .map(
            (s) => `
            <div class="compare-col warn" style="margin-bottom:8px">
              <div class="compare-label">${esc(s.merchant)} · ${esc(s.displayName)}</div>
              <div style="font-weight:700;margin-bottom:3px">Masked email: <span class="mono">${esc(s.accountEmail)}</span></div>
              <div style="font-size:12px;color:var(--ink-secondary)">
                Auto-renewal cancelled; still usable through <span class="mono">${esc(s.metadata?.usableThrough || "stated date")}</span>. Held unassigned — SubVault refuses to guess ownership by initial letter or vendor name.
              </div>
              <div style="margin-top:8px">
                <button class="btn btn-sm btn-jump-source" data-source-id="${esc(s.sourceId)}" type="button">Assign Account in Inspector →</button>
              </div>
            </div>
          `
          )
          .join("");

  const nonPaidHtml = audit.nonPaymentTotals
    .map((s) => {
      const cand = s.candidates.find((c) => c.kind === "credit_note_total" || c.kind === "unjudged_order_total");
      return `
        <div class="occ-item" style="margin-bottom:6px">
          <div>
            <strong>${esc(s.displayName)}</strong>
            <div class="file-meta">${esc(cand?.displayExcerpt || "")} — kept separate from Amount Paid</div>
          </div>
          <span class="badge badge-amber">${esc((cand?.kind || "total").replace(/_/g, " "))}</span>
        </div>
      `;
    })
    .join("");

  root.innerHTML = `
    <div class="audit-grid">
      <section class="audit-card">
        <div class="audit-card-head">
          <div>
            <h3>Byte-Identical Duplicate Detection (SHA-256)</h3>
            <p>Path and filename are never identity. Identical bytes reuse one <code>obj_*</code> file and record new occurrences without duplicating spend or clobbering edits.</p>
          </div>
          <button id="audit-reimport-dup" class="btn btn-sm btn-emerald" type="button">+ Test Re-Import</button>
        </div>
        ${dupHtml}
      </section>

      <section class="audit-card">
        <div class="audit-card-head">
          <div>
            <h3>Cross-Source Amount Conflicts</h3>
            <p>When a store CSV and a tax invoice disagree on the same Order ID, both originals remain preserved side-by-side.</p>
          </div>
          <span class="badge badge-rose">${audit.conflicts.length} Active</span>
        </div>
        ${conflictHtml}
      </section>

      <section class="audit-card">
        <div class="audit-card-head">
          <div>
            <h3>Same-Merchant Personal vs. Work Separation</h3>
            <p>Identical vendor, amount ($107.91), and service period remain distinct when account emails differ.</p>
          </div>
          <span class="badge badge-emerald">Strict Isolation</span>
        </div>
        ${sepHtml}
        <div style="margin-top:14px">
          <div class="compare-label" style="margin-bottom:6px">Non-Payment Totals (Credit Notes &amp; Order Confirmations)</div>
          ${nonPaidHtml || `<p style="color:var(--ink-muted);font-size:12px">None in vault.</p>`}
        </div>
      </section>

      <section class="audit-card">
        <div class="audit-card-head">
          <div>
            <h3>Masked Accounts &amp; Cancelled-But-Usable Renewals</h3>
            <p>Cancelling auto-renewal leaves service usable through the stated date; masked addresses stay unassigned until a human decides.</p>
          </div>
          <span class="badge badge-amber">${audit.maskedUnassigned.length} Unassigned</span>
        </div>
        ${maskedHtml}
      </section>
    </div>
  `;

  const reimportBtn = document.getElementById("audit-reimport-dup");
  if (reimportBtn) {
    reimportBtn.addEventListener("click", handleImportSampleBatch);
  }
  for (const btn of root.querySelectorAll(".btn-jump-source")) {
    btn.addEventListener("click", () => {
      state.selectedSourceId = btn.dataset.sourceId;
      setView("ledger");
    });
  }
}

function renderExport() {
  const root = document.getElementById("view-export");
  const pkg = store.buildExitPackage();

  const fileRows = pkg.manifest.files
    .map(
      (f) => `
      <div class="manifest-row">
        <div>
          <div class="mono" style="font-weight:600">${esc(f.path)}</div>
          <div class="file-meta mono">sha256: ${esc(f.sha256)}</div>
        </div>
        <span class="badge badge-slate mono">${f.byte_length} B</span>
      </div>
    `
    )
    .join("");

  root.innerHTML = `
    <div class="export-grid">
      <section class="panel">
        <div class="panel-header">
          <div>
            <h3 class="panel-title">Read-Only Review Sheet (<code>subvault review</code>)</h3>
            <div class="brand-sub">Matches <code>src/review.rs</code> — conclusions grouped by source, explicit epistemic disclaimers</div>
          </div>
          <button id="dl-review" class="btn btn-sm" type="button">Download review.txt</button>
        </div>
        <pre class="review-pre">${esc(pkg.reviewTxt)}</pre>
      </section>

      <section class="panel">
        <div class="panel-header">
          <div>
            <h3 class="panel-title">Verifiable Offline Exit Package (<code>offline-exit-1</code>)</h3>
            <div class="brand-sub mono">snapshot_id: ${esc(pkg.snapshotId.slice(0, 20))}… · completeness: complete</div>
          </div>
          <span class="badge badge-emerald">Zero Lock-In</span>
        </div>
        <div style="padding:12px 16px;border-bottom:1px solid var(--border);display:flex;gap:8px;flex-wrap:wrap;background:var(--surface-subtle)">
          <button id="dl-zip" class="btn btn-sm btn-emerald" type="button">Download Full Exit Pack (.zip)</button>
          <button id="dl-csv" class="btn btn-sm btn-primary" type="button">Download facts.csv</button>
          <button id="dl-records" class="btn btn-sm" type="button">Download records.json</button>
          <button id="dl-manifest" class="btn btn-sm" type="button">Download manifest.json</button>
        </div>
        <div class="manifest-list" style="max-height:460px;overflow-y:auto">
          ${fileRows}
        </div>
      </section>
    </div>
  `;

  document.getElementById("dl-zip").addEventListener("click", () => {
    const { zipBytes, filename } = store.buildExitZip();
    downloadBinaryFile(filename, zipBytes, "application/zip");
    showToast(`Downloaded ${filename} (includes originals/, records.json, facts.csv, manifest.json — verifiable with 'subvault verify-package').`, false);
  });
  document.getElementById("dl-review").addEventListener("click", () =>
    downloadTextFile("subvault-review.txt", pkg.reviewTxt, "text/plain;charset=utf-8")
  );
  document.getElementById("dl-csv").addEventListener("click", () =>
    downloadTextFile("facts.csv", pkg.factsCsv, "text/csv;charset=utf-8")
  );
  document.getElementById("dl-records").addEventListener("click", () =>
    downloadTextFile("records.json", pkg.recordsJson, "application/json;charset=utf-8")
  );
  document.getElementById("dl-manifest").addEventListener("click", () =>
    downloadTextFile("manifest.json", pkg.manifestJson, "application/json;charset=utf-8")
  );
}

function renderActivityLog() {
  const el = document.getElementById("activity-log");
  if (store.activityLog.length === 0) {
    el.innerHTML = `<div class="log-line">No commands executed yet.</div>`;
    return;
  }
  el.innerHTML = store.activityLog
    .slice(0, 12)
    .map((entry) => {
      const badgeCls =
        entry.type === "duplicate_reused"
          ? "badge-blue"
          : entry.type === "human_correct"
          ? "badge-amber"
          : entry.type === "undo"
          ? "badge-slate"
          : "badge-emerald";
      return `
        <div class="log-line">
          <span class="badge ${badgeCls}">${esc(entry.type)}</span>
          <span class="mono" style="color:var(--ink-muted);font-size:11px">${esc(entry.commandId)}</span>
          <span>${esc(entry.message)}</span>
        </div>
      `;
    })
    .join("");
}

function updateModeChip() {
  const chip = document.getElementById("mode-chip");
  if (!chip) return;
  if (store.mode === "native_export") {
    const nv = store.nativeVerification;
    chip.textContent = nv?.verified
      ? `Native CLI Export · ${nv.verifiedFiles} Files SHA-256 Verified`
      : nv?.hasManifest
      ? `Native CLI Export · ${nv.defects.length} Integrity Defect(s)`
      : "Native CLI Export · records.json Loaded";
  } else {
    chip.textContent = "Interactive Demo · Pure Static";
  }
}

function renderAll() {
  updateModeChip();
  renderKpis();
  if (state.view === "ledger") renderLedger();
  if (state.view === "audit") renderAudit();
  if (state.view === "export") renderExport();
  renderActivityLog();
  persistVault();
}

function handleImportSampleBatch() {
  const tag = `b_demo_${state.importCounter++}`;
  const res = store.loadDemoCorpus(tag);
  if (!state.selectedSourceId && store.sources.length > 0) {
    state.selectedSourceId = store.sources[0].sourceId;
  }
  renderAll();
  if (res.added === 0) {
    showToast(
      `Re-imported all ${res.total} sample files: 0 new objects created, ${res.reused} byte-identical objects reused via SHA-256, and all prior human edits stayed intact.`,
      false
    );
  } else {
    showToast(
      `Imported sample batch (${res.total} files): preserved ${res.added} unique SHA-256 objects and automatically deduplicated ${res.reused} byte-identical copy.`,
      false
    );
  }
}

async function handleNativeExportSelection(fileList) {
  try {
    const fileMap = new Map();
    for (const file of fileList) {
      const rel = file.webkitRelativePath || file.name;
      const buf = await file.arrayBuffer();
      fileMap.set(rel, new Uint8Array(buf));
    }
    const verdict = store.loadNativeExitPackage(fileMap);
    state.selectedSourceId = store.sources[0]?.sourceId || null;
    renderAll();
    if (verdict.hasManifest && verdict.verified) {
      showToast(
        `Verified native CLI export (snapshot ${verdict.snapshotId.slice(0, 12)}…): ${verdict.verifiedFiles} file(s) passed SHA-256 checks.`,
        false
      );
    } else if (verdict.hasManifest) {
      showToast(
        `Loaded native export with ${verdict.defects.length} integrity defect(s): ${verdict.defects.map((d) => `${d.path} (${d.reason})`).join(", ")}`,
        true
      );
    } else {
      showToast(
        `Loaded records.json (${store.sources.length} sources). Select the full export folder including manifest.json to verify SHA-256 hashes.`,
        true
      );
    }
  } catch (err) {
    showToast(`Could not load native export: ${err.message}`, true);
  }
}

async function handleUploadedFiles(fileList) {
  const files = [...fileList];
  if (files.some((f) => f.name === "records.json" || f.name === "manifest.json")) {
    return handleNativeExportSelection(files);
  }
  let added = 0;
  let reused = 0;
  for (const file of files) {
    const content = await file.text();
    const lower = file.name.toLowerCase();
    const mediaType = lower.endsWith(".html") || lower.endsWith(".htm")
      ? "text/html;charset=utf-8"
      : lower.endsWith(".csv")
      ? "text/csv"
      : lower.endsWith(".pdf")
      ? "application/pdf"
      : "text/plain;charset=utf-8";

    const res = store.ingestDocument({
      displayName: file.name,
      externalPath: `local/${file.name}`,
      mediaType,
      content,
      batchId: `b_upload_${state.importCounter++}`
    });
    state.selectedSourceId = res.source.sourceId;
    if (res.storageReused) reused++;
    else added++;
  }
  renderAll();
  showToast(
    `Processed ${fileList.length} local file(s) in-browser: ${added} new preserved object(s), ${reused} byte-identical duplicate(s) reused.`,
    false
  );
}

function wireEvents() {
  document.getElementById("btn-import-sample").addEventListener("click", handleImportSampleBatch);
  document.getElementById("btn-go-export").addEventListener("click", () => setView("export"));
  document.getElementById("status-toast-close").addEventListener("click", hideToast);

  document.getElementById("btn-reset-vault").addEventListener("click", () => {
    store.reset();
    clearPersistedVault();
    state.selectedSourceId = null;
    renderAll();
    showToast("Demo vault & local browser cache cleared. Click '+ Import Sample Batch' to reload preloaded bills from scratch.", true);
  });

  document.getElementById("file-upload-input").addEventListener("change", (e) => {
    if (e.target.files?.length) {
      handleUploadedFiles(e.target.files);
      e.target.value = "";
    }
  });

  document.getElementById("native-export-input").addEventListener("change", (e) => {
    if (e.target.files?.length) {
      handleNativeExportSelection(e.target.files);
      e.target.value = "";
    }
  });

  for (const btn of document.querySelectorAll(".tab-btn")) {
    btn.addEventListener("click", () => setView(btn.dataset.view));
  }

  for (const pill of document.querySelectorAll(".pill-btn")) {
    pill.addEventListener("click", () => {
      state.filter = pill.dataset.filter;
      for (const p of document.querySelectorAll(".pill-btn")) {
        p.classList.toggle("active", p === pill);
      }
      renderLedger();
    });
  }

  document.getElementById("search-input").addEventListener("input", (e) => {
    state.search = e.target.value;
    renderLedger();
  });

  const dropZone = document.getElementById("drop-zone");
  dropZone.addEventListener("dragover", (e) => {
    e.preventDefault();
    dropZone.classList.add("drop-active");
  });
  dropZone.addEventListener("dragleave", () => {
    dropZone.classList.remove("drop-active");
  });
  dropZone.addEventListener("drop", (e) => {
    e.preventDefault();
    dropZone.classList.remove("drop-active");
    if (e.dataTransfer?.files?.length) {
      handleUploadedFiles(e.dataTransfer.files);
    }
  });
}

// --- Boot ---
initQuickSamples();
wireEvents();

const params = new URLSearchParams(window.location.search);
const hasDeterministicParams = params.has("view") || params.has("select") || params.has("demoEdit");
state.persistEnabled = !hasDeterministicParams;

// Restore saved local browser vault if present (except on explicit query-param demo links)
const restored = state.persistEnabled && restorePersistedVault();
if (!restored) {
  // Preload the full 12-file synthetic corpus out-of-the-box!
  store.loadDemoCorpus("b_demo_initial");
}

// Select September receipt (demonstrates SHA-256 duplicate reuse + decision chain right away)
const selectName = params.get("select");
if (selectName) {
  const found = store.sources.find((s) => s.displayName === selectName);
  state.selectedSourceId = found?.sourceId || store.sources[0]?.sourceId || null;
} else {
  state.selectedSourceId = store.sources[0]?.sourceId || null;
}

if (params.get("demoEdit") === "1") {
  const firstAmt = store.facts.find((f) => f.sourceId === state.selectedSourceId && f.factType === "stated_amount_paid");
  if (firstAmt) {
    store.correctAmount(firstAmt.factId, "14.99");
  }
}

const initialView = params.get("view");
if (initialView && ["ledger", "audit", "export"].includes(initialView)) {
  setView(initialView);
} else {
  renderAll();
}
