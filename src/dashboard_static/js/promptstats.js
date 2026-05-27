// ─── Tab 2: Prompt Stats ──────────────────────────────────
let _monthlyData = [], _budgetUsd = null, _actualUsd = null, _allSessions = [];

async function loadPromptStats() {
  const [monthly, tips, allSess, dashStats] = await Promise.all([
    fetch(appendSince('/api/spend/monthly')).then(r => r.json()),
    fetch(appendSince('/api/spend/tips')).then(r => r.json()),
    fetch(appendSince('/api/spend/sessions')).then(r => r.json()),
    fetch(appendSince('/api/stats')).then(r => r.json()).catch(() => ({})),
  ]);
  _monthlyData = monthly;
  _allSessions = allSess;

  if (monthly.length) {
    if (monthly[0].budget_usd != null) _budgetUsd = monthly[0].budget_usd;
    if (monthly[0].actual_spend_usd != null) _actualUsd = monthly[0].actual_spend_usd;
  }

  renderBudget(monthly[0] || null, dashStats);
  renderScore(allSess);
  renderInsights(tips);

  const tabsEl = document.getElementById('month-tabs');
  if (monthly.length) {
    tabsEl.innerHTML = monthly.map((m, i) =>
      `<button class="month-tab ${i===0?'active':''}" onclick="selectMonth('${m.month}',this)">${m.month}</button>`
    ).join('');
    loadSpendSessions(monthly[0].month);
  } else {
    document.getElementById('spend-sessions-table').innerHTML = `<div class="empty">No Claude conversations found.</div>`;
  }
  await loadIntelligencePanels();
  await loadTaskCosts();
}

async function loadTaskCosts() {
  const el = document.getElementById('task-costs-body');
  if (!el) return;
  try {
    const groups = await fetch('/api/task-costs').then(r => r.json());
    if (!groups.length) {
      el.innerHTML = '<div class="empty">No subagent data found. This card populates when Claude Code spawns child sessions (via the Task tool or sub-agents). Regular single-session work appears on the Prompt Stats charts above.</div>';
      return;
    }
    el.innerHTML = groups.map((g, gi) => {
      const childRows = (g.children || []).map(c =>
        `<tr><td><code>${esc(c.session_id)}</code></td><td>${fmtCost(c.cost_usd)}</td><td>${c.requests}</td><td>${fmtK(c.tokens_saved)}</td></tr>`
      ).join('');
      return `<details class="task-cost-group" style="margin-bottom:10px;border:1px solid var(--border);border-radius:8px;padding:8px 12px" ${gi < 3 ? 'open' : ''}>
        <summary style="cursor:pointer;font-size:13px;color:var(--t1)">
          <code>${esc(g.parent_session)}</code> — ${fmtCost(g.total_cost_usd)} · ${g.total_requests} requests
          <span style="font-size:11px;color:var(--t3);margin-left:8px">${esc(g.working_directory || '')}</span>
        </summary>
        <p class="section-sub" style="margin:8px 0 6px">Total cost across ${g.total_requests} requests in this task group. Click to expand child sessions.</p>
        <table style="width:100%;font-size:12px"><thead><tr><th>Session</th><th>Cost</th><th>Requests</th><th>Tokens saved</th></tr></thead><tbody>${childRows}</tbody></table>
      </details>`;
    }).join('');
  } catch (e) {
    el.innerHTML = '<div class="empty">Could not load task costs: ' + esc(String(e)) + '</div>';
  }
}

