// ─── Tab 5: Pipeline — Features at work ────────────────────

const PIPE_GATE_ORDER = [
  'filter', 'auto', 'inject', 'adaptive', 'coach', 'budget', 'behavior', 'compress',
];

const PIPE_GATE_META = {
  filter:   { label: 'Profile filter', ab: 'profile' },
  auto:     { label: 'Auto-profile', ab: null },
  inject:   { label: 'System prefix', ab: 'inject' },
  adaptive: { label: 'Adaptive prefix', ab: 'adaptive' },
  coach:    { label: 'Coaching', ab: 'coaching' },
  budget:   { label: 'Budget guard', ab: null },
  behavior: { label: 'Behavior guard', ab: null },
  compress: { label: 'Bash compress', ab: null },
};

const PIPE_VERDICT_LABELS = {
  keep: 'Worth keeping',
  early: 'Too early',
  off: 'Off',
  review: 'Review',
  unavailable: 'Unavailable',
};

function pipeGoTab(id) {
  for (const nav of document.querySelectorAll('.nav-item')) {
    const onclick = nav.getAttribute('onclick') || '';
    if (onclick.includes("showTab('" + id + "'")) {
      showTab(id, nav);
      return;
    }
  }
}

async function loadPipeline() {
  const [gatesData, readiness, stats, abReport] = await Promise.all([
    fetch(appendSince('/api/gates')).then(r => r.json()).catch(() => ({
      gates: [], activity: [], sessions_fallback_note: null, hook_only: false,
    })),
    fetch('/api/profiles/readiness').then(r => r.json()).catch(() => ({})),
    fetch(appendSince('/api/stats')).then(r => r.json()).catch(() => ({})),
    fetch(appendSince('/api/ab-report')).then(r => r.json()).catch(() => []),
  ]);

  const hookOnly = gatesData.hook_only || !!gatesData.sessions_fallback_note || !!stats.sessions_fallback;
  const activeProfile = gatesData.active_profile || stats.active_profile || pipeActiveProfile(gatesData.gates) || 'all';
  const abByFeature = {};
  for (const row of abReport || []) abByFeature[row.feature] = row;

  const noteEl = document.getElementById('gate-sessions-fallback-note');
  if (noteEl) {
    if (gatesData.sessions_fallback_note) {
      noteEl.style.display = 'block';
      noteEl.textContent = gatesData.sessions_fallback_note;
    } else {
      noteEl.style.display = 'none';
      noteEl.textContent = '';
    }
  }

  const hookCallout = document.getElementById('pipe-hook-callout');
  if (hookCallout) {
    if (hookOnly) {
      hookCallout.style.display = 'block';
      hookCallout.innerHTML =
        '<strong>Hook install.</strong> Counts come from hook traces and session ingest. ' +
        'Behavior guard and some budget signals only appear on the ctx proxy path.';
    } else {
      hookCallout.style.display = 'none';
      hookCallout.textContent = '';
    }
  }

  renderPipeHero(gatesData, readiness, activeProfile);
  renderPipeFeatureCards(gatesData.gates || [], readiness, stats, hookOnly, activeProfile, abByFeature);
  renderGateFeedSummary(gatesData);
  renderGateFeed(gatesData.activity || []);
}

function pipeActiveProfile(gates) {
  const filter = (gates || []).find(g => g.id === 'filter');
  if (!filter || !filter.detail) return null;
  const m = String(filter.detail).match(/^(\S+)\s+profile/i);
  return m ? m[1] : null;
}

function pipeFmtTok(n) {
  if (!n || n <= 0) return '0';
  return n >= 1000 ? (n / 1000).toFixed(1) + 'K' : String(n);
}

function pipeVerdictClass(kind) {
  if (kind === 'saving') return 'exp-verdict-saving';
  if (kind === 'costing') return 'exp-verdict-costing';
  if (kind === 'early') return 'exp-verdict-early';
  return 'exp-verdict-neutral';
}

function pipeVerdictKind(apiVerdict) {
  if (apiVerdict === 'keep') return 'saving';
  if (apiVerdict === 'early') return 'early';
  if (apiVerdict === 'off' || apiVerdict === 'unavailable' || apiVerdict === 'review') return 'neutral';
  return 'neutral';
}

