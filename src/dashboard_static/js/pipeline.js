// ─── Tab 5: Pipeline ─────────────────────────────────────
const GATE_META = {
  filter:   { accent: '#93c043', desc: 'Strips MCP tool schemas not matching the active profile. Biggest token saver.' },
  auto:     { accent: '#60a5fa', desc: 'Reads the working directory from the system prompt and switches profile automatically.' },
  inject:   { accent: '#fbbf24', desc: 'Prepends a custom system_prefix.md to every request when enabled.' },
  adaptive: { accent: '#a78bfa', desc: 'Appends adaptive_prefix.md built from your indexed session history when enabled.' },
  coach:    { accent: '#f87171', desc: 'Detects correction cascades and re-asks, injects a context hint to break the loop.' },
  behavior: { accent: '#c4b5fd', desc: 'Compares current session patterns against your history and warns early if a costly pattern is repeating.' },
  budget:   { accent: '#fb923c', desc: 'Estimates session cost and fires an AskUserQuestion hint when it crosses the threshold from ~/.ctx/config.toml (monthly_budget_usd pacing).' },
  compress: { accent: '#22d3ee', desc: 'Compresses verbose bash tool output before it hits Claude\'s context window.' },
};

async function loadPipeline() {
  const data = await fetch(appendSince('/api/gates')).then(r => r.json()).catch(() => ({ gates: [], activity: [], sessions_fallback_note: null }));
  const noteEl = document.getElementById('gate-sessions-fallback-note');
  if (noteEl) {
    if (data.sessions_fallback_note) {
      noteEl.style.display = 'block';
      noteEl.textContent = data.sessions_fallback_note;
    } else {
      noteEl.style.display = 'none';
      noteEl.textContent = '';
    }
  }
  renderGateFeedSummary(data);
  renderGateFlow(data.gates);
  renderGateFeed(data.activity);
}

function renderGateFeedSummary(data) {
  const el = document.getElementById('gate-feed-summary');
  if (!el) return;
  const act = data.activity || [];
  if (!act.length) {
    el.textContent = 'No pipeline activity yet. Start a Claude Code session. Each prompt flows through ctx and appears here.';
    return;
  }
  let filt = 0, inj = 0, adp = 0, coach = 0, auto = 0;
  for (const a of act) {
    for (const g of a.gates) {
      if (g.id === 'filter') filt++;
      if (g.id === 'inject') inj++;
      if (g.id === 'adaptive') adp++;
      if (g.id === 'coach') coach++;
      if (g.id === 'auto') auto++;
    }
  }
  el.textContent = 'Feed: ' + act.length + ' events — profile strip ' + filt + ', static prefix ' + inj + ', adaptive ' + adp + ', coaching ' + coach + ', auto-profile ' + auto + '. Click a row for folder and session. Stacks with identical chips show a count badge.';
}

function renderGateFlow(gates) {
  const flowEl  = document.getElementById('gate-flow-wrap');
  const legendEl = document.getElementById('gate-legend');
  if (!gates.length) { flowEl.innerHTML = `<div class="empty">Gate definitions load after the first dashboard refresh. If this stays empty, reload the page.</div>`; return; }

  const cards = gates.map(g => {
    const meta  = GATE_META[g.id] || { accent: '#4a5a7a' };
    const fired = g.today_count > 0;
    const dotClass    = !g.enabled ? 'off' : fired ? 'on' : 'idle';
    const statusText  = !g.enabled ? 'Off' : fired ? 'Active' : 'Idle';
    const statusColor = !g.enabled ? 'var(--t4)' : fired ? 'var(--green)' : 'var(--amber)';
    const cardClass   = ['gate-card', !g.enabled ? 'disabled' : '', fired ? 'fired' : ''].filter(Boolean).join(' ');

    return `<div class="${cardClass}">
      <div class="gate-top-bar" style="background:linear-gradient(90deg,${meta.accent},transparent)"></div>
      <div class="gate-name">${esc(g.name)}</div>
      <div class="gate-status-row">
        <div class="gate-dot ${dotClass}"></div>
        <div class="gate-status-text" style="color:${statusColor}">${statusText}</div>
      </div>
      <div class="gate-count">${g.today_count}</div>
      <div class="gate-count-sub">req today</div>
      <div class="gate-detail">${esc(g.detail)}</div>
      ${g.today_tokens > 0 ? `<div class="gate-tokens">-${fmtK(g.today_tokens)} tok</div>` : ''}
    </div>`;
  }).join('');

  flowEl.innerHTML = `<div class="gate-flow">${cards}</div>`;

  // Render descriptions below each card in the same grid
  if (legendEl) {
    legendEl.innerHTML = gates.map(g => {
      const meta = GATE_META[g.id] || {};
      return `<div style="padding:8px 14px 0;font-size:10px;color:var(--t4);line-height:1.5">${esc(meta.desc || '')}</div>`;
    }).join('');
  }
}

function renderGateFeed(activity) {
  const el = document.getElementById('gate-feed-wrap');
  if (!activity.length) {
    el.innerHTML = `<div class="empty">No gate events yet. Use Claude Code with ctx active (<code>filter.js</code> or the proxy). Events append to analytics as requests flow.</div>`;
    return;
  }
  function chipKey(a) {
    return a.gates.map(g => g.id).slice().sort().join('|');
  }
  const groups = [];
  for (const a of activity) {
    const k = chipKey(a);
    const prev = groups[groups.length - 1];
    if (prev && prev.key === k) {
      prev.count++;
      prev.members.push(a);
    } else {
      groups.push({ key: k, count: 1, head: a, members: [a] });
    }
  }
  el.innerHTML = groups.map((g, gi) => {
    const a = g.head;
    const chips = a.gates.map(x => {
      const cls = `gf-chip gf-chip-${x.id}`;
      return `<span class="${cls}">${esc(x.label)}</span>`;
    }).join('');
    const badge = g.count > 1 ? `<span class="gf-count-badge">×${g.count}</span>` : '';
    const meta = [
      a.session_id ? '<div><strong>Session</strong> ' + esc(a.session_id) + '</div>' : '',
      a.working_directory ? '<div><strong>Folder</strong> <code>' + esc(a.working_directory) + '</code></div>' : '',
      a.profile ? '<div><strong>Profile</strong> ' + esc(a.profile) + '</div>' : '',
      a.auto_trigger ? '<div><strong>Auto trigger</strong> ' + esc(a.auto_trigger) + '</div>' : '',
    ].filter(Boolean).join('');
    const times = g.members.map(m => '<div style="font-size:11px;color:var(--t4)">' + esc(m.ts) + '</div>').join('');
    const stackTimes = g.count > 1
      ? '<div style="margin-top:8px;font-weight:600;color:var(--t3)">Timestamps in this stack</div>' + times
      : '';
    return `<div class="gf-row" id="gf-row-${gi}" onclick="toggleGfRow(${gi})">
      <div class="gf-row-head">
        <div class="gf-ts">${fmtTs(a.ts)}</div>
        ${badge}
        <div class="gf-chips">${chips}</div>
      </div>
      <div class="gf-detail" id="gf-det-${gi}">
        ${meta}
        ${stackTimes}
      </div>
    </div>`;
  }).join('');
}
function toggleGfRow(i) {
  const row = document.getElementById('gf-row-' + i);
  if (row) row.classList.toggle('expanded');
}

