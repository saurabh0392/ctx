// ─── Utilities ────────────────────────────────────────────
function esc(s) { return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;'); }
function fmtCost(n) {
  if (n >= 100) return '$' + n.toFixed(0);
  if (n >= 10)  return '$' + n.toFixed(2);
  if (n >= 1)   return '$' + n.toFixed(2);
  if (n >= 0.01) return '$' + n.toFixed(3);
  if (n > 0)     return '$' + n.toFixed(4);
  return '$0.00';
}
function fmtDate(iso) {
  const d = new Date(iso);
  return d.toLocaleDateString('en-US',{month:'short',day:'numeric'}) + ' · ' +
         d.toLocaleTimeString('en-US',{hour:'2-digit',minute:'2-digit'});
}

function animateNum(el, end, prefix='', suffix='', decimals=0, duration=1100) {
  const start = parseFloat(el.textContent.replace(/[^0-9.]/g, '')) || 0;
  if (Math.abs(end - start) < 0.005) { el.textContent = prefix + end.toFixed(decimals) + suffix; return; }
  const t0 = performance.now();
  function tick(now) {
    const p = Math.min((now - t0) / duration, 1);
    const v = start + (end - start) * (1 - Math.pow(1 - p, 3));
    el.textContent = prefix + v.toFixed(decimals) + suffix;
    if (p < 1) requestAnimationFrame(tick);
  }
  requestAnimationFrame(tick);
}

function scoreColor(s) {
  if (s >= 80) return '#22c55e';
  if (s >= 60) return '#84cc16';
  if (s >= 40) return '#f59e0b';
  if (s >= 20) return '#f97316';
  return '#ef4444';
}
function scoreLabel(s) {
  if (s >= 80) return 'Expert';
  if (s >= 60) return 'Efficient';
  if (s >= 40) return 'Developing';
  if (s >= 20) return 'Getting started';
  return 'Needs focus';
}
function scoreDesc(s) {
  if (s >= 80) return 'Your prompt patterns are highly efficient. Context resets are rare and sessions stay focused.';
  if (s >= 60) return 'Good efficiency overall. A few patterns are costing you. See the breakdown below.';
  if (s >= 40) return 'Room to improve. Context resets and long sessions are the main cost drivers right now.';
  if (s >= 20) return 'Several patterns are significantly inflating your costs. The insights below show where.';
  return 'High-cost patterns detected across most sessions. Addressing these could halve your spend.';
}

const PROFILE_LABELS = { carrier:'Carrier work', data:'Data analysis', design:'Design', minimal:'Minimal', all:'All tools', '':'All tools' };

(function initCtxSinceFromUrl() {
  const v = new URLSearchParams(window.location.search).get('since');
  if (v === 'all') localStorage.setItem('ctx-dashboard-since', 'all');
})();

function appendSince(url) {
  if (localStorage.getItem('ctx-dashboard-since') !== 'all') return url;
  return url + (url.indexOf('?') >= 0 ? '&' : '?') + 'since=all';
}

function syncCtxRangeRadios() {
  const all = localStorage.getItem('ctx-dashboard-since') === 'all';
  const ra = document.getElementById('ctx-range-all');
  const rw = document.getElementById('ctx-range-wm');
  if (ra && rw) { ra.checked = all; rw.checked = !all; }
}

function onCtxRangeChange() {
  const all = document.getElementById('ctx-range-all').checked;
  if (all) localStorage.setItem('ctx-dashboard-since', 'all');
  else localStorage.removeItem('ctx-dashboard-since');
  const url = new URL(window.location.href);
  if (all) url.searchParams.set('since', 'all');
  else url.searchParams.delete('since');
  const q = url.searchParams.toString();
  history.replaceState({}, '', url.pathname + (q ? '?' + q : '') + url.hash);
  applyDashboardSinceReload();
}

function applyDashboardSinceReload() {
  syncCtxRangeRadios();
  loadSavings().catch(console.error);
  if (window._psLoaded) loadPromptStats().catch(console.error);
  if (window._traceLoaded) loadTrace().catch(console.error);
  const exp = document.getElementById('tab-experiment');
  if (exp && exp.classList.contains('active')) loadExperimentTab().catch(console.error);
  loadPipeline().catch(console.error);
  const st = document.getElementById('tab-settings');
  if (st && st.classList.contains('active')) loadSettingsTab();
}

function updateCtxRangeMetaFromStats(stats) {
  const el = document.getElementById('ctx-range-meta');
  if (!el || !stats) return;
  const all = localStorage.getItem('ctx-dashboard-since') === 'all';
  if (all) {
    el.textContent = 'Showing every indexed row, including sessions from before ctx was active when SQLite already had them.';
    return;
  }
  const ts = stats.ctx_active_since;
  if (stats.dashboard_watermark_filtering && ts) {
    el.textContent = 'Counts include sessions and requests on or after ' + ts + '. Pick All time to include earlier indexed rows.';
  } else if (ts) {
    el.textContent = 'Watermark recorded at ' + ts + '. It reapplies after reset when the next hook or filtered request runs.';
  } else {
    el.textContent = 'No install stamp yet. After the first hook or filtered request, charts default to data from after ctx was active.';
  }
}

document.getElementById('ctx-range-bar')?.addEventListener('change', (e) => {
  if (e.target && e.target.name === 'ctx-data-range') onCtxRangeChange();
});
syncCtxRangeRadios();