function pipeActivityLine(g, activeProfile) {
  const count = g.today_count || 0;
  const id = g.id;
  if (id === 'filter') {
    return count > 0
      ? 'Stripped tools on ' + count + ' prompt' + (count === 1 ? '' : 's') + ' today'
      : 'No profile strips today · pinned `' + activeProfile + '`';
  }
  if (id === 'auto') {
    if (count > 0) return 'Auto matched ' + count + '× today';
    if (activeProfile && activeProfile !== 'all') {
      return 'No auto match today · pinned `' + activeProfile + '` still filters';
    }
    return 'No auto match today · watching cwd';
  }
  if (id === 'inject') return count > 0 ? 'Prefix applied on ' + count + ' prompt' + (count === 1 ? '' : 's') + ' today' : (g.enabled ? 'Ready · waiting for first prompt' : 'Prefix disabled or missing file');
  if (id === 'adaptive') return count > 0 ? 'Adaptive prefix on ' + count + ' prompt' + (count === 1 ? '' : 's') + ' today' : (g.enabled ? 'Enabled · no fires yet today' : 'Not enabled');
  if (id === 'coach') return count > 0 ? count + ' coaching signal' + (count === 1 ? '' : 's') + ' today' : 'Monitoring correction patterns';
  if (id === 'budget') return count > 0 ? 'Threshold crossed ' + count + '× today' : 'Watching session cost';
  if (id === 'behavior') return count > 0 ? 'Fired ' + count + '× today' : (g.enabled === false ? 'Proxy only' : 'Idle · monitoring');
  if (id === 'compress') return 'Not active';
  return count > 0 ? count + ' events today' : 'Idle today';
}

function pipeApplyAbVerdict(card, abRow) {
  if (!abRow) return card;
  const t = abRow.treatment || {};
  const c = abRow.control || {};
  const total = (t.count || 0) + (c.count || 0);
  if (total < 20) return card;
  const delta = abRow.cost_delta_pct;
  if (delta == null || Math.abs(delta) < 3) {
    card.verdict = { kind: 'neutral', label: 'A/B: same cost', detail: 'Experiment shows about the same cost with or without this feature.' };
    return card;
  }
  if (delta < 0) {
    card.verdict = { kind: 'saving', label: 'A/B: saving', detail: 'Experiment shows ~' + Math.abs(Math.round(delta)) + '% lower cost with this feature on.' };
    card.impactSecondary = (card.impactSecondary ? card.impactSecondary + ' · ' : '') + 'A/B treatment cheaper';
  } else {
    card.verdict = { kind: 'costing', label: 'A/B: review', detail: 'Experiment shows ~' + Math.round(delta) + '% higher cost — consider turning off.' };
    card.impactSecondary = (card.impactSecondary ? card.impactSecondary + ' · ' : '') + 'A/B treatment costs more';
  }
  return card;
}

function pipeGateCard(g, readiness, stats, hookOnly, activeProfile, abByFeature) {
  if (g.impact_primary) {
    let card = {
      impactKind: g.impact_kind || 'none',
      impactPrimary: g.impact_primary,
      impactSecondary: g.impact_secondary || '',
      activityLine: pipeActivityLine(g, activeProfile),
      verdict: {
        kind: pipeVerdictKind(g.verdict),
        label: PIPE_VERDICT_LABELS[g.verdict] || g.verdict || 'Idle',
        detail: g.verdict_detail || '',
      },
    };
    const abKey = g.ab_feature || (PIPE_GATE_META[g.id] && PIPE_GATE_META[g.id].ab);
    if (abKey && abByFeature[abKey]) card = pipeApplyAbVerdict(card, abByFeature[abKey]);
    return card;
  }

  const id = g.id;
  const count = g.today_count || 0;
  const tokens = g.today_tokens || 0;
  const enabled = g.enabled !== false;
  const ready = !!readiness.ready;
  const budgetThreshold = stats.session_budget_threshold_usd;

  let impactKind = 'none';
  let impactPrimary = '';
  let impactSecondary = '';
  let activityLine = pipeActivityLine(g, activeProfile);
  let verdict = { kind: 'neutral', label: 'Idle', detail: '' };

  if (id === 'filter') {
    impactKind = 'cost';
    if (tokens > 0) {
      impactPrimary = 'Saves ~' + pipeFmtTok(tokens) + ' tokens today';
      impactSecondary = count + ' prompt' + (count === 1 ? '' : 's') + ' with fewer tools';
      verdict = { kind: 'saving', label: 'Worth keeping', detail: 'Filtering is stripping unused MCP schemas.' };
    } else if (activeProfile === 'all' && !ready) {
      impactPrimary = 'Measuring your tool universe — no strip on `all` yet';
      impactSecondary = pipeReadinessLine(readiness);
      verdict = { kind: 'early', label: 'Too early', detail: 'Personal profile auto-activates when thresholds are met.' };
    } else {
      impactPrimary = count > 0 ? 'Filtering active — light strip today' : 'No tool strips recorded today';
      verdict = count > 0
        ? { kind: 'saving', label: 'Worth keeping', detail: 'Keep your active profile on.' }
        : { kind: 'early', label: 'Too early', detail: 'Send MCP prompts — activity shows after use.' };
    }
  } else if (id === 'compress') {
    impactKind = 'none';
    impactPrimary = 'Coming soon — not shipped yet';
    verdict = { kind: 'neutral', label: 'Unavailable', detail: 'Bash output compression is on the roadmap.' };
  } else if (id === 'behavior' && hookOnly) {
    impactKind = 'quality';
    impactPrimary = 'Not available on hook-only installs';
    verdict = { kind: 'neutral', label: 'Unavailable', detail: 'Runs on ctx proxy path, not filter.js hooks.' };
  } else if (id === 'budget') {
    impactKind = 'control';
    if (count > 0) {
      impactPrimary = count + ' budget hint' + (count === 1 ? '' : 's') + ' today';
      verdict = { kind: 'saving', label: 'Worth keeping', detail: 'Budget guard is pacing session spend.' };
    } else {
      const th = budgetThreshold > 0 ? '~$' + budgetThreshold.toFixed(0) : 'configured';
      impactPrimary = th + ' session threshold — no crossings today';
      verdict = { kind: 'neutral', label: 'Armed', detail: 'Guard stays on from monthly budget pacing.' };
    }
  } else if (count > 0) {
    impactPrimary = g.detail || (count + ' events today');
    verdict = { kind: 'saving', label: 'Active', detail: 'Feature fired today — see feed below.' };
  } else if (!enabled) {
    impactPrimary = g.detail || 'Disabled';
    verdict = { kind: 'neutral', label: 'Off', detail: 'Turn on in ~/.ctx/config.toml.' };
  } else {
    impactPrimary = g.detail || 'Idle today';
    verdict = { kind: 'early', label: 'Too early', detail: 'Use Claude Code with ctx active.' };
  }

  let card = { impactKind, impactPrimary, impactSecondary, activityLine, verdict };
  const abKey = PIPE_GATE_META[id] && PIPE_GATE_META[id].ab;
  if (abKey && abByFeature[abKey]) card = pipeApplyAbVerdict(card, abByFeature[abKey]);
  return card;
}

