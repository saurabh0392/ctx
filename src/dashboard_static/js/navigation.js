// ─── Tab navigation ───────────────────────────────────────
function showTab(id, el) {
  document.querySelectorAll('.tab-panel').forEach(p => p.classList.remove('active'));
  document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'));
  document.getElementById('tab-' + id).classList.add('active');
  el.classList.add('active');
  if (id === 'promptstats' && !window._psLoaded) { window._psLoaded = true; loadPromptStats(); startPsPolling(); }
  if (id === 'profiles') loadProfiles();
  if (id === 'trace' && !window._traceLoaded) { window._traceLoaded = true; loadTrace(); }
  if (id === 'pipeline') loadPipeline();
  if (id === 'settings') loadSettingsTab();
  if (id === 'experiment') loadExperimentTab();
  if (id === 'simulate') loadSimulateTab();
}

let _psInterval = null;
function startPsPolling() {
  if (_psInterval) return;
  _psInterval = setInterval(refreshPromptStats, 30_000);
}
async function refreshPromptStats() {
  const btn = document.getElementById('ps-refresh-btn');
  if (btn) { btn.textContent = '↻ Refreshing…'; btn.disabled = true; }
  await loadPromptStats();
  if (btn) { btn.textContent = '↻ Refresh'; btn.disabled = false; }
}

function renderProjectsPanel(rows, sessionsFallback) {
  const el = document.getElementById('projects-body');
  if (!el) return;
  if (!rows.length) {
    el.innerHTML = '<div class="empty" style="padding:12px 0">No per-folder rows yet. Send Claude Code traffic through ctx so working directories populate.</div>';
    return;
  }
  const spendHdr = sessionsFallback ? 'Spend (indexed)' : '$ @ cache read';
  const tokHdr = sessionsFallback ? 'Cache read toks' : 'Tokens saved';
  el.innerHTML = '<table class="proj-table"><thead><tr><th>Folder</th><th>Req</th><th>' + tokHdr + '</th><th>' + spendHdr + '</th></tr></thead><tbody>' +
    rows.map(r => `<tr><td class="path">${esc(r.working_directory)}</td><td>${r.requests}</td><td>${(r.tokens_saved || 0).toLocaleString()}</td><td>${fmtCost(r.cost_saved)}</td></tr>`).join('') +
    '</tbody></table>';
}

function renderToolUsagePanel(rows) {
  const el = document.getElementById('tool-usage-body');
  if (!el) return;
  if (!rows.length) {
    el.innerHTML = '<div class="empty" style="padding:12px 0">No MCP tool traffic rows yet. Non-streaming responses record tool_use counts when available.</div>';
    return;
  }
  el.innerHTML = '<table class="proj-table"><thead><tr><th>Server</th><th>Tools sent</th><th>Tools used</th><th>Use ratio</th></tr></thead><tbody>' +
    rows.map(r => {
      let ratio = '—';
      if (r.tools_sent > 0) ratio = ((r.tools_invoked / r.tools_sent) * 100).toFixed(0) + '%';
      else if (r.tools_invoked > 0) ratio = '100%';
      return `<tr><td>${esc(r.server)}</td><td>${r.tools_sent}</td><td>${r.tools_invoked}</td><td>${ratio}</td></tr>`;
    }).join('') + '</tbody></table>';
}