async function loadIntelligencePanels() {
  try {
    const [alerts, health, patterns] = await Promise.all([
      fetch('/api/quality-alerts').then(r => r.json()).catch(() => []),
      fetch(appendSince('/api/project-health')).then(r => r.json()).catch(() => []),
      fetch('/api/pattern-alerts').then(r => r.json()).catch(() => []),
    ]);
    const qw = document.getElementById('intel-quality-wrap');
    const qb = document.getElementById('intel-quality-body');
    if (alerts.length) {
      qw.style.display = 'block';
      qb.innerHTML = alerts.map(a =>
        `<div class="insight-card" style="margin-bottom:12px"><div class="insight-title">Pattern shift after a profile change</div><div class="insight-detail">${esc(a.recommendation)}</div></div>`
      ).join('');
    } else {
      qw.style.display = 'none';
    }
    const hw = document.getElementById('intel-health-wrap');
    const hb = document.getElementById('intel-health-body');
    if (health.length) {
      hw.style.display = 'block';
      hb.innerHTML = '<table class="proj-table"><thead><tr><th>Folder</th><th>Week</th><th>Spend</th><th>Correction rate</th></tr></thead><tbody>' +
        health.slice(0, 48).map(h =>
          `<tr><td class="path">${esc(h.working_directory)}</td><td>${esc(h.week)}</td><td>${fmtCost(h.spend_usd)}</td><td>${(h.correction_rate * 100).toFixed(0)}%</td></tr>`
        ).join('') + '</tbody></table>';
    } else {
      hw.style.display = 'none';
    }
    const pw = document.getElementById('intel-patterns-wrap');
    const pb = document.getElementById('intel-patterns-body');
    if (pw && pb) {
      pw.style.display = 'block';
      if (Array.isArray(patterns) && patterns.length) {
        pb.innerHTML = patterns.map(p =>
          `<div class="insight-card" style="margin-bottom:12px"><div class="insight-title">${esc(p.title || '')}</div><div class="insight-detail">${esc(p.detail || '')}</div></div>`
        ).join('');
      } else {
        pb.innerHTML = '<p class="section-sub" style="margin:0">No recurring cost patterns detected this month.</p>';
      }
    }
    const cw = document.getElementById('intel-clusters-wrap');
    const cb = document.getElementById('intel-clusters-body');
    const clusters = await fetch('/api/prompt-clusters').then(r => r.json()).catch(() => []);
    if (clusters.length) {
      cw.style.display = 'block';
      cb.innerHTML = '<ul style="padding-left:18px;color:var(--t2);line-height:1.6">' +
        clusters.map(c => `<li>${esc(typeof c === 'string' ? c : JSON.stringify(c))}</li>`).join('') + '</ul>';
    } else {
      cw.style.display = 'block';
      cb.innerHTML = '<p class="section-sub" style="margin:0">No clusters indexed yet. Run <code style="background:var(--surface2);padding:2px 6px;border-radius:4px">ctx ingest</code> so sessions land in SQLite; grouping UI fills in as the index grows.</p>';
    }
  } catch (e) {
    console.warn(e);
  }
}

async function regeneratePersonalProfile() {
  const r = await fetch('/api/profiles/auto', { method: 'POST' }).then(x => x.json()).catch(() => ({}));
  alert(r.ok ? 'Personal profile updated from your tool history.' : ('Could not regenerate: ' + (r.error || 'unknown')));
  loadProfiles();
}

