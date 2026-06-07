// ─── Simulate tab (intuitive UX) ─────────────────────────

let _simLastTrace = null;

function simFmtK(n) {
  return n >= 1000 ? (n / 1000).toFixed(1) + 'K' : String(n);
}

function simFmtCents(n) {
  if (n == null || n <= 0) return '0¢';
  const cents = Math.round(n * 100);
  return cents + '¢';
}

function simFmtCentsSigned(n) {
  if (n == null || Math.abs(n) < 0.005) return '0¢';
  return simFmtCents(Math.abs(n));
}

function simFmtTimeShort(ts) {
  const d = new Date(ts);
  return d.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
}

function simRenderHeroCard(hero) {
  return `<div class="narrative-card sim-hero">
    <div class="narrative-eyebrow">Bottom line</div>
    <div class="narrative-body" style="margin-bottom:12px"><strong>${esc(hero.headline)}</strong></div>
    <div class="section-sub" style="margin-bottom:${hero.action ? '8px' : '0'};line-height:1.55">${esc(hero.body)}</div>
    ${hero.action ? `<div class="section-sub" style="margin-bottom:0;color:var(--t3)">${esc(hero.action)}</div>` : ''}
  </div>`;
}

function simBuildPreviewHero(r) {
  const saved = r.savings_usd || 0;
  let headline;
  if (saved >= 0.005) headline = 'Would save about ' + simFmtCents(saved) + ' on this prompt';
  else if (saved <= -0.005) headline = 'Would cost about ' + simFmtCentsSigned(saved) + ' more on this prompt';
  else headline = 'About the same cost with or without ctx on this prompt';

  const parts = [];
  if (r.auto_selected) parts.push(r.effective_profile + ' profile auto-selected');
  else parts.push('Using ' + r.effective_profile + ' profile');
  if (r.tools_removed > 0) parts.push('ctx would hide ' + r.tools_removed + ' tools');
  if (r.inject_fired) parts.push('add your system prefix');
  if (r.adaptive_fired) parts.push('add adaptive prefix');
  if (r.coaching_fired) parts.push('inject a coaching hint');
  if (r.budget_blocked) parts.push('budget guard would block this prompt');
  if (r.fatigue_blocked) parts.push('coaching fatigue would block this prompt');

  return {
    headline,
    body: parts.join('. ') + '.',
    action: 'Estimates use your current tool list and config — not an exact API bill.',
  };
}

function simBuildCompareHero(sorted) {
  const best = sorted[0];
  if (!best) {
    return { headline: 'No profiles to compare', body: 'Run ctx profile generate first.', action: '' };
  }
  const second = sorted[1];
  let headline = (best.effective_profile || best.profile_slug) + ' saves the most for this prompt';
  let body = 'Would cost ' + fmtCost(best.estimated_cost_with_ctx) + ' per request with ctx';
  if (best.savings_usd >= 0.005) {
    body += ', saving ' + simFmtCents(best.savings_usd) + ' vs all tools';
  }
  body += '.';
  if (second && second.savings_usd >= 0.005 && second.profile_slug !== best.profile_slug) {
    body += ' ' + (second.effective_profile || second.profile_slug) + ' is close (saves ' + simFmtCents(second.savings_usd) + ').';
  }
  return { headline, body, action: '' };
}

function simBuildReplayHero(items) {
  const valid = items.filter(x => x.sim);
  const matched = valid.filter(({ trace: t, sim: s }) =>
    t.tools_kept === s.tools_kept && t.tools_removed === s.tools_removed
  ).length;
  const total = valid.length;
  return {
    headline: matched + ' of ' + total + ' recent prompts match simulation',
    body: 'When rows do not match, your config or profiles likely changed since that prompt ran.',
    action: 'Use this to verify ctx behaves the same as when traces were recorded.',
  };
}

