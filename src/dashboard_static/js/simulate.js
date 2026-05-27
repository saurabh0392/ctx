// ─── Tab 8: Simulate ──────────────────────────────────────

async function loadSimulateTab() {
  const profileSel = document.getElementById('sim-profile');
  if (!profileSel) return;
  try {
    const profs = await fetch('/api/profiles').then(r => r.json());
    profileSel.innerHTML = '<option value="">(auto)</option>' +
      (profs || []).map(p => `<option value="${esc(p.slug)}">${esc(p.display || p.slug)}</option>`).join('');
  } catch (_) {}
  const hookTraces = await fetch('/api/hook-traces?limit=1').then(r => r.json()).catch(() => []);
  if (hookTraces.length) {
    const last = hookTraces[0];
    if (!document.getElementById('sim-cwd').value && last.working_directory) {
      document.getElementById('sim-cwd').value = last.working_directory;
    }
    if (!document.getElementById('sim-prompt').value && last.human_text_prefix) {
      document.getElementById('sim-prompt').value = last.human_text_prefix;
    }
  }
}

async function runSimulate() {
  const body = {
    prompt: document.getElementById('sim-prompt').value,
    cwd: document.getElementById('sim-cwd').value || '.',
    profile: document.getElementById('sim-profile').value || null,
  };
  const el = document.getElementById('sim-result');
  el.style.display = 'block';
  el.innerHTML = '<div class="empty">Running simulation...</div>';
  try {
    const resp = await fetch('/api/simulate', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }).then(r => r.json());
    if (resp.result) {
      el.innerHTML = renderSimResult(resp.result);
    } else {
      el.innerHTML = '<div class="empty">No result returned.</div>';
    }
  } catch (e) {
    el.innerHTML = '<div class="empty">Simulation failed: ' + esc(String(e)) + '</div>';
  }
}

async function runSimulateAllProfiles() {
  const body = {
    prompt: document.getElementById('sim-prompt').value,
    cwd: document.getElementById('sim-cwd').value || '.',
    all_profiles: true,
  };
  const el = document.getElementById('sim-result');
  el.style.display = 'block';
  el.innerHTML = '<div class="empty">Comparing profiles...</div>';
  try {
    const resp = await fetch('/api/simulate', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }).then(r => r.json());
    if (resp.all_profiles && resp.all_profiles.length) {
      el.innerHTML = renderProfileComparison(resp.all_profiles);
    } else {
      el.innerHTML = '<div class="empty">No profiles found. Run ctx profile generate first.</div>';
    }
  } catch (e) {
    el.innerHTML = '<div class="empty">Comparison failed: ' + esc(String(e)) + '</div>';
  }
}

async function runSimulateReplay() {
  const el = document.getElementById('sim-result');
  el.style.display = 'block';
  el.innerHTML = '<div class="empty">Replaying traces...</div>';
  try {
    const traces = await fetch('/api/hook-traces?limit=10').then(r => r.json());
    if (!traces.length) {
      el.innerHTML = '<div class="empty">No hook traces found. Send a prompt through Claude Code first.</div>';
      return;
    }
    const results = [];
    for (const t of traces) {
      const resp = await fetch('/api/simulate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          prompt: t.human_text_prefix || '',
          cwd: t.working_directory || '.',
          profile: t.profile || null,
        }),
      }).then(r => r.json());
      results.push({ trace: t, sim: resp.result });
    }
    el.innerHTML = renderReplayTable(results);
  } catch (e) {
    el.innerHTML = '<div class="empty">Replay failed: ' + esc(String(e)) + '</div>';
  }
}

