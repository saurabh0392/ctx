// ─── Profile analytics ────────────────────────────────────
async function loadProfileAnalytics() {
  const [stats, profiles] = await Promise.all([
    fetch(appendSince('/api/profiles/analytics')).then(r => r.json()),
    fetch('/api/profiles').then(r => r.json()),
  ]);

  const el = document.getElementById('profile-analytics-body');
  if (!stats.length) {
    el.innerHTML = `<div class="empty">No requests recorded yet. Start the proxy and use Claude Code to see your profile breakdown here.</div>`;
    return;
  }

  const totalReqs = stats.reduce((s, p) => s + p.requests, 0);
  const totalTokens = stats.reduce((s, p) => s + p.tokens_saved, 0);
  const totalCost = stats.reduce((s, p) => s + p.cost_saved, 0);
  const autoTotal = stats.reduce((s, p) => s + p.auto_selected_count, 0);

  const top = stats[0];
  const topPct = Math.round(top.pct_of_total);

  // Narrative prose
  let narrative = `<p>ctx has filtered <strong>${fmtK(totalTokens)} tokens</strong> of tool schema across <strong>${totalReqs} requests</strong>. That's <strong>$${totalCost.toFixed(2)}</strong> back in your pocket.</p>`;

  if (top) {
    narrative += `<p>You run on <strong>${top.display || top.slug}</strong> ${topPct}% of the time.`;
    if (stats.length > 1) {
      const others = stats.slice(1).map(s => `<strong>${s.display || s.slug}</strong> for ${s.requests} request${s.requests!==1?'s':''}`).join(', ');
      narrative += ` When your work shifted, ctx noticed and switched: ${others}.`;
    }
    narrative += `</p>`;
  }

  if (autoTotal > 0) {
    narrative += `<p>Auto-select picked your profile <strong>${autoTotal} time${autoTotal!==1?'s':''}</strong>. ctx read the system prompt, spotted a keyword, and switched without you having to do anything.</p>`;
  } else {
    narrative += `<p>Auto-select hasn't fired yet. Once ctx spots keywords like "carrier," "data," or "design" in the system prompt, it will switch profiles on its own. You'll see it here when it does.</p>`;
  }

  // Bar chart rows
  const maxReqs = Math.max(...stats.map(s => s.requests), 1);
  const bars = stats.map(s => {
    const pct = Math.round((s.requests / maxReqs) * 100);
    return `<div class="pa-bar-row">
      <div class="pa-bar-label">${s.display || s.slug}</div>
      <div class="pa-bar-track"><div class="pa-bar-fill" style="width:${pct}%"></div></div>
      <div class="pa-bar-meta">${s.requests} req &middot; $${s.cost_saved.toFixed(2)} saved${s.auto_selected_count > 0 ? ' &middot; <span style="color:var(--blue)">'+s.auto_selected_count+' auto</span>' : ''}</div>
    </div>`;
  }).join('');

  el.innerHTML = `
    <div class="pa-narrative">${narrative}</div>
    <div class="card" style="padding:20px 24px">
      <div class="section-head" style="margin-bottom:12px">Requests by profile</div>
      ${bars}
    </div>`;
}

