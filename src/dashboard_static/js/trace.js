// ─── Tab 4: Request Trace ────────────────────────────────
async function loadTrace() {
  const [requests, hookTraces] = await Promise.all([
    fetch(appendSince('/api/requests?limit=100')).then(r => r.json()),
    fetch(appendSince('/api/hook-traces?limit=100')).then(r => r.json()).catch(() => []),
  ]);
  const el = document.getElementById('trace-list');

  if (!requests.length && !hookTraces.length) {
    el.innerHTML = `<div class="card" style="padding:20px;margin-bottom:16px;border-color:rgba(147,192,67,.25)">
      <div class="section-head" style="margin-bottom:8px">No trace events</div>
      <div class="section-sub" style="margin-bottom:0">No trace events recorded yet. Use Claude Code with ctx hooks enabled. Each turn records a trace row automatically.</div>
    </div>`;
    return;
  }

  const todayStr = new Date().toISOString().slice(0, 10);
  const todayReqs = requests.filter(r => r.ts.slice(0, 10) === todayStr);
  const todayHookTraces = hookTraces.filter(h => h.ts.slice(0, 10) === todayStr);
  const todayTokens = todayReqs.reduce((s, r) => s + r.tokens_saved, 0);
  const todayCost = todayReqs.reduce((s, r) => s + r.cost_saved, 0);
  const totalToday = todayReqs.length + todayHookTraces.length;
  const autoCount = todayReqs.filter(r => r.auto_selected).length + todayHookTraces.filter(h => h.auto_selected).length;
  const autoLine = autoCount > 0
    ? ` Profile auto-switched ${autoCount} time${autoCount!==1?'s':''}.`
    : '';

  let bannerParts = [];
  if (todayReqs.length) bannerParts.push(`${todayReqs.length} proxy traces`);
  if (todayHookTraces.length) bannerParts.push(`${todayHookTraces.length} hook traces`);
  if (todayTokens > 0) bannerParts.push(`${fmtK(todayTokens)} tokens stripped`);

  const banner = totalToday ? `
    <div style="background:var(--surface2);border:1px solid var(--border);border-radius:10px;padding:14px 18px;margin-bottom:16px;display:flex;gap:32px;align-items:center">
      <div>
        <div style="font-size:11px;color:var(--t3);text-transform:uppercase;letter-spacing:.05em">Today</div>
        ${todayCost > 0 ? `<div style="font-size:22px;font-weight:700;color:var(--green)">-$${todayCost.toFixed(3)}</div>` : `<div style="font-size:22px;font-weight:700;color:var(--t2)">${totalToday} turns</div>`}
        <div style="font-size:11px;color:var(--t3)">${bannerParts.join(' &middot; ')}${autoLine}</div>
      </div>
      <div style="flex:1;text-align:right;font-size:11px;color:var(--t4)">Click any row to expand details.</div>
    </div>` : '';

  // Merge into unified timeline
  const unified = [];
  for (const req of requests) unified.push({ type: 'request', ts: req.ts, data: req });
  for (const ht of hookTraces) unified.push({ type: 'hook_trace', ts: ht.ts, data: ht });
  unified.sort((a, b) => b.ts.localeCompare(a.ts));

  el.innerHTML = banner + unified.map((item, i) => {
    if (item.type === 'request') return traceRow(item.data, i);
    return hookTraceRow(item.data, i);
  }).join('');
}

function renderAbBadges(abGroup) {
  if (!abGroup) return '';
  return abGroup.split(/\s+/).filter(Boolean).map(p => {
    const m = p.match(/^([PIAC]):([TC])$/);
    if (!m) return '';
    const cls = m[2] === 'T' ? 'ab-chip-t' : 'ab-chip-c';
    return `<span class="ab-chip ${cls}">${esc(p)}</span>`;
  }).join('');
}

function renderCtxBulletList(items) {
  return '<ul class="ctx-bullet-list">' + items.map(it => {
    const detail = it.detail
      ? `<span class="ctx-bullet-toggle" onclick="event.stopPropagation();this.parentElement.classList.toggle('expanded')">+</span><div class="ctx-bullet-detail">${it.detail}</div>`
      : '';
    return `<li class="ctx-bullet">${it.line}${detail}</li>`;
  }).join('') + '</ul>';
}

