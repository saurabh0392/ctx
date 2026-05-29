// ─── Tab 1: Savings ───────────────────────────────────────

const ALLOWANCE_LABELS = {
  five_hour: 'Session (5hr)',
  seven_day: 'Weekly (7 day)',
};

function fmtDurationSecs(secs) {
  if (secs == null || secs <= 0) return 'resets soon';
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h >= 48) return `resets in ${Math.round(h / 24)}d`;
  if (h > 0) return `resets in ${h}h ${m}m`;
  return `resets in ${m}m`;
}

function savingsGoTab(id) {
  const nav = document.querySelector('.nav-item[onclick*="showTab(\'' + id + '\'"]')
    || document.querySelector('.nav-item[onclick*="' + id + '"]');
  if (nav && typeof showTab === 'function') showTab(id, nav);
}

function detectStoryPhase(ctx) {
  const { stats, allowance, burn } = ctx;
  if (!stats || stats.session_count === 0) return 'empty';
  if (allowance && allowance.configured && !allowance.stale) {
    if (burn && burn.metrics_ready) return 'established';
    return 'runway';
  }
  if (stats.sessions_fallback) return 'hookOnly';
  return 'bootstrapping';
}

function formatToolMixSummary(tools) {
  if (!tools || !tools.length) return '';
  const counts = {};
  tools.forEach(t => {
    const tail = t.includes('__') ? t.split('__').pop().replace(/_/g, ' ') : t;
    const key = tail || t;
    counts[key] = (counts[key] || 0) + 1;
  });
  return Object.entries(counts)
    .map(([name, n]) => (n > 1 ? `${name} ×${n}` : name))
    .join(', ');
}

