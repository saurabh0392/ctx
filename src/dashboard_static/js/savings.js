// ─── Tab 1: Savings ───────────────────────────────────────
async function loadSavings() {
  const now = new Date();
  const monthStr = now.toISOString().slice(0, 7);
  const monthLabel = now.toLocaleDateString('en-US', {month: 'long', year: 'numeric'});

  const [stats, timeline, sessions, spendRaw, projectsRows, toolHeat] = await Promise.all([
    fetch(appendSince('/api/stats')).then(r => r.json()),
    fetch(appendSince('/api/timeline')).then(r => r.json()),
    fetch(appendSince('/api/sessions')).then(r => r.json()),
    fetch(appendSince(`/api/spend/sessions?month=${monthStr}`)).then(r => r.json()).catch(() => []),
    fetch(appendSince('/api/projects')).then(r => r.json()).catch(() => []),
    fetch(appendSince('/api/tool-usage')).then(r => r.json()).catch(() => []),
  ]);
  const spendSessions = Array.isArray(spendRaw) ? spendRaw : [];
  updateCtxRangeMetaFromStats(stats);

  // Proxy status
  const dot = document.getElementById('status-dot');
  const txt = document.getElementById('status-text');
  if (stats.sessions_fallback) {
    dot.className = 'status-dot on';
    txt.textContent = 'Session index (post-ctx sessions only)';
  } else {
    dot.className = 'status-dot ' + (stats.proxy_listening ? 'on' : 'off');
    txt.textContent = stats.proxy_listening ? 'Proxy active' : 'Proxy offline';
  }
  const sockEl = document.getElementById('socket-status');
  if (sockEl) sockEl.textContent = 'Event stream: active';

  // Core derived metrics
  const oldest = sessions.length ? sessions[sessions.length - 1] : null;
  const days = oldest ? Math.max(1, Math.ceil((Date.now() - new Date(oldest.started_at)) / 86400000)) : 0;
  const avgDurMins = sessions.length ? Math.round(sessions.reduce((s,x) => s + x.duration_mins, 0) / sessions.length) : 0;
  const removedPerReq = stats.request_count > 0 ? Math.round(stats.total_tools_removed / stats.request_count) : 0;
  const keptPerReq = stats.request_count > 0 ? Math.round((stats.total_tools_kept || 0) / stats.request_count) : 0;
  const totalPerReq = removedPerReq + keptPerReq;
  const annualRate = days >= 7 ? Math.round(stats.cost_saved / days * 365) : 0;
  const monthSaved = timeline.filter(p => p.date.startsWith(monthStr)).reduce((s, p) => s + p.cost, 0);
  const perSession = stats.session_count > 0 ? stats.cost_saved / stats.session_count : 0;

  // ── Narrative card ───────────────────────────────────────
  document.getElementById('narrative-eyebrow').textContent = `ctx usage summary · ${monthLabel}`;

  let narrative = '';
  if (days > 0 && stats.session_count > 0) {
    const n = stats.session_count;
    const cost = fmtCost(stats.cost_saved);

    let lead = '';
    if (days === 1)       lead = `Not bad for day one.`;
    else if (days < 4)    lead = `${days} days in.`;
    else if (days < 14)   lead = `About a week in.`;
    else if (days < 45)   lead = `${Math.round(days/7)} weeks in.`;
    else                  lead = `${Math.round(days/30)} month${days >= 60 ? 's' : ''} running.`;

    let sessLine = '';
    if (n === 1 && avgDurMins > 0) {
      sessLine = `One session, ${avgDurMins} minutes.`;
    } else if (days === 1) {
      sessLine = `${n} sessions` + (avgDurMins > 0 ? `, ${avgDurMins} minutes on average` : '') + `.`;
    } else {
      sessLine = `You've run <strong>${n} sessions</strong>` + (avgDurMins > 0 ? `, averaging ${avgDurMins} minutes each` : '') + `.`;
    }

    let toolLine = '';
    if (totalPerReq > 0 && removedPerReq > 0) {
      const pct = Math.round(removedPerReq / totalPerReq * 100);
      toolLine = `Each request started with <strong>${totalPerReq} tools</strong>. ` +
        `Your <strong>${esc(stats.active_profile)}</strong> profile kept ${keptPerReq} and stripped ${removedPerReq} (${pct}% reduction).`;
    } else if (removedPerReq > 0) {
      toolLine = `ctx stripped ${removedPerReq} tool schemas per request using the <strong>${esc(stats.active_profile)}</strong> profile.`;
    }

    let costLine = '';
    if (stats.cost_saved > 0) {
      costLine = `That saved <strong>${cost}</strong> in context tokens`;
      if (annualRate > 0) {
        costLine += `, on pace for <strong>$${annualRate.toLocaleString()}</strong> by year end`;
      }
      costLine += `.`;
    }

    narrative = [lead, sessLine, toolLine, costLine].filter(Boolean).join(' ');
  } else if (stats.session_count > 0) {
    const spendTotal = spendSessions.reduce((s, x) => s + (x.total_usd || 0), 0);
    narrative = `Tracking <strong>${stats.session_count} sessions</strong> this month (<strong>${fmtCost(spendTotal)}</strong> total spend). `;
    if (stats.sessions_fallback) {
      narrative += `Per-request filter rows are not in ctx.db yet. Sessions come from <code>ctx ingest</code> on Claude JSONL. Start the dashboard when using Claude Code so <code>filter.js</code> dual-writes into SQLite.`;
    }
    if (stats.monthly_burn_projection_usd > 0) {
      narrative += ` Month-end projection: <strong>${fmtCost(stats.monthly_burn_projection_usd)}</strong>.`;
    }
  } else {
    narrative = `ctx is installed and running. Your summary shows up here after the first session.`;
  }
  document.getElementById('narrative-body').innerHTML = narrative;

  // Stat pills
  const pills = [];
  const spendSum = spendSessions.reduce((s, x) => s + (x.total_usd || 0), 0);
  if (stats.session_count > 0)  pills.push(`<span>${stats.session_count}</span> sessions this month`);
  if (monthSaved > 0)           pills.push(`<span>${fmtCost(monthSaved)}</span> saved this month`);
  else if (spendSum > 0)        pills.push(`<span>${fmtCost(spendSum)}</span> total spend`);
  if (perSession > 0)           pills.push(`<span>${fmtCost(perSession)}</span> per session avg`);
  if (annualRate > 0)           pills.push(`<span>$${annualRate}</span> projected this year`);
  else if (stats.monthly_burn_projection_usd > 0)
    pills.push(`<span>${fmtCost(stats.monthly_burn_projection_usd)}</span> month-end projection`);
  document.getElementById('narrative-pills').innerHTML = pills.map(p => `<div class="narrative-pill">${p}</div>`).join('');

  const onboardEl = document.getElementById('onboarding-wrap');
  const wizDone = localStorage.getItem('ctx-onboarding-done');
  if (onboardEl) {
    const showWiz = !wizDone && stats.request_count === 0 && spendSum < 0.01;
    onboardEl.style.display = showWiz ? 'block' : 'none';
    if (showWiz) { wizPrepareFromServer(); wizShowStep(1); }
  }
  const econCard = document.getElementById('savings-econ-card');
  const econBody = document.getElementById('savings-econ-body');
  if (econCard && econBody) {
    const worst = stats.cost_saved_worst_case != null ? stats.cost_saved_worst_case : 0;
    if (stats.total_tokens_saved > 0 || stats.request_count > 0) {
      econCard.style.display = 'block';
      const keptTotal = stats.total_tools_kept || 0;
      const allTotal = keptTotal + stats.total_tools_removed;
      const pctStrip = allTotal > 0 ? Math.round(stats.total_tools_removed / allTotal * 100) : 0;
      let econText = '';
      if (allTotal > 0) {
        econText += 'Your <strong>' + esc(stats.active_profile) + '</strong> profile kept ' + keptTotal + ' of ' + allTotal + ' total tool slots (' + pctStrip + '% stripped). ';
      }
      econText += 'Savings use <strong>$0.30 / MTok</strong> (cache read rate). At full Sonnet input pricing the same volume would cost <strong>' + fmtCost(worst) + '</strong>. ';
      econText += 'The first request per session pays full input price; subsequent requests benefit from prompt caching. Your real saving sits between these two.';
      econBody.innerHTML = econText;
    } else {
      econCard.style.display = 'none';
    }
  }

  // ── Persona card ─────────────────────────────────────────
  const persona = computePersona(spendSessions);
  if (persona) {
    document.getElementById('persona-icon').textContent = persona.icon;
    document.getElementById('persona-name').textContent = persona.name;
    document.getElementById('persona-desc').textContent = persona.desc;
    document.getElementById('persona-traits').innerHTML = persona.traits.map(t =>
      `<div class="persona-trait"><span class="persona-trait-label">${t.label}</span><span class="persona-trait-val">${t.val}</span></div>`
    ).join('');
  } else {
    document.getElementById('persona-icon').textContent = '📊';
    document.getElementById('persona-name').textContent = 'Building your profile';
    document.getElementById('persona-desc').textContent = 'Your prompt persona is calculated from session turn patterns. It will appear after a few sessions of data.';
    document.getElementById('persona-traits').innerHTML = '';
  }

  // ── Insights ─────────────────────────────────────────────
  const { lead, strengths, action } = buildInsights(stats, sessions, spendSessions);
  document.getElementById('insights-list').innerHTML = `
    <p style="color:var(--t1);font-size:14px;line-height:1.75;margin:0 0 ${strengths.length ? 18 : 0}px">${lead}</p>
    ${strengths.length ? `
      <div style="margin-bottom:${action ? 16 : 0}px">
        <div style="font-size:10px;font-weight:700;color:var(--t3);text-transform:uppercase;letter-spacing:.08em;margin-bottom:9px">Working in your favor</div>
        <ul style="margin:0;padding-left:16px;color:var(--t2);font-size:13px;line-height:2">
          ${strengths.map(s => `<li>${s}</li>`).join('')}
        </ul>
      </div>` : ''}
    ${action ? `
      <div style="font-size:13px;color:var(--t2);border-top:1px solid var(--border);padding-top:13px;line-height:1.65">
        <span style="font-size:10px;font-weight:700;color:var(--t3);text-transform:uppercase;letter-spacing:.08em;display:block;margin-bottom:5px">Try this</span>
        ${action}
      </div>` : ''}
  `;

  renderProjectsPanel(Array.isArray(projectsRows) ? projectsRows : [], !!stats.sessions_fallback);
  renderToolUsagePanel(Array.isArray(toolHeat) ? toolHeat : []);

  // ── Cumulative savings chart ──────────────────────────────
  let running = 0;
  const cumul = timeline.map(p => { running += p.cost; return { date: p.date, total: +running.toFixed(4) }; });
  // Prepend a zero anchor point so single-day data has a visible line
  if (cumul.length) {
    const firstDate = new Date(cumul[0].date);
    firstDate.setDate(firstDate.getDate() - 1);
    cumul.unshift({ date: firstDate.toISOString().slice(0, 10), total: 0 });
  }
  const singlePoint = cumul.length === 2; // anchor + one real point
  if (cumul.length && typeof Chart !== 'undefined') {
    const chartEl = document.getElementById('timeline-chart').getContext('2d');
    if (window._timelineChart) { window._timelineChart.destroy(); window._timelineChart = null; }
    const grad = chartEl.createLinearGradient(0, 0, 0, 220);
    grad.addColorStop(0, 'rgba(147,192,67,.22)');
    grad.addColorStop(1, 'rgba(147,192,67,.0)');
    window._timelineChart = new Chart(chartEl, {
      type: 'line',
      data: {
        labels: cumul.map(p => { const d = new Date(p.date); return d.toLocaleDateString('en-US', {month:'short', day:'numeric'}); }),
        datasets: [{ label: stats.sessions_fallback ? 'Cumulative spend' : 'Total saved', data: cumul.map(p => p.total),
          borderColor: '#93c043', backgroundColor: grad,
          borderWidth: 2.5, pointRadius: singlePoint ? [0, 5] : 0, pointHoverRadius: 5,
          pointBackgroundColor: '#b0d46a', fill: true, tension: 0.45 }]
      },
      options: {
        responsive: true, maintainAspectRatio: false, animation: { duration: 1000 },
        plugins: { legend: { display: false },
          tooltip: { backgroundColor: '#1f2e33', borderColor: '#2e4048', borderWidth: 1, padding: 12,
            callbacks: { label: c => '  ' + (stats.sessions_fallback ? 'Spend: ' : 'Total saved: ') + fmtCost(c.parsed.y) } } },
        scales: {
          x: { grid: { color: '#2e4048', lineWidth: .5 }, ticks: { color: '#4d7060', font: {size: 11} } },
          y: { grid: { color: '#1f2a30', lineWidth: .5 }, ticks: { color: '#4d7060', font: {size: 11}, callback: v => '$' + v.toFixed(2) }, beginAtZero: true }
        }
      }
    });
  }

  // ── Compact sessions ──────────────────────────────────────
  const sessEl = document.getElementById('sessions-compact');
  if (!sessions.length) {
    sessEl.innerHTML = `<div class="empty">No sessions yet.</div>`;
  } else {
    sessEl.innerHTML = sessions.slice(0, 8).map(s => {
    const dur = s.duration_mins < 60 ? `${s.duration_mins}m` : `${Math.floor(s.duration_mins/60)}h ${s.duration_mins % 60}m`;
    return `<div class="sv-session">
      <div>
        <div class="sv-session-date">${fmtDate(s.started_at)}</div>
        <div class="sv-session-meta">${dur} &middot; ${s.requests} requests</div>
      </div>
      <div class="sv-session-cost">${fmtCost(s.cost)}</div>
    </div>`;
  }).join('');
  }

  loadProfileAnalytics().catch(() => {});
}