function renderBudget(spend, dashStats) {
  dashStats = dashStats || {};
  const label = document.getElementById('budget-month-label');
  const spentEl = document.getElementById('budget-spent');
  const ofEl = document.getElementById('budget-of');
  const ctxEl = document.getElementById('budget-ctx');
  const ctxTxt = document.getElementById('budget-ctx-text');
  const metaEl = document.getElementById('budget-meta');
  const trackEl = document.getElementById('budget-bar-track');
  const fillEl = document.getElementById('budget-bar-fill');

  const now = new Date();
  const monthStr = spend ? spend.month : `${now.getFullYear()}-${String(now.getMonth()+1).padStart(2,'0')}`;
  label.textContent = monthStr;

  if (!spend) {
    spentEl.textContent = '—';
    ofEl.textContent = 'no data yet';
    trackEl.style.display = 'none';
    const th0 = dashStats.session_budget_threshold_usd;
    const projA = dashStats.monthly_burn_projection_usd;
    if (th0 != null || projA != null) {
      const bits = [];
      if (th0 != null) bits.push(`Session budget guard near <strong>${fmtCost(th0)}</strong> estimated session spend.`);
      if (projA != null) bits.push(`Analytics month-end pace: <strong>${fmtCost(projA)}</strong>.`);
      metaEl.innerHTML = `<span style="color:var(--t4)">${bits.join(' ')}</span>`;
    } else {
      metaEl.innerHTML = '';
    }
    return;
  }

  // If Anthropic billing snapshot exists, add session delta since the snapshot
  // was taken. The baseline is stored server-side so it survives page reloads.
  const sessionTotal = spend.total_usd;
  const baseline = spend.actual_spend_baseline_usd;
  const delta = (spend.actual_spend_usd != null && baseline != null)
    ? Math.max(0, sessionTotal - baseline) : 0;
  const liveSpend = (spend.actual_spend_usd != null) ? spend.actual_spend_usd + delta : sessionTotal;

  const spendSource = (spend.actual_spend_usd != null)
    ? `Anthropic billing ${fmtCost(spend.actual_spend_usd)} + ${fmtCost(delta)} new (updates every 30s)`
    : 'from sessions (updates every 30s)';

  animateNum(spentEl, liveSpend, '$', '', 2);

  if (_budgetUsd) {
    const pct = Math.min(100, (liveSpend / _budgetUsd) * 100);
    const fillColor = pct < 70 ? '#22c55e' : pct < 90 ? '#f59e0b' : '#ef4444';
    ofEl.textContent = `of ${fmtCost(_budgetUsd)} budget`;
    trackEl.style.display = 'block';
    fillEl.style.background = fillColor;
    requestAnimationFrame(() => requestAnimationFrame(() => {
      fillEl.style.width = pct + '%';
    }));

    const daysInMonth = new Date(now.getFullYear(), now.getMonth()+1, 0).getDate();
    const day = now.getDate();
    const daysLeft = daysInMonth - day;
    const burnRate = liveSpend / day;
    const projected = burnRate * daysInMonth;
    const onTrack = projected <= _budgetUsd;

    metaEl.innerHTML = `
      <span>${pct.toFixed(0)}% used (${spendSource})</span>
      <span>${onTrack ? '✓' : '⚠'} ${daysLeft}d left. Projected ${fmtCost(projected)} (${onTrack ? 'on track' : 'over budget'})</span>`;
    const th = dashStats.session_budget_threshold_usd;
    const projA = dashStats.monthly_burn_projection_usd;
    if (th != null || projA != null) {
      const bits = [];
      if (th != null) bits.push(`Session budget guard near <strong>${fmtCost(th)}</strong> estimated session spend (blended rate, before cache discounts).`);
      if (projA != null) bits.push(`Analytics-based month-end pace: <strong>${fmtCost(projA)}</strong> (from filtered-request cost in the current month).`);
      metaEl.innerHTML += `<div style="width:100%;margin-top:8px;font-size:11px;color:var(--t4);line-height:1.5">${bits.join(' ')}</div>`;
    }
  } else {
    ofEl.textContent = `across ${spend.sessions} sessions this month (${spendSource})`;
    let extra = '<span style="color:var(--t4)">Set a budget to see burn rate and projections.</span>';
    const th0 = dashStats.session_budget_threshold_usd;
    const projA = dashStats.monthly_burn_projection_usd;
    if (th0 != null || projA != null) {
      const bits = [];
      if (th0 != null) bits.push(`Session budget guard near <strong>${fmtCost(th0)}</strong> estimated session spend.`);
      if (projA != null) bits.push(`Analytics month-end pace: <strong>${fmtCost(projA)}</strong>.`);
      extra = `<span style="color:var(--t4)">${bits.join(' ')}</span>`;
    }
    metaEl.innerHTML = extra;
    trackEl.style.display = 'none';
  }

  if (spend.ctx_saved_usd > 0.01) {
    ctxEl.style.display = 'flex';
    ctxTxt.textContent = `ctx saved ${fmtCost(spend.ctx_saved_usd)} this month. Without it you'd have spent ${fmtCost(liveSpend + spend.ctx_saved_usd)}`;
  }
}