function buildSavingsStory(ctx) {
  const {
    stats, allowance, burn, settings, sessions, spendSessions, timeline,
    monthLabel, days, spendSum, toolMix, accessFriction,
  } = ctx;

  const phase = detectStoryPhase(ctx);
  const persona = computePersona(spendSessions);
  const { lead, strengths, action } = buildInsights(stats, sessions, spendSessions);
  const profileLabel = esc(PROFILE_LABELS[stats.active_profile] || stats.active_profile || 'all');
  const observedSpend = spendSum > 0 ? spendSum : (stats.current_month_session_spend_usd || 0);
  const sessionN = stats.session_count || 0;
  const avgDurMins = sessions.length
    ? Math.round(sessions.reduce((s, x) => s + x.duration_mins, 0) / sessions.length)
    : 0;
  const removedPerReq = stats.request_count > 0 ? Math.round(stats.total_tools_removed / stats.request_count) : 0;
  const keptPerReq = stats.request_count > 0 ? Math.round((stats.total_tools_kept || 0) / stats.request_count) : 0;
  const totalPerReq = removedPerReq + keptPerReq;

  const confidence = [];
  if (observedSpend > 0 || sessionN > 0) confidence.push('Observed spend');
  if (allowance && allowance.configured && !allowance.stale) confidence.push('Plan meter');
  else if (phase === 'bootstrapping' && allowance && allowance.statusline_wired) confidence.push('Plan meter pending');
  if (toolMix && toolMix.source === 'semantic') confidence.push('Similar sessions');
  if (stats.cost_saved > 0 && !stats.sessions_fallback) confidence.push('Estimated');
  if (phase === 'empty') confidence.push('Collecting');
  if (!confidence.length) confidence.push('Collecting');

  let kicker = monthLabel;
  if (days > 0) kicker += `, ${days} day${days === 1 ? '' : 's'} with ctx`;
  else if (phase === 'empty') kicker += ', ctx installed';

  const meters = [];
  if (allowance && allowance.configured && !allowance.stale) {
    for (const key of ['five_hour', 'seven_day']) {
      const w = allowance.windows && allowance.windows[key];
      if (!w) continue;
      meters.push({
        label: ALLOWANCE_LABELS[key] || key,
        pctUsed: w.used_pct,
        pctLeft: w.remaining_pct,
        meta: `${Math.round(w.remaining_pct)}% left, ${fmtDurationSecs(w.resets_in_secs)}`,
      });
    }
  }

  let act1Html = '';
  let setupHint = null;

  const heroSpend = observedSpend > 0
    ? fmtCost(observedSpend)
    : ((stats.current_month_session_spend_usd || 0) > 0 ? fmtCost(stats.current_month_session_spend_usd) : UI_EMPTY);
  const heroSpendSub = phase === 'hookOnly' ? 'This month, hook traces + JSONL' : 'This month';
  const heroSessionSub = avgDurMins > 0 && sessionN <= 3
    ? `Avg ${avgDurMins} min each`
    : (days > 0 ? `${days} day${days === 1 ? '' : 's'} with ctx` : 'Tracked by ctx');

  if (phase === 'empty') {
    act1Html = 'Nothing since ctx was activated yet. Your story appears after the first Claude Code session is ingested.';
    if (stats.dashboard_watermark_filtering) {
      act1Html += ' Switch the <strong>Data</strong> bar to <strong>All time</strong> for pre-install history.';
    }
  } else if (phase === 'bootstrapping') {
    act1Html = 'Allowance meters appear after Claude Code sends statusLine data. Reload VS Code and send one prompt (Pro/Max).';
    if (allowance && allowance.setup_hint) setupHint = allowance.setup_hint;
  } else if (phase === 'runway' || phase === 'established') {
    if (phase === 'established' && burn && burn.message) {
      act1Html = burn.message.replace(/\.$/, '') + '.';
    } else {
      act1Html = 'Plan allowance meters are live below. Spend ($) and allowance (%) measure different things.';
    }
  } else if (phase === 'hookOnly') {
    act1Html = 'Plan allowance meters are not shown until statusLine data arrives from Claude Code.';
  }

  if (allowance && allowance.stale && allowance.configured) {
    setupHint = 'Allowance data is stale. Open Claude Code and send a prompt so statusLine refreshes.';
  }

  let act2Html = '';
  if (toolMix && toolMix.source === 'semantic' && toolMix.neighbor_count >= 2 && toolMix.tools && toolMix.tools.length) {
    const toolSummary = formatToolMixSummary(toolMix.tools);
    act2Html = `On <span class="story-em">${profileLabel}</span>, ctx matched <span class="story-em">${toolMix.neighbor_count} similar sessions</span> and kept <span class="story-em">${toolMix.tools.length} tools</span> for recent work (${esc(toolSummary)}). `;
    act2Html += 'Estimated schema impact is not invoice-verified on hook installs.';
  } else if (phase === 'hookOnly' || stats.sessions_fallback) {
    act2Html = `On <span class="story-em">${profileLabel}</span>, ctx is filtering tools via permissions.deny. Per-request byte savings are not measured on hook installs.`;
  } else if (totalPerReq > 0 && removedPerReq > 0) {
    const pct = Math.round(removedPerReq / totalPerReq * 100);
    act2Html = `On <span class="story-em">${profileLabel}</span>, ctx kept ${keptPerReq} of ${totalPerReq} tool slots per request (${pct}% stripped). `;
    if (stats.cost_saved > 0) {
      act2Html += `Estimated filter impact <span class="story-em">${fmtCost(stats.cost_saved)}</span> in context tokens (not invoice-verified).`;
    }
  } else {
    act2Html = `Active profile: <span class="story-em">${profileLabel}</span>. Ctx hooks and ingest are tracking your sessions.`;
  }


  const mechanismStats = {
    requestCount: stats.request_count || 0,
    toolsRemovedPerReq: removedPerReq,
    toolsKeptPerReq: keptPerReq,
    tokensSaved: stats.total_tokens_saved || 0,
    costSaved: stats.cost_saved || 0,
    hookOnly: phase === 'hookOnly' || stats.sessions_fallback,
    toolMix,
  };

  let act3Html = action || lead;
  if (accessFriction && accessFriction.length && !action) {
    const first = accessFriction[0];
    act3Html = `ctx hid <span class="story-em">${esc(first.tool_display || first.tool)}</span>, needed ${first.count}× this week. Expand it in the appendix below.`;
  }

  let dataNote = 'Since ctx tracked hides pre-install sessions unless you pick All time in the Data bar.';
  if (phase === 'hookOnly') dataNote = 'Spend ($) and allowance (%) are different axes. The chart shows spend over time, not verified filter savings.';
  else if (phase === 'runway' || phase === 'established') dataNote = 'Observed spend ($) and plan allowance (%) measure different things. Do not reconcile arithmetically.';

  const chapterSubs = {
    ch1: phase === 'empty'
      ? 'Ctx is installed. Your baseline appears after the first session.'
      : 'Your baseline today. The starting point ctx measures improvement from.',
    ch2: phase === 'hookOnly' || stats.sessions_fallback
      ? 'Ctx filters tools on each session. Per-request savings are not measured on hook installs yet.'
      : 'Ctx shapes each request before Claude sees it. Profiles, filters, and tool recovery.',
    ch3: accessFriction && accessFriction.length
      ? 'Fix recurring friction first. Small profile tweaks compound fastest.'
      : 'One habit change this week beats tuning ten settings.',
    ch4: phase === 'established'
      ? 'Burn is slowing vs your first week. This is the arc ctx is building toward.'
      : 'As sessions accumulate, profiles and auto-switches tell the improvement story.',
  };

  const budgetNote = (() => {
    const cap = settings && settings.monthly_budget_usd;
    if (cap != null && cap > 0) {
      const used = settings.monthly_actual_spend_usd != null
        ? settings.monthly_actual_spend_usd
        : observedSpend;
      return `Manual budget: ${fmtCost(Math.max(0, cap - used))} left of ${fmtCost(cap)} cap (Settings).`;
    }
    return null;
  })();

  return {
    phase,
    kicker,
    confidence,
    hero: {
      visible: phase !== 'empty',
      spend: heroSpend,
      spendSub: heroSpendSub,
      sessions: sessionN,
      sessionsSub: heroSessionSub,
      profile: PROFILE_LABELS[stats.active_profile] || stats.active_profile || 'All tools',
    },
    act1: { html: act1Html, meters, setupHint, budgetNote },
    act2: {
      html: act2Html,
      persona,
      mechanismStats,
      links: [
        { tab: 'pipeline', label: 'Pipeline' },
        { tab: 'trace', label: 'Trace' },
        { tab: 'profiles', label: 'Profiles' },
      ],
    },
    act3: { html: act3Html },
    dataNote,
    chapterSubs,
    accessFriction: accessFriction || [],
  };
}