function computePersona(sp) {
  if (!sp || !sp.length) return null;
  const totalTurns = sp.reduce((s,x) => s + (x.turn_count||0), 0);
  if (totalTurns === 0) return null;
  const avgTurns = totalTurns / sp.length;
  const corrRate  = sp.reduce((s,x) => s + (x.correction_turns||0), 0) / totalTurns;
  const compactN  = sp.filter(s => s.hit_compact).length;
  const compactRate = compactN / sp.length;

  if (compactRate > 0.35) return {
    icon: '🤿', name: 'The Deep Diver',
    desc: 'You work through large, complex problems end-to-end. Sessions regularly push the limits of working memory.',
    traits: [{label:'Avg session length', val:`${Math.round(avgTurns)} turns`}, {label:'Context resets', val:compactN}, {label:'Approach', val:'Thorough'}]
  };
  if (avgTurns > 28 && corrRate < 0.12) return {
    icon: '🏗', name: 'The Architect',
    desc: 'Long, focused sessions with clear direction. You think through complex problems systematically before asking.',
    traits: [{label:'Avg session length', val:`${Math.round(avgTurns)} turns`}, {label:'Correction rate', val:`${Math.round(corrRate*100)}%`}, {label:'Approach', val:'Systematic'}]
  };
  if (corrRate > 0.22) return {
    icon: '🔄', name: 'The Refiner',
    desc: 'You iterate toward precision. High correction rate signals you know exactly what you want and push until you get it.',
    traits: [{label:'Correction rate', val:`${Math.round(corrRate*100)}%`}, {label:'Avg session length', val:`${Math.round(avgTurns)} turns`}, {label:'Approach', val:'Iterative'}]
  };
  if (avgTurns < 12 && corrRate < 0.1) return {
    icon: '⚡', name: 'The Sprinter',
    desc: 'Short, decisive sessions with clear intent. You get in, get the answer, get out.',
    traits: [{label:'Avg session length', val:`${Math.round(avgTurns)} turns`}, {label:'Correction rate', val:`${Math.round(corrRate*100)}%`}, {label:'Approach', val:'Decisive'}]
  };
  return {
    icon: '🔨', name: 'The Builder',
    desc: 'Steady, productive sessions. A balanced mix of exploration and execution.',
    traits: [{label:'Avg session length', val:`${Math.round(avgTurns)} turns`}, {label:'Correction rate', val:`${Math.round(corrRate*100)}%`}, {label:'Approach', val:'Balanced'}]
  };
}