function renderSimResult(r) {
  const total = r.tools_kept + r.tools_removed;
  const pct = total > 0 ? Math.round(r.tools_removed / total * 100) : 0;
  const gates = [
    { name: 'Auto-Profile', fired: r.auto_selected, detail: r.auto_selected ? 'matched ' + esc(r.auto_trigger || 'cwd') + ' -> ' + esc(r.profile_slug) : 'no switch' },
    { name: 'Profile Filter', fired: r.tools_removed > 0, detail: r.tools_removed > 0 ? '-' + r.tools_removed + ' tools from ' + total : 'no tools stripped' },
    { name: 'Inject', fired: r.inject_fired, detail: r.inject_fired ? 'system_prefix.md (' + r.inject_chars + ' chars)' : 'not active' },
    { name: 'Adaptive', fired: r.adaptive_fired, detail: r.adaptive_fired ? 'adaptive_prefix.md (' + r.adaptive_chars + ' chars)' : 'not active' },
    { name: 'Coaching', fired: r.coaching_fired, detail: r.coach_kind || 'no signal' },
    { name: 'Budget Guard', fired: r.budget_blocked, detail: r.budget_blocked ? 'BLOCKED' : 'within budget' },
  ];
  if (r.fatigue_blocked) gates.push({ name: 'Fatigue', fired: true, detail: 'session would be blocked' });

  const gateHtml = gates.map(g => {
    const icon = g.fired ? '<span style="color:#22c55e">+</span>' : '<span style="color:var(--t4)">.</span>';
    return `<div style="display:flex;gap:8px;font-size:13px;margin-bottom:4px">${icon} <strong style="width:120px">${g.name}</strong> <span style="color:var(--t2)">${g.detail}</span></div>`;
  }).join('');

  const ctxPreview = r.additional_context
    ? `<div class="card" style="margin-top:14px"><div class="section-head">Injected context (${r.additional_context.length} chars)</div><pre style="white-space:pre-wrap;font-size:11px;max-height:200px;overflow:auto;color:var(--t2)">${esc(r.additional_context.slice(0, 2000))}</pre></div>`
    : '';

  return `<div class="card" style="margin-bottom:16px">
    <div class="section-head">Pipeline result</div>
    <div style="margin-bottom:12px;font-size:13px;color:var(--t2)">
      <strong>${esc(r.profile_slug)}</strong> (${esc(r.effective_profile)})${r.auto_selected ? ' — auto-selected' : ''}<br>
      ${r.tools_kept} tools kept, ${r.tools_removed} stripped (${pct}% cut), ~${fmtK(r.tokens_saved)} tokens saved
    </div>
    <div class="section-head" style="font-size:12px;margin-top:12px">Gates</div>
    ${gateHtml}
  </div>
  <div class="card" style="margin-bottom:16px">
    <div class="section-head">Cost estimate (per request)</div>
    <div style="display:flex;gap:40px;font-size:14px">
      <div><div style="font-size:11px;color:var(--t3)">Without ctx</div><div style="font-size:20px;font-weight:700">${fmtCost(r.estimated_cost_without_ctx)}</div></div>
      <div><div style="font-size:11px;color:var(--t3)">With ctx</div><div style="font-size:20px;font-weight:700;color:var(--green)">${fmtCost(r.estimated_cost_with_ctx)}</div></div>
      <div><div style="font-size:11px;color:var(--t3)">Savings</div><div style="font-size:20px;font-weight:700;color:var(--green)">${fmtCost(r.savings_usd)} (${r.savings_pct.toFixed(0)}%)</div></div>
    </div>
  </div>
  ${ctxPreview}`;
}

function renderProfileComparison(results) {
  const rows = results.map(r => {
    const total = r.tools_kept + r.tools_removed;
    return `<tr>
      <td><strong>${esc(r.profile_slug)}</strong></td>
      <td>${r.tools_kept}</td>
      <td>${r.tools_removed}</td>
      <td>${fmtK(r.tokens_saved)}</td>
      <td>${fmtCost(r.estimated_cost_with_ctx)}</td>
      <td>${r.savings_pct.toFixed(0)}%</td>
    </tr>`;
  }).join('');
  const bars = results.map(r => {
    const w = Math.max(2, r.savings_pct);
    return `<div style="display:flex;align-items:center;gap:8px;margin-bottom:4px">
      <span style="width:80px;font-size:12px;text-align:right">${esc(r.profile_slug)}</span>
      <div style="height:16px;width:${w}%;max-width:400px;background:linear-gradient(90deg,#22c55e,#86efac);border-radius:4px"></div>
      <span style="font-size:11px;color:var(--t3)">${r.savings_pct.toFixed(0)}%</span>
    </div>`;
  }).join('');
  const best = results[0];
  return `<div class="card" style="margin-bottom:16px">
    <div class="section-head">Profile comparison</div>
    <div class="section-sub">Each bar shows how many tokens this profile would strip from the same prompt. Taller bars save more.</div>
    ${bars}
  </div>
  <div class="card" style="margin-bottom:16px">
    <table style="width:100%;font-size:12px"><thead><tr><th>Profile</th><th>Tools</th><th>Stripped</th><th>Tokens Saved</th><th>Est. Cost</th><th>Savings</th></tr></thead><tbody>${rows}</tbody></table>
  </div>
  ${best && best.savings_pct > 0 ? `<div class="card"><div class="section-head">Best fit: ${esc(best.profile_slug)} (${best.savings_pct.toFixed(0)}% savings)</div></div>` : ''}`;
}

function renderReplayTable(items) {
  if (!items.length) return '<div class="empty">No traces to replay.</div>';
  const rows = items.map(({ trace: t, sim: s }) => {
    if (!s) return '';
    const match = t.tools_kept === s.tools_kept && t.tools_removed === s.tools_removed;
    const style = match ? '' : 'background:rgba(239,68,68,.06)';
    return `<tr style="${style}">
      <td>${esc(t.ts.slice(0, 19))}</td>
      <td>${esc(t.profile)}</td>
      <td>${t.tools_kept}</td><td>${s.tools_kept}</td>
      <td>${fmtK(t.tokens_saved)}</td><td>${fmtK(s.tokens_saved)}</td>
      <td>${match ? '<span style="color:#22c55e">match</span>' : '<span style="color:#ef4444">diff</span>'}</td>
    </tr>`;
  }).join('');
  return `<div class="card">
    <div class="section-head">Replay: actual vs simulated</div>
    <div class="section-sub">Red rows indicate a discrepancy between what happened and what the simulation predicts now.</div>
    <table style="width:100%;font-size:12px"><thead><tr><th>Timestamp</th><th>Profile</th><th>Kept(A)</th><th>Kept(S)</th><th>Saved(A)</th><th>Saved(S)</th><th>Match</th></tr></thead><tbody>${rows}</tbody></table>
  </div>`;
}