function simBuildGateSteps(r) {
  const total = r.tools_kept + r.tools_removed;
  const steps = [
    {
      label: 'Auto-profile',
      fired: r.auto_selected,
      summary: r.auto_selected
        ? 'Matched ' + (r.auto_trigger || 'folder') + ' → ' + r.effective_profile
        : 'Using ' + r.effective_profile + ' (no auto switch)',
    },
    {
      label: 'Tool filtering',
      fired: r.tools_removed > 0,
      summary: r.tools_removed > 0
        ? 'Would hide ' + r.tools_removed + ' of ' + total + ' tools (~' + simFmtK(r.tokens_saved) + ' tokens)'
        : 'No tools hidden for this profile',
    },
    {
      label: 'System prefix',
      fired: r.inject_fired,
      summary: r.inject_fired
        ? 'Would prepend system_prefix.md (' + r.inject_chars + ' chars)'
        : 'Not added — inject disabled or empty',
    },
    {
      label: 'Adaptive prefix',
      fired: r.adaptive_fired,
      summary: r.adaptive_fired
        ? 'Would prepend adaptive_prefix.md (' + r.adaptive_chars + ' chars)'
        : 'Not added — adaptive disabled or empty',
    },
    {
      label: 'Coaching',
      fired: r.coaching_fired,
      summary: r.coaching_fired
        ? 'Would inject coaching hint (' + (r.coach_kind || 'signal') + ')'
        : 'No correction pattern detected',
    },
    {
      label: 'Budget guard',
      fired: r.budget_blocked,
      summary: r.budget_blocked
        ? 'Would block: ' + (r.budget_reason || 'over budget pace')
        : 'Within monthly budget pace',
      danger: r.budget_blocked,
    },
  ];
  if (r.fatigue_blocked) {
    steps.push({
      label: 'Coaching fatigue',
      fired: true,
      summary: 'Session would be blocked due to repeated corrections',
      danger: true,
    });
  }
  return steps;
}

function renderSimGateFlow(steps) {
  return steps.map(step => {
    const pillCls = step.fired ? (step.danger ? 'sim-pill-danger' : 'sim-pill-run') : 'sim-pill-skip';
    const pillText = step.fired ? (step.danger ? 'Would block' : 'Would run') : 'Skipped';
    const dotCls = step.fired ? (step.danger ? 'sim-gate-dot danger' : 'sim-gate-dot fired') : 'sim-gate-dot';
    return `<div class="sim-gate-row">
      <div class="${dotCls}"></div>
      <div class="sim-gate-body">
        <div class="sim-gate-top">
          <span class="sim-gate-label">${esc(step.label)}</span>
          <span class="sim-on-pill ${pillCls}">${pillText}</span>
        </div>
        <div class="sim-gate-summary">${esc(step.summary)}</div>
      </div>
    </div>`;
  }).join('');
}

function renderSimCostRow(r) {
  const saved = r.savings_usd || 0;
  const saveCls = saved >= 0.005 ? 'sim-stat-save' : saved <= -0.005 ? 'sim-stat-cost' : '';
  return `<div class="sim-cost-row">
    <div class="sim-stat">
      <div class="sim-stat-label">Without ctx</div>
      <div class="sim-stat-val">${fmtCost(r.estimated_cost_without_ctx)}</div>
    </div>
    <div class="sim-stat">
      <div class="sim-stat-label">With ctx</div>
      <div class="sim-stat-val sim-stat-save">${fmtCost(r.estimated_cost_with_ctx)}</div>
    </div>
    <div class="sim-stat">
      <div class="sim-stat-label">You save</div>
      <div class="sim-stat-val ${saveCls}">${saved >= 0.005 ? simFmtCents(saved) : saved <= -0.005 ? simFmtCentsSigned(saved) + ' more' : '—'}</div>
    </div>
  </div>`;
}