function buildInsights(stats, sessions, sp) {
  const avgDur = sessions.length
    ? Math.round(sessions.reduce((s,x) => s + x.duration_mins, 0) / sessions.length)
    : 0;
  const toolsPerReq = stats.request_count > 0
    ? Math.round(stats.total_tools_removed / stats.request_count)
    : 0;

  let compactN = 0, corrRate = 0, avgTurns = 0, cacheHitRate = 0;
  if (sp && sp.length) {
    compactN = sp.filter(s => s.hit_compact).length;
    const totalTurns = sp.reduce((s,x) => s + (x.turn_count||0), 0);
    const totalCorr  = sp.reduce((s,x) => s + (x.correction_turns||0), 0);
    corrRate  = totalTurns > 0 ? totalCorr / totalTurns : 0;
    avgTurns  = sp.length > 0 ? totalTurns / sp.length : 0;
    const totalRead = sp.reduce((s,x) => s + (x.cache_read_tokens||0), 0);
    const totalAll  = totalRead + sp.reduce((s,x) => s + (x.cache_creation_tokens||0) + (x.input_tokens||0), 0);
    cacheHitRate = totalAll > 0 ? totalRead / totalAll : 0;
  }

  // Classify signals
  const problems = [];
  if (avgDur >= 45) problems.push('session_length');
  if (compactN > 0) problems.push('context_reset');
  if (corrRate > 0.15) problems.push('correction_rate');

  // Lead paragraph — synthesizes the biggest cost drivers into one narrative sentence
  let lead = '';
  if (problems.length === 0) {
    lead = `Your sessions are running efficiently this month. Focused length, low friction, strong cache utilization. The score breakdown shows where there is still room to go.`;
  } else if (problems.includes('session_length') && problems.includes('context_reset')) {
    lead = `Sessions averaging <strong>${avgDur} min</strong> and <strong>${compactN} context reset${compactN>1?'s':''}</strong> are your biggest cost drivers this month. These two compound: long sessions are more likely to hit the context limit, and each reset forces Claude to rebuild working state from scratch, paying the re-read cost twice.`;
  } else if (problems.includes('session_length')) {
    lead = `Sessions averaging <strong>${avgDur} min</strong> are the main cost driver this month. Claude re-reads the full conversation context on every turn, so each turn in a long session costs more than the one before. Breaking work into shorter, focused sessions is the fastest lever.`;
  } else if (problems.includes('context_reset')) {
    lead = `<strong>${compactN} session${compactN>1?'s':''}</strong> hit the context limit this month and had to reset mid-way. Each reset forces Claude to rebuild its working state from a summary, paying extra tokens to re-establish context before the actual task continues.`;
  } else if (problems.includes('correction_rate')) {
    lead = `A <strong>${Math.round(corrRate*100)}% correction rate</strong> means roughly 1 in ${Math.round(1/corrRate)} messages is redirecting Claude rather than advancing the task. That is billable context with no output to show for it, and it pushes sessions longer, compounding cost.`;
  }

  // Strength bullets
  const strengths = [];
  if (corrRate < 0.1 && sp && sp.length)
    strengths.push(`<strong>${Math.round(corrRate*100)}% correction rate.</strong> Your prompts are landing first time with minimal wasted context.`);
  if (cacheHitRate > 0.8)
    strengths.push(`<strong>${Math.round(cacheHitRate*100)}% cache hit rate.</strong> Context warms up fast, keeping per-turn cost low.`);
  if (toolsPerReq > 0) {
    const pName = PROFILE_LABELS[stats.active_profile] || stats.active_profile || 'Current';
    strengths.push(`ctx is filtering <strong>${toolsPerReq} tool schemas per request.</strong> Tokens Claude never sees before your prompt.`);
  }
  if (!problems.includes('session_length') && avgDur > 0)
    strengths.push(`Sessions averaging <strong>${avgDur} min.</strong> Under the threshold where context costs start compounding.`);
  if (!problems.includes('context_reset') && sp && sp.length)
    strengths.push(`No context resets this month. Sessions stayed within working memory.`);

  // Concrete action
  let action = '';
  if (problems.includes('session_length') && problems.includes('context_reset')) {
    action = 'When you start a new task, open a new session instead of continuing the current one. It\'s consistently cheaper than extending a session past the context limit.';
  } else if (problems.includes('session_length')) {
    action = 'One task per session. When a task feels complete or you\'re switching contexts, start fresh. A clean context window is cheaper per turn than a full one.';
  } else if (problems.includes('context_reset')) {
    action = 'Before a long session, break it into sub-tasks and plan the sequence. Staying within one context window avoids the rebuild overhead.';
  } else if (problems.includes('correction_rate')) {
    action = 'Add a one-line output format to your opening prompt. "Respond as a numbered list" or "output a unified diff." It typically halves back-and-forth rounds.';
  }

  return { lead, strengths, action };
}