function renderStoryMeters(meters) {
  const el = document.getElementById('story-meters');
  if (!el) return;
  if (!meters || !meters.length) {
    el.innerHTML = '';
    return;
  }
  el.innerHTML = meters.map(m => {
    const used = Math.min(100, Math.max(0, m.pctUsed || 0));
    const warn = used >= 80 ? ' warn' : '';
    return `<div class="story-meter">
      <div class="story-meter-head">
        <span class="story-meter-label">${esc(m.label)}</span>
        <span class="story-meter-meta">${esc(m.meta)}</span>
      </div>
      <div class="story-meter-bar-track">
        <div class="story-meter-bar-fill${warn}" style="width:${used}%"></div>
      </div>
    </div>`;
  }).join('');
}

function renderStoryHero(hero) {
  const wrap = document.getElementById('story-hero');
  if (!wrap) return;
  if (!hero || !hero.visible) {
    wrap.hidden = true;
    return;
  }
  wrap.hidden = false;
  const spendEl = document.getElementById('story-hero-spend');
  const spendSubEl = document.getElementById('story-hero-spend-sub');
  const sessEl = document.getElementById('story-hero-sessions');
  const sessSubEl = document.getElementById('story-hero-sessions-sub');
  const profEl = document.getElementById('story-hero-profile');
  const profSubEl = document.getElementById('story-hero-profile-sub');
  if (spendEl) spendEl.textContent = hero.spend || UI_EMPTY;
  if (spendSubEl) spendSubEl.textContent = hero.spendSub || 'This month';
  if (sessEl) sessEl.textContent = String(hero.sessions ?? UI_EMPTY);
  if (sessSubEl) sessSubEl.textContent = hero.sessionsSub || 'Tracked by ctx';
  if (profEl) profEl.textContent = hero.profile || UI_EMPTY;
  if (profSubEl) profSubEl.textContent = 'Tool filter profile';
}

function renderPersonaCard(persona) {
  const el = document.getElementById('story-act-2-persona');
  if (!el) return;
  if (!persona) {
    el.hidden = true;
    el.innerHTML = '';
    return;
  }
  el.hidden = false;
  const traits = (persona.traits || []).map(t =>
    `<div class="persona-trait"><span class="persona-trait-label">${esc(t.label)}</span><span class="persona-trait-val">${esc(t.val)}</span></div>`
  ).join('');
  el.innerHTML = `
    <div class="persona-icon">${persona.icon}</div>
    <div class="persona-eyebrow">Your work style</div>
    <div class="persona-name">${esc(persona.name)}</div>
    <div class="persona-desc">${esc(persona.desc)}</div>
    ${traits ? `<div class="persona-traits">${traits}</div>` : ''}`;
}

function renderMechanismStats(ms) {
  const el = document.getElementById('story-act-2-stats');
  if (!el || !ms) return;
  const tiles = [
    {
      label: 'Requests filtered',
      val: ms.requestCount > 0 ? ms.requestCount.toLocaleString() : UI_EMPTY,
      sub: ms.hookOnly ? 'Hook traces' : 'This period',
    },
    {
      label: 'Tools stripped / req',
      val: ms.toolsRemovedPerReq > 0 ? `~${ms.toolsRemovedPerReq}` : UI_EMPTY,
      sub: 'Before the API call',
    },
    {
      label: 'Tools kept / req',
      val: ms.toolsKeptPerReq > 0 ? `~${ms.toolsKeptPerReq}` : UI_EMPTY,
      sub: 'Sent to Claude',
    },
    {
      label: 'Est. tokens saved',
      val: ms.tokensSaved > 0 ? fmtK(ms.tokensSaved) : UI_EMPTY,
      sub: ms.costSaved > 0 ? fmtCost(ms.costSaved) + ' est.' : (ms.hookOnly ? 'Schema estimate' : 'Filter impact'),
    },
  ];
  if (ms.toolMix && ms.toolMix.tools && ms.toolMix.tools.length) {
    tiles.push({
      label: 'Semantic recovery',
      val: String(ms.toolMix.tools.length),
      sub: `${ms.toolMix.neighbor_count} similar sessions`,
    });
  }
  el.innerHTML = `<div class="story-mechanism-grid">${tiles.map(t =>
    `<div class="story-mechanism-stat">
      <div class="story-mechanism-label">${esc(t.label)}</div>
      <div class="story-mechanism-val">${t.val}</div>
      <div class="story-mechanism-sub">${esc(t.sub)}</div>
    </div>`
  ).join('')}</div>`;
}