function pipeReadinessLine(readiness) {
  if (!readiness || readiness.ready) return '';
  const parts = [];
  if (readiness.min_tool_invocations) {
    parts.push((readiness.tool_invocations || 0) + '/' + readiness.min_tool_invocations + ' tool calls');
  }
  if (readiness.min_distinct_servers) {
    parts.push((readiness.distinct_servers || 0) + '/' + readiness.min_distinct_servers + ' servers');
  }
  if (readiness.min_sessions_with_mcp) {
    parts.push((readiness.sessions_with_mcp || 0) + '/' + readiness.min_sessions_with_mcp + ' MCP sessions');
  }
  return parts.length ? 'Progress: ' + parts.join(' · ') : '';
}

function renderPipeHero(gatesData, readiness, activeProfile) {
  const el = document.getElementById('pipe-hero');
  if (!el) return;

  const gates = gatesData.gates || [];
  const byId = {};
  for (const g of gates) byId[g.id] = g;

  const parts = [];
  const filter = byId.filter;
  const adaptive = byId.adaptive;
  const inject = byId.inject;
  const coach = byId.coach;

  if (filter && filter.impact_primary && filter.today_tokens > 0) {
    parts.push(filter.impact_primary.replace(/^~?/, '').replace(/ today$/, '') + ' today');
  } else if (activeProfile === 'all' && !readiness.ready) {
    parts.push('Profile filtering not active yet (on `all`) — ctx is measuring your MCP tools');
    const prog = pipeReadinessLine(readiness);
    if (prog) parts.push(prog.replace(/^Progress: /, ''));
  } else if (filter && filter.today_count > 0) {
    parts.push('Profile filter ran on ' + filter.today_count + ' prompt' + (filter.today_count === 1 ? '' : 's'));
  }

  if (adaptive && adaptive.today_count > 0) {
    parts.push('adaptive prefix on ' + adaptive.today_count + ' prompt' + (adaptive.today_count === 1 ? '' : 's'));
  } else if (inject && inject.today_count > 0) {
    parts.push('system prefix on ' + inject.today_count + ' prompt' + (inject.today_count === 1 ? '' : 's'));
  }

  if (coach && coach.today_count > 0) {
    parts.push('coaching on ' + coach.today_count + ' turn' + (coach.today_count === 1 ? '' : 's'));
  }

  if (gatesData.correction_rate_7d != null && coach && coach.today_count === 0) {
    parts.push(Math.round(gatesData.correction_rate_7d * 100) + '% correction rate over 7 days');
  }

  let headline = parts.length
    ? parts[0].charAt(0).toUpperCase() + parts[0].slice(1) + (parts.length > 1 ? '. ' + parts.slice(1).join('. ') + '.' : '.')
    : 'No feature activity yet today — use Claude Code with ctx active and check back after a few prompts.';

  let heroKind = 'neutral';
  if (filter && filter.today_tokens > 0) heroKind = 'saving';
  else if ((adaptive && adaptive.today_count > 0) || (inject && inject.today_count > 0)) heroKind = 'early';
  else if (!readiness.ready && activeProfile === 'all') heroKind = 'early';

  el.className = 'pipe-hero exp-verdict-callout ' + pipeVerdictClass(heroKind);
  el.textContent = headline;
}