function renderInsights(tips) {
  const el = document.getElementById('insights-grid');
  if (!tips.length) {
    el.innerHTML = `<div style="font-size:13px;color:var(--t3);padding:16px 0">Not enough session data yet. Keep using Claude Code and revisit after a few more sessions.</div>`;
    return;
  }
  el.innerHTML = tips.map((t, i) => `
    <div class="insight-card ${t.kind}">
      <div class="insight-number">0${i+1}</div>
      <div class="insight-title">${t.title}</div>
      <div class="insight-detail">${t.detail}</div>
      <div class="insight-saving">
        <span>💰</span> Potential saving: ${fmtCost(t.value * .3)}–${fmtCost(t.value * .6)}/mo
      </div>
    </div>
  `).join('');
}

let _selectedMonth = '';
let _openTraceSession = null;
let _selectedTurnIdx = {};

function selectMonth(month, el) {
  document.querySelectorAll('.month-tab').forEach(t => t.classList.remove('active'));
  el.classList.add('active');
  _selectedMonth = month;
  loadSpendSessions(month);
}

function flagColor(f) {
  const map = { correction:'#ef4444', clarification:'#f59e0b', long_dump:'#fb923c', pre_compact:'#a78bfa', opus:'#6366f1' };
  return map[f] || '#4a5a7a';
}

function renderTrace(s, si) {
  if (!s.top_turns || !s.top_turns.length) return `<div class="td-empty">No expensive turns flagged.</div>`;

  const maxTurnCost = Math.max(...s.top_turns.map(t => t.cost_usd));

  return `<div class="trace-viewer">
    <div class="trace-list">
      ${s.top_turns.map((t, i) => `
        <div class="trace-list-item ${i===0?'active':''}" id="tli-${si}-${i}" onclick="selectTurn(${si},${i})">
          <div class="tl-num">Turn ${t.turn_index + 1} ${t.flags.map(f => `<span style="font-size:9px;padding:1px 5px;border-radius:10px;background:${flagColor(f)}22;color:${flagColor(f)}">${f}</span>`).join(' ')}</div>
          <div class="tl-cost">${fmtCost(t.cost_usd)}</div>
          <div class="tl-bar"><div class="tl-bar-fill" style="width:${(t.cost_usd/maxTurnCost*100).toFixed(0)}%"></div></div>
        </div>
      `).join('')}
    </div>
    <div class="turn-detail" id="td-${si}">
      ${renderTurnDetail(s.top_turns[0])}
    </div>
  </div>`;
}

function renderTurnDetail(t) {
  if (!t) return '<div class="td-empty">Select a turn.</div>';
  return `
    <div class="td-flags">${t.flags.map(f => `<span class="chip flag-${f}">${f.replace(/_/g,' ')}</span>`).join('')}</div>
    <div class="td-prompt">${esc(t.human_text)}${t.human_text.length >= 1000 ? '\n…' : ''}</div>
    ${t.tip ? `<div class="td-tip">${esc(t.tip)}</div>` : ''}
  `;
}