function renderSavingsStory(story) {
  const kickerEl = document.getElementById('story-kicker');
  const confEl = document.getElementById('story-confidence');
  if (kickerEl) kickerEl.textContent = story.kicker || '';
  if (confEl) {
    confEl.innerHTML = (story.confidence || [])
      .map(t => `<span class="story-confidence-tag">${esc(t)}</span>`)
      .join('');
  }
  renderStoryHero(story.hero);

  const act1Body = document.getElementById('story-act-1-body');
  if (act1Body) {
    let html = story.act1.html || '';
    if (story.act1.budgetNote) {
      html += `<p class="story-data-note" style="margin-top:12px;margin-bottom:0">${esc(story.act1.budgetNote)}</p>`;
    }
    if (story.act1.setupHint) {
      html += `<div class="story-setup-hint">${esc(story.act1.setupHint)}</div>`;
    }
    act1Body.innerHTML = html;
  }
  renderStoryMeters(story.act1.meters);

  const act2Body = document.getElementById('story-act-2-body');
  if (act2Body) act2Body.innerHTML = story.act2.html || '';
  renderMechanismStats(story.act2.mechanismStats);
  renderPersonaCard(story.act2.persona);

  const linksEl = document.getElementById('story-act-2-links');
  if (linksEl) {
    linksEl.innerHTML = (story.act2.links || [])
      .map(l => `<a href="#" onclick="savingsGoTab('${esc(l.tab)}');return false">${esc(l.label)}</a>`)
      .join('');
  }

  const act3Body = document.getElementById('story-act-3-body');
  if (act3Body) act3Body.innerHTML = story.act3.html || '';

  const subs = story.chapterSubs || {};
  const ch1 = document.getElementById('story-ch1-sub');
  const ch2 = document.getElementById('story-ch2-sub');
  const ch3 = document.getElementById('story-ch3-sub');
  const ch4 = document.getElementById('story-ch4-sub');
  if (ch1 && subs.ch1) ch1.textContent = subs.ch1;
  if (ch2 && subs.ch2) ch2.textContent = subs.ch2;
  if (ch3 && subs.ch3) ch3.textContent = subs.ch3;
  if (ch4 && subs.ch4) ch4.textContent = subs.ch4;

  const noteEl = document.getElementById('story-data-note');
  if (noteEl) noteEl.textContent = story.dataNote || '';
}

async function keepFrictionTool(tool) {
  try {
    await fetch('/api/savings/keep-tool', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ tool }),
    });
    loadSavings();
  } catch (_) {}
}

function renderAccessFrictionPanel(items) {
  const panel = document.getElementById('access-friction-panel');
  const body = document.getElementById('access-friction-body');
  if (!panel || !body) return;
  if (!items || !items.length) {
    panel.style.display = 'none';
    return;
  }
  panel.style.display = 'block';
  body.innerHTML = items.map(row => {
    const label = esc(row.tool_display || row.tool);
    return `<div class="access-friction-row">
      <span>ctx hid <strong>${label}</strong>, needed ${row.count}× this week</span>
      <button type="button" class="access-friction-keep" onclick="keepFrictionTool('${esc(row.tool)}')">Keep tool</button>
    </div>`;
  }).join('');
}