function hookTraceRow(ht, i) {
  const ts = fmtTs(ht.ts);
  const profileLabel = ht.profile || 'all';
  const autoChip = ht.auto_selected
    ? `<span class="trace-auto-chip">auto: ${ht.auto_trigger || 'matched'}</span>`
    : '';
  const modeChip = ht.mode
    ? `<span class="trace-profile-chip" title="This request used the ${esc(ht.mode)} mode (profile and feature toggles bundled)">${esc(ht.mode)}</span>`
    : '';

  const enrichedBadge = ht.enriched
    ? ''
    : '<span style="font-size:9px;padding:1px 6px;border-radius:3px;background:#3a3a20;color:#c8b44a;margin-left:8px">awaiting ingest</span>';

  const costUsd = ht.cost_usd || 0;
  const inputTok = ht.input_tokens || 0;
  const outputTok = ht.output_tokens || 0;
  const cacheRead = ht.cache_read_tokens || 0;
  const cacheCreate = ht.cache_creation_tokens || 0;
  const model = ht.model || '';
  const totalTok = inputTok + outputTok + cacheRead + cacheCreate;

  const toolsKept = ht.tools_kept || 0;
  const toolsRemoved = ht.tools_removed || 0;
  const tokensSaved = ht.tokens_saved || 0;
  const totalTools = toolsKept + toolsRemoved;
  const pctCut = totalTools > 0 ? Math.round(toolsRemoved / totalTools * 100) : 0;
  const savingsCost = (tokensSaved / 1_000_000) * 0.30;

  const ctxItems = [];
  ctxItems.push({
    line: `Applied <strong>${esc(profileLabel)}</strong> profile (via allowedMcpServers).`,
    detail: toolsRemoved > 0
      ? `Kept ${toolsKept} tools, stripped ${toolsRemoved} (${pctCut}% cut).`
      : 'All MCP servers allowed (no strip).'
  });
  if (ht.inject_fired) {
    ctxItems.push({ line: 'Prepended <strong>system_prefix.md</strong>.', detail: 'Static prefix from ~/.ctx/system_prefix.md.' });
  }
  if (ht.adaptive_fired) {
    ctxItems.push({ line: 'Appended <strong>adaptive_prefix.md</strong>.', detail: 'Behavioral profile from indexed sessions.' });
  }
  if (ht.coach_kind) {
    ctxItems.push({ line: `Coaching: <strong>${esc(ht.coach_kind)}</strong>.`, detail: 'Suggestion injected into additionalContext.' });
  }
  if (ht.budget_fired) ctxItems.push({ line: 'Session cost alert fired.', detail: '' });
  const ctxBullets = renderCtxBulletList(ctxItems);

  const promptPreview = ht.human_text_prefix
    ? `<div class="trace-prompt-preview" onclick="event.stopPropagation();this.classList.toggle('expanded')">${esc(ht.human_text_prefix)}</div>`
    : '<div class="trace-prompt-preview" style="color:var(--t4)">Prompt text available after next ingest.</div>';
  const abBadges = renderAbBadges(ht.ab_group);

  const savingsBar = totalTools > 0 ? `<div class="trace-token-impact">
      <div class="trace-token-impact-title">Token impact</div>
      <div class="trace-token-bar">
        <div class="trace-token-bar-removed" style="width:${(toolsRemoved / totalTools * 100).toFixed(1)}%"></div>
        <div class="trace-token-bar-kept" style="width:${(toolsKept / totalTools * 100).toFixed(1)}%"></div>
      </div>
      <div class="trace-token-label">
        <strong>${totalTools}</strong> tools &rarr; <strong>${toolsKept}</strong> tools (${pctCut}% cut)<br>
        <strong>${fmtK(tokensSaved)}</strong> tokens stripped, saving <strong>${fmtCost(savingsCost)}</strong>/turn
      </div>
    </div>` : '';

  let costLine = '';
  if (ht.enriched) {
    costLine = `<div class="trace-token-impact">
      <div class="trace-token-impact-title">Turn cost</div>
      <div class="trace-token-label">
        ${model ? `<strong>${esc(model)}</strong> &middot; ` : ''}
        <strong>${fmtK(inputTok)}</strong> input &middot;
        <strong>${fmtK(outputTok)}</strong> output &middot;
        <strong>${fmtK(cacheRead)}</strong> cache read
        ${costUsd > 0 ? ` &middot; <strong>${fmtCost(costUsd)}</strong>` : ''}
      </div>
    </div>`;
  } else {
    costLine = `<div style="font-size:11px;color:var(--t4);margin-top:8px">Turn cost data will appear after the next ingest cycle.</div>`;
  }

  const storyPanel = `<div class="trace-story">
    <div class="trace-story-eyebrow">Your interaction</div>
    <div class="trace-story-context">
      ${ht.working_directory ? `<code>${esc(ht.working_directory)}</code><br>` : ''}
      ${ts}
    </div>
    ${promptPreview}
    <div class="trace-ctx-band">
      <div class="trace-ctx-band-title">What ctx did</div>
      ${ctxBullets}
    </div>
    ${savingsBar}
    ${costLine}
  </div>`;

  const ga = GATE_META;
  const flowNodes = [
    { id: 'in', name: 'Request in', fired: true, desc: `${totalTools} tools from all servers`, anchor: true },
    { id: 'auto', name: 'Auto-Profile', fired: ht.auto_selected, desc: ht.auto_selected ? `Matched ${esc(ht.auto_trigger || 'cwd')} &rarr; ${esc(profileLabel)}` : 'No switch', accent: ga.auto?.accent },
    { id: 'allow', name: 'Profile Filter', fired: toolsRemoved > 0, desc: toolsRemoved > 0 ? `-${toolsRemoved} tools, kept ${toolsKept}` : 'No tools stripped', accent: ga.filter?.accent },
    { id: 'inject', name: 'Inject', fired: ht.inject_fired, desc: ht.inject_fired ? 'system_prefix.md prepended' : 'Not active', accent: ga.inject?.accent },
    { id: 'adaptive', name: 'Adaptive', fired: !!ht.adaptive_fired, desc: ht.adaptive_fired ? 'adaptive_prefix.md appended' : 'Not active', accent: ga.adaptive?.accent },
    { id: 'coach', name: 'Coaching', fired: !!ht.coach_kind, desc: ht.coach_kind ? esc(ht.coach_kind) : '--', accent: ga.coach?.accent },
    { id: 'budget', name: 'Budget Guard', fired: ht.budget_fired, desc: ht.budget_fired ? 'Cost alert fired' : '--', accent: ga.budget?.accent },
    { id: 'out', name: 'Sent to API', fired: true, desc: `${toolsKept} tools, ~${fmtK(tokensSaved)} tokens saved`, anchor: true },
  ];

  const flowHtml = flowNodes.map(n => {
    const cls = n.anchor ? 'trace-flow-node anchor fired' : (n.fired ? 'trace-flow-node fired' : 'trace-flow-node dimmed');
    const style = n.fired && n.accent ? `--node-accent:${n.accent}` : '';
    return `<div class="${cls}" style="${style}">
      <div class="trace-flow-node-name">${n.name}</div>
      <div class="trace-flow-node-desc">${n.desc}</div>
    </div>`;
  }).join('');

  const flowPanel = `<div class="trace-flow-panel">
    <div class="trace-flow-eyebrow">ctx pipeline</div>
    ${flowHtml}
  </div>`;

  const savingsSummary = toolsRemoved > 0
    ? `-<strong>${toolsRemoved}</strong> of ${totalTools} tools &middot; <strong>${fmtK(tokensSaved)}</strong> tok &middot; ${pctCut}% cut`
    : `${totalTools} tools (no filter)`;
  const costSummary = ht.enriched ? fmtCost(costUsd) : '';

  return `<div class="trace-row" id="trace-${i}" onclick="toggleTraceReq(${i})">
    <div class="trace-summary">
      <div class="trace-ts">${ts}</div>
      <div class="trace-profile-chip">${profileLabel}</div>
      ${modeChip}
      ${autoChip}
      ${abBadges}
      ${enrichedBadge}
      <div class="trace-stat">${savingsSummary}</div>
      <div class="trace-cost">${costSummary}</div>
      <div class="trace-chevron">▼</div>
    </div>
    <div class="trace-detail">
      <div class="trace-panels">
        ${storyPanel}
        ${flowPanel}
      </div>
    </div>
  </div>`;
}