function selectTurn(si, ti) {
  const session = window._lastSessions ? window._lastSessions[si] : null;
  // Update active state in list
  document.querySelectorAll(`[id^="tli-${si}-"]`).forEach(el => el.classList.remove('active'));
  const item = document.getElementById(`tli-${si}-${ti}`);
  if (item) item.classList.add('active');
  // Update detail panel
  const detailEl = document.getElementById(`td-${si}`);
  if (!detailEl) return;
  // Re-fetch session data from DOM (stored in closure via renderTrace)
  // We need the turn data - find it from the expand panel
  const rows = document.querySelectorAll('.expand-panel');
  // Parse from the trace list items
  // Actually, we need to store session data globally
  if (window._spendSessions && window._spendSessions[si]) {
    const turn = window._spendSessions[si].top_turns[ti];
    if (turn) detailEl.innerHTML = renderTurnDetail(turn);
  }
}

// Store sessions for selectTurn
async function loadSpendSessions(month) {
  const el = document.getElementById('spend-sessions-table');
  el.innerHTML = `<div class="empty">Loading...</div>`;
  const url = month
    ? appendSince(`/api/spend/sessions?month=${encodeURIComponent(month)}`)
    : appendSince('/api/spend/sessions');
  const sessions = await fetch(url).then(r => r.json());
  window._spendSessions = sessions;
  if (!sessions.length) { el.innerHTML = `<div class="empty">No sessions for ${month}.</div>`; return; }

  const maxCost = Math.max(...sessions.map(s => s.total_usd));
  el.innerHTML = `<table class="tbl tbl-clickable" id="spend-tbl">
    <thead><tr>
      <th>Date</th><th>Project</th><th>Turns</th><th>Model</th><th>Cost</th>
    </tr></thead>
    <tbody>
    ${sessions.map((s, i) => `
      <tr onclick="toggleTrace(${i})" style="transition:background .1s">
        <td style="color:var(--t2)">${fmtDate(s.started_at)}</td>
        <td style="color:var(--t1);font-weight:600">${esc(s.project)}</td>
        <td>${s.turn_count}${s.hit_compact?`<span title="Hit context limit" style="color:var(--purple);margin-left:4px">⚠</span>`:''}
        </td>
        <td>${s.models_used.map(m=>`<span class="chip chip-${m==='opus'?'blue':m==='haiku'?'amber':'green'}" style="font-size:10px">${m}</span>`).join(' ')}</td>
        <td style="color:var(--green);font-weight:700;font-size:14px">${fmtCost(s.total_usd)}</td>
      </tr>
      <tr class="expand-panel" id="exp-${i}">
        <td colspan="5" style="padding:0;background:var(--bg);border-top:2px solid var(--border)">
          <div style="padding:16px 18px">${renderTrace(s,i)}</div>
        </td>
      </tr>
    `).join('')}
    </tbody>
  </table>`;
}

function toggleTrace(i) {
  const panel = document.getElementById(`exp-${i}`);
  if (!panel) return;
  const isOpen = panel.classList.contains('open');
  // Close all
  document.querySelectorAll('.expand-panel.open').forEach(p => p.classList.remove('open'));
  if (!isOpen) panel.classList.add('open');
}

// ─── Budget modal ─────────────────────────────────────────
function openBudgetModal() {
  if (_budgetUsd) document.getElementById('budget-input').value = _budgetUsd;
  if (_actualUsd) document.getElementById('actual-spend-input').value = _actualUsd;
  document.getElementById('budget-modal').classList.add('open');
}
function closeBudgetModal() { document.getElementById('budget-modal').classList.remove('open'); }
async function saveBudget() {
  const budgetVal = parseFloat(document.getElementById('budget-input').value);
  const actualVal = parseFloat(document.getElementById('actual-spend-input').value);
  if (!budgetVal || budgetVal <= 0) return;
  const body = { budget_usd: budgetVal };
  if (!isNaN(actualVal) && actualVal > 0) body.actual_usd = actualVal;
  const resp = await fetch('/api/budget',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
  const data = await resp.json();
  _budgetUsd = data.monthly_budget_usd;
  _actualUsd = data.monthly_actual_spend_usd ?? null;
  closeBudgetModal();
  renderBudget(_monthlyData[0] || null);
}