async function loadSavings() {
  const now = new Date();
  const monthStr = now.toISOString().slice(0, 7);
  const monthLabel = now.toLocaleDateString('en-US', { month: 'long', year: 'numeric' });

  const [stats, timeline, sessions, spendRaw, projectsRows, toolHeat, allowanceCurrent, allowanceBurn, settings, toolMix, accessFriction] = await Promise.all([
    fetch(appendSince('/api/stats')).then(r => r.json()),
    fetch(appendSince('/api/timeline')).then(r => r.json()),
    fetch(appendSince('/api/sessions')).then(r => r.json()),
    fetch(appendSince(`/api/spend/sessions?month=${monthStr}`)).then(r => r.json()).catch(() => []),
    fetch(appendSince('/api/projects')).then(r => r.json()).catch(() => []),
    fetch(appendSince('/api/tool-usage')).then(r => r.json()).catch(() => []),
    fetch('/api/allowance/current').then(r => r.json()).catch(() => null),
    fetch('/api/allowance/burn-rate').then(r => r.json()).catch(() => null),
    fetch('/api/settings').then(r => r.json()).catch(() => ({})),
    fetch('/api/savings/tool-mix').then(r => r.json()).catch(() => null),
    fetch('/api/savings/access-friction').then(r => r.json()).catch(() => []),
  ]);

  const spendSessions = Array.isArray(spendRaw) ? spendRaw : [];
  const spendSum = spendSessions.reduce((s, x) => s + (x.total_usd || 0), 0);
  const sessionRows = Array.isArray(sessions) ? sessions : [];
  const timelineRows = Array.isArray(timeline) ? timeline : [];
  updateCtxRangeMetaFromStats(stats || {});

  const oldest = sessionRows.length ? sessionRows[sessionRows.length - 1] : null;
  const days = oldest ? Math.max(1, Math.ceil((Date.now() - new Date(oldest.started_at)) / 86400000)) : 0;

  const story = buildSavingsStory({
    stats,
    allowance: allowanceCurrent,
    burn: allowanceBurn,
    settings,
    sessions: sessionRows,
    spendSessions,
    timeline: timelineRows,
    monthLabel,
    days,
    spendSum,
    toolMix,
    accessFriction: Array.isArray(accessFriction) ? accessFriction : [],
  });
  renderSavingsStory(story);
  renderAccessFrictionPanel(story.accessFriction);

  // Proxy status
  const dot = document.getElementById('status-dot');
  const txt = document.getElementById('status-text');
  if (dot && txt) {
    if (stats.sessions_fallback) {
      dot.className = 'status-dot on';
      txt.textContent = 'Session index (post-ctx sessions only)';
    } else {
      dot.className = 'status-dot ' + (stats.proxy_listening ? 'on' : 'off');
      txt.textContent = stats.proxy_listening ? 'Proxy active' : 'Proxy offline';
    }
  }
  const sockEl = document.getElementById('socket-status');
  if (sockEl && !window._ctxEventSource) {
    sockEl.textContent = 'Event stream: none yet';
  }

  const onboardEl = document.getElementById('onboarding-wrap');
  const wizDone = localStorage.getItem('ctx-onboarding-done');
  if (onboardEl) {
    const showWiz = !wizDone && stats.request_count === 0 && spendSum < 0.01;
    onboardEl.style.display = showWiz ? 'block' : 'none';
    if (showWiz) { wizPrepareFromServer(); wizShowStep(1); }
  }

  renderProjectsPanel(Array.isArray(projectsRows) ? projectsRows : [], !!stats.sessions_fallback);
  renderToolUsagePanel(Array.isArray(toolHeat) ? toolHeat : [], { invocationsOnly: !!stats.sessions_fallback });

  const hookOnly = !!stats.sessions_fallback;
  const chartTitle = document.getElementById('timeline-chart-title');
  const chartSub = document.getElementById('timeline-chart-sub');
  const chartLegend = document.getElementById('timeline-chart-legend');
  if (chartTitle) chartTitle.textContent = hookOnly ? 'Spend over time' : 'Savings over time';
  if (chartSub) chartSub.textContent = hookOnly
    ? 'Your trajectory since ctx started tracking. The baseline to improve from.'
    : 'Running total since ctx was installed. Savings accumulating over time.';
  if (chartLegend) chartLegend.textContent = hookOnly ? 'Cumulative spend' : 'Cumulative saved';

  // ── Cumulative chart ──────────────────────────────────────
  let running = 0;
  const cumul = timelineRows.map(p => { running += p.cost; return { date: p.date, total: +running.toFixed(4) }; });
  if (cumul.length) {
    const firstDate = new Date(cumul[0].date);
    firstDate.setDate(firstDate.getDate() - 1);
    cumul.unshift({ date: firstDate.toISOString().slice(0, 10), total: 0 });
  }
  const singlePoint = cumul.length === 2;
  if (cumul.length && typeof Chart !== 'undefined') {
    const chartEl = document.getElementById('timeline-chart').getContext('2d');
    if (window._timelineChart) { window._timelineChart.destroy(); window._timelineChart = null; }
    const grad = chartEl.createLinearGradient(0, 0, 0, 220);
    grad.addColorStop(0, 'rgba(147,192,67,.22)');
    grad.addColorStop(1, 'rgba(147,192,67,.0)');
    window._timelineChart = new Chart(chartEl, {
      type: 'line',
      data: {
        labels: cumul.map(p => { const d = new Date(p.date); return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' }); }),
        datasets: [{ label: hookOnly ? 'Cumulative spend' : 'Total saved', data: cumul.map(p => p.total),
          borderColor: '#93c043', backgroundColor: grad,
          borderWidth: 2.5, pointRadius: singlePoint ? [0, 5] : 0, pointHoverRadius: 5,
          pointBackgroundColor: '#b0d46a', fill: true, tension: 0.45 }]
      },
      options: {
        responsive: true, maintainAspectRatio: false, animation: { duration: 1000 },
        plugins: { legend: { display: false },
          tooltip: { backgroundColor: '#1f2e33', borderColor: '#2e4048', borderWidth: 1, padding: 12,
            callbacks: { label: c => '  ' + (hookOnly ? 'Spend: ' : 'Total saved: ') + fmtCost(c.parsed.y) } } },
        scales: {
          x: { grid: { color: '#2e4048', lineWidth: .5 }, ticks: { color: '#4d7060', font: { size: 11 } } },
          y: { grid: { color: '#1f2a30', lineWidth: .5 }, ticks: { color: '#4d7060', font: { size: 11 }, callback: v => '$' + v.toFixed(2) }, beginAtZero: true }
        }
      }
    });
  }

  const sessEl = document.getElementById('sessions-compact');
  if (sessEl) {
    if (!sessionRows.length) {
      sessEl.innerHTML = '<div class="empty">No sessions yet.</div>';
    } else {
      sessEl.innerHTML = sessionRows.slice(0, 8).map(s => {
        const dur = s.duration_mins < 60 ? `${s.duration_mins}m` : `${Math.floor(s.duration_mins / 60)}h ${s.duration_mins % 60}m`;
        return `<div class="sv-session">
      <div>
        <div class="sv-session-date">${fmtDate(s.started_at)}</div>
        <div class="sv-session-meta">${dur}, ${s.requests} requests</div>
      </div>
      <div class="sv-session-cost">${fmtCost(s.cost)}</div>
    </div>`;
      }).join('');
    }
  }

  loadProfileAnalytics().catch(() => {});
}

function computePersona(sp) {
  if (!sp || !sp.length) return null;
  const totalTurns = sp.reduce((s, x) => s + (x.turn_count || 0), 0);
  if (totalTurns === 0) return null;
  const avgTurns = totalTurns / sp.length;
  const corrRate = sp.reduce((s, x) => s + (x.correction_turns || 0), 0) / totalTurns;
  const compactN = sp.filter(s => s.hit_compact).length;
  const compactRate = compactN / sp.length;

  if (compactRate > 0.35) return {
    icon: '🤿', name: 'The Deep Diver',
    desc: 'You work through large, complex problems end-to-end. Sessions regularly push the limits of working memory.',
    traits: [{ label: 'Avg session length', val: `${Math.round(avgTurns)} turns` }, { label: 'Context resets', val: compactN }, { label: 'Approach', val: 'Thorough' }]
  };
  if (avgTurns > 28 && corrRate < 0.12) return {
    icon: '🏗', name: 'The Architect',
    desc: 'Long, focused sessions with clear direction. You think through complex problems systematically before asking.',
    traits: [{ label: 'Avg session length', val: `${Math.round(avgTurns)} turns` }, { label: 'Correction rate', val: `${Math.round(corrRate * 100)}%` }, { label: 'Approach', val: 'Systematic' }]
  };
  if (corrRate > 0.22) return {
    icon: '🔄', name: 'The Refiner',
    desc: 'You iterate toward precision. High correction rate signals you know exactly what you want and push until you get it.',
    traits: [{ label: 'Correction rate', val: `${Math.round(corrRate * 100)}%` }, { label: 'Avg session length', val: `${Math.round(avgTurns)} turns` }, { label: 'Approach', val: 'Iterative' }]
  };
  if (avgTurns < 12 && corrRate < 0.1) return {
    icon: '⚡', name: 'The Sprinter',
    desc: 'Short, decisive sessions with clear intent. You get in, get the answer, get out.',
    traits: [{ label: 'Avg session length', val: `${Math.round(avgTurns)} turns` }, { label: 'Correction rate', val: `${Math.round(corrRate * 100)}%` }, { label: 'Approach', val: 'Decisive' }]
  };
  return {
    icon: '🔨', name: 'The Builder',
    desc: 'Steady, productive sessions. A balanced mix of exploration and execution.',
    traits: [{ label: 'Avg session length', val: `${Math.round(avgTurns)} turns` }, { label: 'Correction rate', val: `${Math.round(corrRate * 100)}%` }, { label: 'Approach', val: 'Balanced' }]
  };
}

function buildInsights(stats, sessions, sp) {
  const avgDur = sessions.length
    ? Math.round(sessions.reduce((s, x) => s + x.duration_mins, 0) / sessions.length)
    : 0;
  const toolsPerReq = stats.request_count > 0
    ? Math.round(stats.total_tools_removed / stats.request_count)
    : 0;

  let compactN = 0, corrRate = 0, avgTurns = 0, cacheHitRate = 0;
  if (sp && sp.length) {
    compactN = sp.filter(s => s.hit_compact).length;
    const totalTurns = sp.reduce((s, x) => s + (x.turn_count || 0), 0);
    const totalCorr = sp.reduce((s, x) => s + (x.correction_turns || 0), 0);
    corrRate = totalTurns > 0 ? totalCorr / totalTurns : 0;
    avgTurns = sp.length > 0 ? totalTurns / sp.length : 0;
    const totalRead = sp.reduce((s, x) => s + (x.cache_read_tokens || 0), 0);
    const totalAll = totalRead + sp.reduce((s, x) => s + (x.cache_creation_tokens || 0) + (x.input_tokens || 0), 0);
    cacheHitRate = totalAll > 0 ? totalRead / totalAll : 0;
  }

  const problems = [];
  if (avgDur >= 45) problems.push('session_length');
  if (compactN > 0) problems.push('context_reset');
  if (corrRate > 0.15) problems.push('correction_rate');

  let lead = '';
  if (problems.length === 0) {
    lead = 'Your sessions are running efficiently this month. Focused length, low friction, strong cache utilization.';
  } else if (problems.includes('session_length') && problems.includes('context_reset')) {
    lead = `Sessions averaging <strong>${avgDur} min</strong> and <strong>${compactN} context reset${compactN > 1 ? 's' : ''}</strong> are your biggest cost drivers this month.`;
  } else if (problems.includes('session_length')) {
    lead = `Sessions averaging <strong>${avgDur} min</strong> are the main cost driver this month. One task per session when switching context keeps per-turn cost down.`;
  } else if (problems.includes('context_reset')) {
    lead = `<strong>${compactN} session${compactN > 1 ? 's' : ''}</strong> hit the context limit this month. Break long work into sub-tasks before starting.`;
  } else if (problems.includes('correction_rate')) {
    lead = `A <strong>${Math.round(corrRate * 100)}% correction rate</strong> means many turns redirect Claude rather than advance the task.`;
  }

  const strengths = [];
  if (corrRate < 0.1 && sp && sp.length) {
    strengths.push(`<strong>${Math.round(corrRate * 100)}% correction rate.</strong> Your prompts are landing first time.`);
  }
  if (cacheHitRate > 0.8) {
    strengths.push(`<strong>${Math.round(cacheHitRate * 100)}% cache hit rate.</strong> Context warms up fast.`);
  }
  if (toolsPerReq > 0) {
    strengths.push(`ctx is filtering <strong>${toolsPerReq} tool schemas per request.</strong>`);
  }

  let action = '';
  if (problems.includes('session_length') && problems.includes('context_reset')) {
    action = 'When you start a new task, open a new session instead of continuing the current one.';
  } else if (problems.includes('session_length')) {
    action = 'One task per session. When a task feels complete, start fresh. A clean context window is cheaper per turn.';
  } else if (problems.includes('context_reset')) {
    action = 'Before a long session, break it into sub-tasks and plan the sequence.';
  } else if (problems.includes('correction_rate')) {
    action = 'Add a one-line output format to your opening prompt. It typically halves back-and-forth rounds.';
  } else if (problems.length === 0) {
    action = 'Sessions are running efficiently. Keep one task per session when switching context.';
  }

  return { lead, strengths, action };
}

// ─── Efficiency score ──────────────────────────────────────
function calcScore(sessions) {
  if (!sessions.length) return { score: 50, avgTurns: 0, compactN: 0, opusN: 0, corrRate: 0, cacheHitRate: 0 };
  let score = 100;
  const totalTurns = sessions.reduce((s, x) => s + x.turn_count, 0);
  const totalCorrections = sessions.reduce((s, x) => s + x.correction_turns, 0);
  const avgTurns = totalTurns / sessions.length;
  const compactN = sessions.filter(s => s.hit_compact).length;
  const opusN = sessions.filter(s => (s.models_used || []).includes('opus')).length;
  const corrRate = totalTurns > 0 ? totalCorrections / totalTurns : 0;

  const totalCacheRead = sessions.reduce((s, x) => s + (x.cache_read_tokens || 0), 0);
  const totalCacheCreate = sessions.reduce((s, x) => s + (x.cache_creation_tokens || 0), 0);
  const totalInput = sessions.reduce((s, x) => s + (x.input_tokens || 0), 0);
  const totalAllInput = totalCacheRead + totalCacheCreate + totalInput;
  const cacheHitRate = totalAllInput > 0 ? totalCacheRead / totalAllInput : 0;

  score -= Math.min(30, compactN * 5);
  const lst = _userProfile.long_session_threshold || 26;
  if (avgTurns > lst * 1.6) score -= 22;
  else if (avgTurns > lst * 1.0) score -= 14;
  else if (avgTurns > lst * 0.7) score -= 6;
  score -= Math.min(22, opusN * 8);
  if (corrRate > 0.25) score -= 14;
  else if (corrRate > 0.15) score -= 8;
  else if (corrRate > 0.08) score -= 4;
  if (cacheHitRate >= 0.90) score = Math.min(100, score + 5);

  return { score: Math.max(5, Math.min(100, Math.round(score))), avgTurns, compactN, opusN, corrRate, cacheHitRate };
}

function renderScore(sessions) {
  const { score, avgTurns, compactN, opusN, corrRate, cacheHitRate } = calcScore(sessions);
  const color = scoreColor(score);

  const sbGauge = document.getElementById('sb-gauge-ring');
  const circumference = 125.66;
  sbGauge.style.stroke = color;
  sbGauge.style.strokeDashoffset = circumference - (score / 100 * circumference);
  document.getElementById('sb-gauge-val').textContent = score;
  document.getElementById('sb-score-val').style.color = color;
  animateNum(document.getElementById('sb-score-val'), score, '', '', 0, 900);
  document.getElementById('sb-score-text').textContent = scoreLabel(score);
  document.getElementById('sb-score-card').style.display = 'block';

  const ring = document.getElementById('score-ring');
  const circumM = 213.63;
  ring.style.stroke = color;
  setTimeout(() => { ring.style.strokeDashoffset = circumM - (score / 100 * circumM); }, 100);
  const bigEl = document.getElementById('score-big');
  bigEl.style.color = color;
  animateNum(bigEl, score, '', '', 0, 1000);
  document.getElementById('score-label').textContent = scoreLabel(score);

  let desc = '';
  if (score >= 80) {
    desc = `You're in the top tier. Cache hits at ${Math.round(cacheHitRate * 100)}%, correction rate near zero, sessions staying focused.`;
  } else if (score >= 60) {
    desc = `Solid efficiency. ${compactN > 0 ? `${compactN} context reset${compactN > 1 ? 's' : ''} and ` : ''}sessions averaging ${Math.round(avgTurns)} turns are the main cost drivers.`;
  } else if (score >= 40) {
    desc = `${compactN > 0 ? `Context resets (${compactN} this month) and ` : ''}sessions averaging ${Math.round(avgTurns)} turns are eating into your budget.`;
  } else {
    desc = `${opusN > 0 ? `Opus in ${opusN} sessions, ` : ''}${compactN > 0 ? `${compactN} context resets, ` : ''}${Math.round(avgTurns)} turns on average.`;
  }
  document.getElementById('score-desc').textContent = desc;

  const _lst = _userProfile.long_session_threshold || 26;
  const compactPct = Math.max(0, 100 - compactN * 17);
  const turnPct = Math.max(0, 100 - Math.min(100, (avgTurns / (_lst * 2)) * 100));
  const opusPct = Math.max(0, 100 - opusN * 22);
  const corrPct = Math.max(0, 100 - corrRate * 200);
  const cachePct = Math.round(cacheHitRate * 100);

  function metricRow(label, pct, what, why) {
    const barColor = pct > 70 ? '#22c55e' : pct > 40 ? '#f59e0b' : '#ef4444';
    const badge = pct > 70 ? 'good' : pct > 40 ? 'warn' : 'bad';
    const badgeColors = { good: '#22c55e22', warn: '#f59e0b22', bad: '#ef444422' };
    const badgeText = { good: '#22c55e', warn: '#f59e0b', bad: '#ef4444' };
    const safeWhy = why.replace(/"/g, '&quot;');
    return `
    <div style="padding:7px 0;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:10px" title="${safeWhy}">
      <div style="font-size:12px;font-weight:600;color:var(--t1);width:112px;flex-shrink:0">${label}</div>
      <div style="flex:1;background:var(--surface2);border-radius:4px;height:4px;overflow:hidden">
        <div style="height:100%;border-radius:4px;width:${pct}%;background:${barColor};transition:width .6s .2s"></div>
      </div>
      <div style="font-size:11px;font-weight:700;padding:2px 8px;border-radius:10px;background:${badgeColors[badge]};color:${badgeText[badge]};white-space:nowrap;flex-shrink:0">${what}</div>
    </div>`;
  }

  const compactWhat = compactN === 0 ? 'No resets' : `${compactN} reset${compactN > 1 ? 's' : ''}`;
  const compactWhy = compactN === 0
    ? 'None of your sessions ran out of working memory this month.'
    : `${compactN} session${compactN > 1 ? 's' : ''} hit the context limit and reset mid-way.`;

  const lst = _userProfile.long_session_threshold || 26;
  const turnWhat = avgTurns < lst * 0.6 ? 'Short and focused' : avgTurns < lst ? `~${Math.round(avgTurns)} turns avg` : `${Math.round(avgTurns)} turns avg`;
  const turnWhy = avgTurns < lst * 0.6
    ? 'Your sessions are short and focused.'
    : `Your sessions average ${Math.round(avgTurns)} turns.`;

  const opusWhat = opusN === 0 ? 'Sonnet only' : `Opus in ${opusN} session${opusN > 1 ? 's' : ''}`;
  const opusWhy = opusN === 0 ? 'All sessions ran on Sonnet.' : `${opusN} session(s) used Opus.`;

  const ct = _userProfile.correction_threshold || 40;
  const corrWhat = corrRate === 0 ? 'None detected' : `${(corrRate * 100).toFixed(0)}% of turns`;
  const corrWhy = corrRate < 0.15
    ? `${(corrRate * 100).toFixed(0)}% of turns were short follow-ups.`
    : `${(corrRate * 100).toFixed(0)}% of turns were short redirects.`;

  const cacheWhat = `${cachePct}% cache hits`;
  const cacheWhy = `${cachePct}% of your input tokens are cache reads at $0.30/MTok.`;

  document.getElementById('score-breakdown').innerHTML =
    metricRow('Context resets', compactPct, compactWhat, compactWhy) +
    metricRow('Session length', Math.round(turnPct), turnWhat, turnWhy) +
    metricRow('Model choice', Math.round(opusPct), opusWhat, opusWhy) +
    metricRow('Correction rate', Math.round(corrPct), corrWhat, corrWhy) +
    metricRow('Cache efficiency', Math.min(100, cachePct), cacheWhat, cacheWhy).replace('border-bottom:1px solid var(--border)', 'border-bottom:none');
}