function fmtTs(ts) {
  const d = new Date(ts);
  const now = new Date();
  const diffMs = now - d;
  const diffH = diffMs / 3600000;
  if (diffH < 1) return `${Math.round(diffMs/60000)}m ago`;
  if (diffH < 24) return `${Math.round(diffH)}h ago`;
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' }) +
    ' ' + d.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
}

function fmtK(n) { return n >= 1000 ? (n/1000).toFixed(1)+'K' : String(n); }

function traceFlowNode(id, name, fired, desc, accent) {
  const cls = fired ? 'trace-flow-node fired' : 'trace-flow-node dimmed';
  const style = fired && accent ? `--node-accent:${accent}` : '';
  return `<div class="${cls}" style="${style}">
    <div class="trace-flow-node-name">${esc(name)}</div>
    <div class="trace-flow-node-desc">${desc}</div>
  </div>`;
}

function serverDisplayName(s) {
  return s.replace(/^mcp__claude_ai_/, '').replace(/__$/, '').replace(/_/g, ' ');
}

function traceRow(req, i) {
  const ts = fmtTs(req.ts);
  const profileLabel = req.profile || 'all';
  const autoChip = req.auto_selected
    ? `<span class="trace-auto-chip">auto: ${req.auto_trigger || 'matched'}</span>`
    : '';

  const totalTools = req.tools_removed + (req.tools_sent_count || 0);
  const keptTools = req.tools_sent_count || 0;
  const pctCut = totalTools > 0 ? Math.round(req.tools_removed / totalTools * 100) : 0;

  // ── Left panel: Your interaction ──
  const ctxItems = [];
  ctxItems.push({
    line: `Applied <strong>${esc(profileLabel)}</strong> profile.`,
    detail: req.kept_servers.length
      ? `Kept: ${req.kept_servers.map(s => esc(serverDisplayName(s))).join(', ')}. Removed: ${req.removed_servers.map(s => esc(serverDisplayName(s))).join(', ') || 'none'}.`
      : `Stripped ${req.tools_removed} tools.`
  });
  if (req.inject_fired) ctxItems.push({ line: 'Prepended <strong>system_prefix.md</strong>.', detail: '' });
  if (req.coach_kind) ctxItems.push({ line: `Coaching: <strong>${esc(req.coach_kind)}</strong>.`, detail: '' });
  if (req.budget_fired) ctxItems.push({ line: 'Session cost alert fired.', detail: '' });
  if (req.behavior_kind) ctxItems.push({ line: `Behavior guard: <strong>${esc(req.behavior_kind)}</strong>.`, detail: '' });
  if (req.compress_chars_saved > 0) ctxItems.push({ line: `Compressed <strong>${fmtK(req.compress_chars_saved)}</strong> chars of bash output.`, detail: '' });
  const ctxBullets = renderCtxBulletList(ctxItems);

  let responseHtml = '';
  if (req.mcp_tools_invoked && req.mcp_tools_invoked.length) {
    const names = req.mcp_tools_invoked.map(n => serverDisplayName(n));
    const unique = [...new Set(names)];
    responseHtml = `Claude used <strong>${unique.length}</strong> MCP tool${unique.length !== 1 ? 's' : ''}: ${unique.map(n => esc(n)).join(', ')}`;
  } else {
    responseHtml = 'Claude responded (streaming; tool use not captured)';
  }

  const barTotal = Math.max(totalTools, 1);
  const removedPct = (req.tools_removed / barTotal * 100).toFixed(1);
  const keptPct = (keptTools / barTotal * 100).toFixed(1);

  const storyPanel = `<div class="trace-story">
    <div class="trace-story-eyebrow">Your interaction</div>
    <div class="trace-story-context">
      ${req.working_directory ? `<code>${esc(req.working_directory)}</code><br>` : ''}
      ${ts}
    </div>
    <div class="trace-ctx-band">
      <div class="trace-ctx-band-title">What ctx did</div>
      ${ctxBullets}
    </div>
    <div class="trace-response">${responseHtml}</div>
    <div class="trace-token-impact">
      <div class="trace-token-impact-title">Token impact</div>
      <div class="trace-token-bar">
        <div class="trace-token-bar-removed" style="width:${removedPct}%"></div>
        <div class="trace-token-bar-kept" style="width:${keptPct}%"></div>
      </div>
      <div class="trace-token-label">
        <strong>${totalTools}</strong> tools &rarr; <strong>${keptTools}</strong> tools (${pctCut}% cut)<br>
        <strong>${fmtK(req.tokens_saved)}</strong> tokens stripped, saving <strong>${fmtCost(req.cost_saved)}</strong>
      </div>
    </div>
  </div>`;

  // ── Right panel: ctx pipeline ──
  const ga = GATE_META;
  const flowNodes = [
    { id: 'in', name: 'Request in', fired: true, desc: `${totalTools} tools from ${req.removed_servers.length + req.kept_servers.length} servers`, accent: null, anchor: true },
    { id: 'auto', name: 'Auto-Profile', fired: req.auto_selected, desc: req.auto_selected ? `Matched ${esc(req.auto_trigger || 'cwd')} &rarr; ${esc(profileLabel)}` : 'No switch', accent: ga.auto?.accent },
    { id: 'filter', name: 'Profile Filter', fired: req.tools_removed > 0, desc: req.tools_removed > 0 ? `-${req.tools_removed} tools from ${req.removed_servers.length} servers, kept ${keptTools}` : 'No tools stripped', accent: ga.filter?.accent },
    { id: 'inject', name: 'Inject', fired: req.inject_fired, desc: req.inject_fired ? 'system_prefix.md prepended' : 'Not active', accent: ga.inject?.accent },
    { id: 'coach', name: 'Coaching', fired: !!req.coach_kind, desc: req.coach_kind ? esc(req.coach_kind) : '--', accent: ga.coach?.accent },
    { id: 'behavior', name: 'Behavior Guard', fired: !!req.behavior_kind, desc: req.behavior_kind ? esc(req.behavior_kind) : '--', accent: ga.behavior?.accent },
    { id: 'budget', name: 'Budget Guard', fired: req.budget_fired, desc: req.budget_fired ? 'Cost alert fired' : '--', accent: ga.budget?.accent },
    { id: 'compress', name: 'Bash Compress', fired: req.compress_chars_saved > 0, desc: req.compress_chars_saved > 0 ? `${fmtK(req.compress_chars_saved)} chars compressed` : '--', accent: ga.compress?.accent },
    { id: 'out', name: 'Sent to API', fired: true, desc: `${keptTools} tools, ~${fmtK(req.tokens_saved)} tokens saved`, accent: null, anchor: true },
  ];

  const flowHtml = flowNodes.map(n => {
    const cls = n.anchor ? 'trace-flow-node anchor fired' : (n.fired ? 'trace-flow-node fired' : 'trace-flow-node dimmed');
    const style = n.fired && n.accent ? `--node-accent:${n.accent}` : '';
    return `<div class="${cls}" style="${style}">
      <div class="trace-flow-node-name">${n.name}</div>
      <div class="trace-flow-node-desc">${n.desc}</div>
    </div>`;
  }).join('');

  const flowPanel = `<div class="trace-flow-panel">
    <div class="trace-flow-eyebrow">ctx pipeline</div>
    ${flowHtml}
  </div>`;

  return `<div class="trace-row" id="trace-${i}" onclick="toggleTraceReq(${i})">
    <div class="trace-summary">
      <div class="trace-ts">${ts}</div>
      <div class="trace-profile-chip">${profileLabel}</div>
      ${autoChip}
      <div class="trace-stat">-<strong>${req.tools_removed}</strong> of ${totalTools} tools &middot; <strong>${fmtK(req.tokens_saved)}</strong> tok &middot; ${pctCut}% cut</div>
      <div class="trace-cost">${fmtCost(req.cost_saved)}</div>
      <div class="trace-chevron">▼</div>
    </div>
    <div class="trace-detail">
      <div class="trace-panels">
        ${storyPanel}
        ${flowPanel}
      </div>
    </div>
  </div>`;
}

function toggleTraceReq(i) {
  document.getElementById('trace-'+i).classList.toggle('expanded');
}