function renderSimResult(r) {
  const hero = simBuildPreviewHero(r);
  const steps = simBuildGateSteps(r);
  const blockCallout = (r.budget_blocked || r.fatigue_blocked)
    ? `<div class="sim-verdict-callout sim-verdict-costing">${r.budget_blocked ? esc(r.budget_reason || 'Budget guard would block this prompt.') : 'Coaching fatigue would block this prompt.'}</div>`
    : '';

  const ctxPreview = r.additional_context
    ? `<details class="sim-details"><summary>Injected context preview (${r.additional_context.length} chars)</summary>
        <pre class="sim-context-pre">${esc(r.additional_context.slice(0, 2000))}</pre>
      </details>`
    : '';

  return simRenderHeroCard(hero)
    + renderSimCostRow(r)
    + blockCallout
    + `<div class="card" style="margin-top:16px">
        <div class="section-head">What would happen</div>
        <div class="sim-gate-flow">${renderSimGateFlow(steps)}</div>
      </div>`
    + ctxPreview;
}

function renderProfileComparison(results) {
  const sorted = [...results].sort((a, b) => (b.savings_usd || 0) - (a.savings_usd || 0));
  const hero = simBuildCompareHero(sorted);
  const bestSlug = sorted[0]?.profile_slug;
  const cwd = document.getElementById('sim-cwd')?.value || 'this folder';

  let html = simRenderHeroCard(hero);
  html += '<table class="sim-table"><thead><tr><th>Profile</th><th>Tools kept</th><th>Hidden</th><th>Est. cost</th><th>Saves</th></tr></thead><tbody>';
  sorted.forEach(r => {
    const rowCls = r.profile_slug === bestSlug && (r.savings_usd || 0) > 0 ? 'sim-row-best' : '';
    const saved = r.savings_usd || 0;
    html += `<tr class="${rowCls}">
      <td><strong>${esc(r.effective_profile || r.profile_slug)}</strong></td>
      <td>${r.tools_kept}</td>
      <td>${r.tools_removed}</td>
      <td>${fmtCost(r.estimated_cost_with_ctx)}</td>
      <td>${saved >= 0.005 ? simFmtCents(saved) : '—'}</td>
    </tr>`;
  });
  html += '</tbody></table>';

  if (bestSlug && (sorted[0].savings_usd || 0) >= 0.005) {
    html += `<div class="sim-verdict-callout sim-verdict-saving">Recommendation: use ${esc(sorted[0].effective_profile || bestSlug)} for prompts like this in ${esc(cwd)}</div>`;
  }
  return html;
}

function renderReplayTable(items) {
  if (!items.length) return '<div class="sim-idle-callout">No traces to replay.</div>';
  const hero = simBuildReplayHero(items);
  let html = simRenderHeroCard(hero);
  html += '<table class="sim-table"><thead><tr><th>When</th><th>Profile</th><th>Match?</th><th>Note</th></tr></thead><tbody>';
  items.forEach(({ trace: t, sim: s }) => {
    if (!s) return;
    const match = t.tools_kept === s.tools_kept && t.tools_removed === s.tools_removed;
    const rowCls = match ? 'sim-row-match' : 'sim-row-mismatch';
    html += `<tr class="${rowCls}">
      <td>${esc(simFmtTimeShort(t.ts))}</td>
      <td>${esc(t.profile || '—')}</td>
      <td><span class="sim-match-pill ${match ? 'yes' : 'no'}">${match ? 'Yes' : 'No'}</span></td>
      <td>${match ? 'Same tools kept' : 'Tools differ — config may have changed'}</td>
    </tr>`;
  });
  html += '</tbody></table>';
  return html;
}

function simSetBusy(busy) {
  ['sim-btn-preview', 'sim-btn-compare', 'sim-btn-replay'].forEach(id => {
    const el = document.getElementById(id);
    if (el) el.disabled = busy;
  });
}

function simShowResults(html) {
  const idle = document.getElementById('sim-idle');
  const results = document.getElementById('sim-results');
  const status = document.getElementById('sim-status');
  if (idle) idle.style.display = 'none';
  if (status) status.style.display = 'none';
  if (results) {
    results.style.display = 'block';
    results.innerHTML = html;
  }
}