// ─── Efficiency score ──────────────────────────────────────
function calcScore(sessions) {
  if (!sessions.length) return { score: 50, avgTurns: 0, compactN: 0, opusN: 0, corrRate: 0, cacheHitRate: 0 };
  let score = 100;
  const totalTurns = sessions.reduce((s,x) => s + x.turn_count, 0);
  const totalCorrections = sessions.reduce((s,x) => s + x.correction_turns, 0);
  const avgTurns = totalTurns / sessions.length;
  const compactN = sessions.filter(s => s.hit_compact).length;
  const opusN = sessions.filter(s => s.models_used.includes('opus')).length;
  const corrRate = totalTurns > 0 ? totalCorrections / totalTurns : 0;

  const totalCacheRead = sessions.reduce((s,x) => s + (x.cache_read_tokens || 0), 0);
  const totalCacheCreate = sessions.reduce((s,x) => s + (x.cache_creation_tokens || 0), 0);
  const totalInput = sessions.reduce((s,x) => s + (x.input_tokens || 0), 0);
  const totalAllInput = totalCacheRead + totalCacheCreate + totalInput;
  const cacheHitRate = totalAllInput > 0 ? totalCacheRead / totalAllInput : 0;

  // Context resets are costly
  score -= Math.min(30, compactN * 5);
  // Long sessions — thresholds calibrated to user's own median session length
  const lst = _userProfile.long_session_threshold || 26;
  if (avgTurns > lst * 1.6) score -= 22;
  else if (avgTurns > lst * 1.0) score -= 14;
  else if (avgTurns > lst * 0.7) score -= 6;
  // Opus overuse
  score -= Math.min(22, opusN * 8);
  // Correction rate
  if (corrRate > 0.25) score -= 14;
  else if (corrRate > 0.15) score -= 8;
  else if (corrRate > 0.08) score -= 4;
  // Cache efficiency bonus (up to +5 for >90% hit rate)
  if (cacheHitRate >= 0.90) score = Math.min(100, score + 5);

  return { score: Math.max(5, Math.min(100, Math.round(score))), avgTurns, compactN, opusN, corrRate, cacheHitRate };
}