function renderPipeFeatureCards(gates, readiness, stats, hookOnly, activeProfile, abByFeature) {
  const el = document.getElementById('pipe-feature-grid');
  if (!el) return;

  if (!gates.length) {
    el.innerHTML = '<div class="empty">Feature cards load after the first dashboard refresh. Reload if this stays empty.</div>';
    return;
  }

  const byId = {};
  for (const g of gates) byId[g.id] = g;

  const cards = PIPE_GATE_ORDER.map(id => {
    const g = byId[id];
    if (!g) return '';
    const meta = PIPE_GATE_META[id] || { label: g.name, ab: null };
    const card = pipeGateCard(g, readiness, stats, hookOnly, activeProfile, abByFeature);
    const abLink = (g.ab_feature || meta.ab)
      ? '<div class="pipe-card-link"><a href="#" onclick="pipeGoTab(\'experiment\');return false">Open Experiment</a></div>'
      : (id === 'filter'
        ? '<div class="pipe-card-link"><a href="#" onclick="pipeGoTab(\'trace\');return false">See in Trace</a></div>'
        : '');

    const secondary = card.impactSecondary
      ? '<div class="exp-card-meta">' + esc(card.impactSecondary) + '</div>'
      : '';

    const kindBadge = card.impactKind && card.impactKind !== 'none'
      ? '<span class="pipe-kind-badge">' + esc(card.impactKind) + '</span>'
      : '';

    return `<div class="exp-feature-card pipe-feature-card">
      <div class="exp-feature-card-top">
        <div class="exp-feature-name">${esc(meta.label)}${kindBadge}</div>
        <span class="exp-verdict-pill ${pipeVerdictClass(card.verdict.kind)}">${esc(card.verdict.label)}</span>
      </div>
      <div class="exp-card-headline">${esc(card.impactPrimary)}</div>
      ${secondary}
      <div class="pipe-activity-line">${esc(card.activityLine)}</div>
      <div class="exp-verdict-callout ${pipeVerdictClass(card.verdict.kind)}" style="margin-top:12px;margin-bottom:0">${esc(card.verdict.detail)}</div>
      ${abLink}
    </div>`;
  }).join('');

  el.innerHTML = cards;
}

function renderGateFeedSummary(data) {
  const el = document.getElementById('gate-feed-summary');
  if (!el) return;
  const act = data.activity || [];
  if (!act.length) {
    el.textContent = 'No pipeline activity yet. Start a Claude Code session — each prompt with ctx active appears here.';
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
  el.textContent = act.length + ' events — filter ' + filt + ', prefix ' + inj + ', adaptive ' + adp + ', coaching ' + coach + ', auto ' + auto + '. Click a row for details; identical stacks show a count badge.';
}

function renderGateFeed(activity) {
  const el = document.getElementById('gate-feed-wrap');
  if (!activity.length) {
    el.innerHTML = '<div class="empty">No gate events yet. Use Claude Code with ctx active (<code>filter.js</code> or the proxy). Events append as prompts flow.</div>';
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
      const cls = 'gf-chip gf-chip-' + x.id;
      return '<span class="' + cls + '">' + esc(x.label) + '</span>';
    }).join('');
    const badge = g.count > 1 ? '<span class="gf-count-badge">×' + g.count + '</span>' : '';
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
    return '<div class="gf-row" id="gf-row-' + gi + '" onclick="toggleGfRow(' + gi + ')">' +
      '<div class="gf-row-head">' +
        '<div class="gf-ts">' + fmtTs(a.ts) + '</div>' +
        badge +
        '<div class="gf-chips">' + chips + '</div>' +
      '</div>' +
      '<div class="gf-detail" id="gf-det-' + gi + '">' +
        meta + stackTimes +
      '</div>' +
    '</div>';
  }).join('');
}

function toggleGfRow(i) {
  const row = document.getElementById('gf-row-' + i);
  if (row) row.classList.toggle('expanded');
}