function simShowError(msg) {
  const status = document.getElementById('sim-status');
  if (status) {
    status.style.display = 'block';
    status.textContent = msg;
  }
}

function simShowLoading(msg) {
  simShowResults('<div class="sim-loading">' + esc(msg) + '</div>');
}

async function loadSimulateTab() {
  const profileSel = document.getElementById('sim-profile');
  if (!profileSel) return;
  try {
    const profs = await fetch('/api/profiles').then(r => r.json());
    profileSel.innerHTML = '<option value="">(auto, let ctx pick)</option>' +
      (profs || []).map(p => `<option value="${esc(p.slug)}">${esc(p.display || p.slug)}</option>`).join('');
  } catch (_) {}
  const hookTraces = await fetch('/api/hook-traces?limit=1').then(r => r.json()).catch(() => []);
  if (hookTraces.length) {
    _simLastTrace = hookTraces[0];
    const cwdEl = document.getElementById('sim-cwd');
    const promptEl = document.getElementById('sim-prompt');
    if (cwdEl && !cwdEl.value && _simLastTrace.working_directory) {
      cwdEl.value = _simLastTrace.working_directory;
    }
    if (promptEl && !promptEl.value && _simLastTrace.human_text_prefix) {
      promptEl.value = _simLastTrace.human_text_prefix;
    }
  }
}

function simUseLastPrompt() {
  if (!_simLastTrace?.human_text_prefix) {
    alert('No recent prompt found. Send Claude Code traffic through ctx first.');
    return;
  }
  const el = document.getElementById('sim-prompt');
  if (el) el.value = _simLastTrace.human_text_prefix;
}

function simUseLastFolder() {
  if (!_simLastTrace?.working_directory) {
    alert('No recent working directory found. Send Claude Code traffic through ctx first.');
    return;
  }
  const el = document.getElementById('sim-cwd');
  if (el) el.value = _simLastTrace.working_directory;
}

async function runSimulate() {
  const body = {
    prompt: document.getElementById('sim-prompt').value,
    cwd: document.getElementById('sim-cwd').value || '.',
    profile: document.getElementById('sim-profile').value || null,
  };
  simSetBusy(true);
  simShowLoading('Running preview…');
  try {
    const resp = await fetch('/api/simulate', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }).then(r => r.json());
    if (resp.result) simShowResults(renderSimResult(resp.result));
    else simShowError('No result returned from simulation.');
  } catch (e) {
    simShowError('Simulation failed: ' + e);
  } finally {
    simSetBusy(false);
  }
}

async function runSimulateAllProfiles() {
  const body = {
    prompt: document.getElementById('sim-prompt').value,
    cwd: document.getElementById('sim-cwd').value || '.',
    all_profiles: true,
  };
  simSetBusy(true);
  simShowLoading('Comparing profiles…');
  try {
    const resp = await fetch('/api/simulate', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }).then(r => r.json());
    if (resp.all_profiles && resp.all_profiles.length) {
      simShowResults(renderProfileComparison(resp.all_profiles));
    } else {
      simShowResults('<div class="sim-idle-callout">No profiles found. Run <code>ctx profile generate</code> first.</div>');
    }
  } catch (e) {
    simShowError('Comparison failed: ' + e);
  } finally {
    simSetBusy(false);
  }
}

async function runSimulateReplay() {
  simSetBusy(true);
  simShowLoading('Checking last 10 prompts…');
  try {
    const traces = await fetch('/api/hook-traces?limit=10').then(r => r.json());
    if (!traces.length) {
      simShowResults('<div class="sim-idle-callout">No hook traces found yet. Send a prompt through Claude Code with ctx hooks enabled, then try again.</div>');
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
    simShowResults(renderReplayTable(results));
  } catch (e) {
    simShowError('Replay check failed: ' + e);
  } finally {
    simSetBusy(false);
  }
}