function renderScore(sessions) {
  const { score, avgTurns, compactN, opusN, corrRate, cacheHitRate } = calcScore(sessions);
  const color = scoreColor(score);

  // Sidebar gauge
  const sbGauge = document.getElementById('sb-gauge-ring');
  const circumference = 125.66;
  sbGauge.style.stroke = color;
  sbGauge.style.strokeDashoffset = circumference - (score / 100 * circumference);
  document.getElementById('sb-gauge-val').textContent = score;
  document.getElementById('sb-score-val').style.color = color;
  animateNum(document.getElementById('sb-score-val'), score, '', '', 0, 900);
  document.getElementById('sb-score-text').textContent = scoreLabel(score);
  document.getElementById('sb-score-card').style.display = 'block';

  // Main ring
  const ring = document.getElementById('score-ring');
  const circumM = 213.63;
  ring.style.stroke = color;
  setTimeout(() => { ring.style.strokeDashoffset = circumM - (score / 100 * circumM); }, 100);
  const bigEl = document.getElementById('score-big');
  bigEl.style.color = color;
  animateNum(bigEl, score, '', '', 0, 1000);
  document.getElementById('score-label').textContent = scoreLabel(score);
  // Narrative score description using actual numbers
  let desc = '';
  if (score >= 80) {
    desc = `You're in the top tier. Cache hits at ${Math.round(cacheHitRate*100)}%, correction rate near zero, sessions staying focused. Not much to change here.`;
  } else if (score >= 60) {
    desc = `Solid efficiency. ${compactN > 0 ? `${compactN} context reset${compactN>1?'s':''} and ` : ''}sessions averaging ${Math.round(avgTurns)} turns are the main cost drivers. A few habit tweaks would push this above 80.`;
  } else if (score >= 40) {
    desc = `${compactN > 0 ? `Context resets (${compactN} this month) and ` : ''}sessions averaging ${Math.round(avgTurns)} turns are eating into your budget. The breakdown shows exactly where.`;
  } else {
    desc = `${opusN > 0 ? `Opus in ${opusN} sessions, ` : ''}${compactN > 0 ? `${compactN} context resets, ` : ''}${Math.round(avgTurns)} turns on average. Several patterns compounding cost. Start with whichever metric is red below.`;
  }
  document.getElementById('score-desc').textContent = desc;

  // Breakdown — narrative rows
  const _lst = _userProfile.long_session_threshold || 26;
  const compactPct = Math.max(0, 100 - compactN * 17);
  const turnPct    = Math.max(0, 100 - Math.min(100, (avgTurns / (_lst * 2)) * 100));
  const opusPct    = Math.max(0, 100 - opusN * 22);
  const corrPct    = Math.max(0, 100 - corrRate * 200);
  const cachePct   = Math.round(cacheHitRate * 100);

  function metricRow(label, pct, what, why) {
    const barColor = pct > 70 ? '#22c55e' : pct > 40 ? '#f59e0b' : '#ef4444';
    const badge = pct > 70 ? 'good' : pct > 40 ? 'warn' : 'bad';
    const badgeColors = { good: '#22c55e22', warn: '#f59e0b22', bad: '#ef444422' };
    const badgeText   = { good: '#22c55e',   warn: '#f59e0b',   bad: '#ef4444' };
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
    ? 'None of your sessions ran out of working memory this month. Focused, single-task sessions tend to stay within limits.'
    : `${compactN} session${compactN > 1 ? 's' : ''} hit the context limit and reset mid-way. Each reset costs extra as Claude rebuilds context from scratch. Splitting long work across multiple sessions helps.`;

  const lst = _userProfile.long_session_threshold || 26;
  const turnWhat = avgTurns < lst * 0.6 ? 'Short and focused' : avgTurns < lst ? `~${Math.round(avgTurns)} turns avg` : `${Math.round(avgTurns)} turns avg`;
  const turnWhy = avgTurns < lst * 0.6
    ? 'Your sessions are short and focused. This is the most cost-efficient pattern. Claude stays sharp and context stays cheap.'
    : avgTurns < lst
    ? `Averaging ${Math.round(avgTurns)} turns per session, within your normal range. Context cost compounds steadily past turn ${Math.round(lst)}.`
    : `Your sessions average ${Math.round(avgTurns)} turns, above your typical ${_userProfile.median_session_turns}-turn median. Past your norm, context overhead compounds on every turn.`;

  const opusWhat = opusN === 0 ? 'Sonnet only' : `Opus in ${opusN} session${opusN > 1 ? 's' : ''}`;
  const opusWhy = opusN === 0
    ? 'All sessions ran on Sonnet, which is 5x cheaper than Opus and handles the vast majority of engineering and writing tasks at the same quality.'
    : `${opusN} session${opusN > 1 ? 's' : ''} used Opus, which costs $15/MTok vs $3/MTok for Sonnet. Opus is worth it for genuinely complex reasoning, but most tasks don't need it.`;

  const ct = _userProfile.correction_threshold || 40;
  const corrWhat = corrRate === 0 ? 'None detected' : `${(corrRate * 100).toFixed(0)}% of turns`;
  const corrWhy = !_userProfile.calibrated
    ? 'Still learning your writing patterns. Correction detection calibrates to your message length baseline after 5+ sessions.'
    : corrRate < 0.05
    ? `Almost no short follow-up turns detected (calibrated to your typical ${ct}-char message baseline). Your prompts are landing first time.`
    : corrRate < 0.15
    ? `${(corrRate * 100).toFixed(0)}% of turns were short follow-ups after long Claude responses, likely corrections or redirects. Adding explicit output format to your opening prompt typically cuts this in half.`
    : `${(corrRate * 100).toFixed(0)}% of turns were short redirects. Claude needed course-correcting frequently. "Goal / Context / Output format" in the first message is the single highest-leverage prompt habit.`;

  const cacheWhat = `${cachePct}% cache hits`;
  const cacheWhy = cachePct >= 90
    ? `${cachePct}% of your input tokens are cache reads at $0.30/MTok. You're getting the full benefit of Anthropic's prompt cache. Long, focused sessions build a warm cache that makes each follow-up turn much cheaper.`
    : cachePct >= 70
    ? `${cachePct}% cache hit rate. Your context is being reused well. Cache reads cost 10x less than fresh input. The higher this number, the better.`
    : `${cachePct}% cache hit rate. Frequently starting new sessions or changing system prompts prevents the cache from warming up, costing more per turn.`;

  document.getElementById('score-breakdown').innerHTML =
    metricRow('Context resets', compactPct, compactWhat, compactWhy) +
    metricRow('Session length', Math.round(turnPct), turnWhat, turnWhy) +
    metricRow('Model choice', Math.round(opusPct), opusWhat, opusWhy) +
    metricRow('Correction rate', Math.round(corrPct), corrWhat, corrWhy) +
    metricRow('Cache efficiency', Math.min(100, cachePct), cacheWhat, cacheWhy).replace('border-bottom:1px solid var(--border)', 'border-bottom:none');
}

