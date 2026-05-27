'use strict';

/**
 * In-process filter for Claude Code: MCP tool stripping, auto-profile, gates, analytics.
 * Reads ~/.ctx/filter-config.json (see Rust filter_hook).
 */
if (global.__CTX_FILTER_PATCHED__) {
  module.exports = {};
} else {
  global.__CTX_FILTER_PATCHED__ = true;

  const fs = require('fs');
  const path = require('path');
  const crypto = require('crypto');
  const http = require('http');
  const https = require('https');

  // Priority order from most-specific to broadest. Slugs not in this list
  // (custom user profiles) are appended dynamically at the end.
  const CATEGORY_PRIORITY = [
    'carrier', 'minimal', 'shippo', 'infra', 'finance', 'files',
    'work', 'design', 'data', 'comms', 'personal', 'other'
  ];

  const CORRECTION_PHRASES = [
    'no,', 'no that', 'no -', 'nope', "that's wrong", 'thats wrong',
    'not that', 'actually,', 'wait,', 'stop,', 'that is wrong',
    'incorrect', 'you misunderstood', 'wrong,', 'thats not right',
    "that's not right", 'not what i', 'not what I', "that's not what",
    'thats not what', "you're wrong", 'youre wrong',
  ];

  const STOP_WORDS = new Set([
    'the', 'a', 'an', 'and', 'or', 'but', 'is', 'are', 'was', 'were',
    'be', 'been', 'being', 'have', 'has', 'had', 'do', 'does', 'did',
    'will', 'would', 'could', 'should', 'may', 'might', 'shall', 'can',
    'to', 'of', 'in', 'for', 'on', 'with', 'at', 'by', 'from', 'as',
    'into', 'through', 'during', 'before', 'after', 'above', 'below',
    'i', 'you', 'it', 'this', 'that', 'they', 'we', 'he', 'she',
    'my', 'your', 'its', 'their', 'our', 'me', 'him', 'her', 'us',
    'what', 'how', 'why', 'when', 'where', 'which', 'who',
    'just', 'also', 'now', 'up', 'out', 'so', 'if', 'then',
    'please', 'ok', 'okay', 'yes', 'no', 'not',
  ]);

  function ctxDir() {
    return process.env.CTX_HOME || path.join(process.env.HOME || '', '.ctx');
  }

  function filterHost() {
    return process.env.CTX_FILTER_HOST || 'api.anthropic.com';
  }

  function filterPortEnv() {
    return process.env.CTX_FILTER_PORT;
  }

  function loadCfg() {
    const def = {
      profile: 'all',
      keep: [],
      auto_profile_enabled: true,
      inject_enabled: true,
      session_budget_threshold_usd: 25,
      dashboard_port: 8789,
      profiles: {},
    };
    try {
      const p = path.join(ctxDir(), 'filter-config.json');
      const j = JSON.parse(fs.readFileSync(p, 'utf8'));
      return {
        profile: j.profile || def.profile,
        keep: Array.isArray(j.keep) ? j.keep : def.keep,
        auto_profile_enabled: j.auto_profile_enabled !== false,
        inject_enabled: j.inject_enabled !== false,
        session_budget_threshold_usd: typeof j.session_budget_threshold_usd === 'number'
          ? j.session_budget_threshold_usd
          : def.session_budget_threshold_usd,
        dashboard_port: typeof j.dashboard_port === 'number' ? j.dashboard_port : def.dashboard_port,
        profiles: j.profiles && typeof j.profiles === 'object' ? j.profiles : {},
      };
    } catch (_e) {
      return def;
    }
  }

  function extractSystemText(o) {
    if (!o || !o.system) return '';
    const s = o.system;
    if (typeof s === 'string') return s;
    if (Array.isArray(s)) {
      return s
        .filter(function (b) { return b && b.type === 'text' && b.text; })
        .map(function (b) { return String(b.text); })
        .join('\n');
    }
    return '';
  }

  function extractWorkingDirectory(system) {
    const lines = String(system).split(/\r?\n/);
    for (let i = 0; i < lines.length; i++) {
      const lower = lines[i].trim().toLowerCase();
      let rest = null;
      if (lower.startsWith('primary working directory:')) {
        rest = lower.slice('primary working directory:'.length).trim();
      } else if (lower.startsWith('working directory:')) {
        rest = lower.slice('working directory:'.length).trim();
      } else if (lower.startsWith('cwd:')) {
        rest = lower.slice('cwd:'.length).trim();
      }
      if (rest) return rest;
    }
    return '';
  }

  function profileFromSlug(cfg, slug) {
    const raw = cfg.profiles[slug];
    if (!raw || typeof raw !== 'object') return { keep: [], path_patterns: [], triggers: [] };
    return {
      keep: Array.isArray(raw.keep) ? raw.keep : [],
      path_patterns: Array.isArray(raw.path_patterns) ? raw.path_patterns : [],
      triggers: Array.isArray(raw.triggers) ? raw.triggers : [],
    };
  }

  function matchesPath(profile, cwd) {
    const low = cwd.toLowerCase();
    for (let i = 0; i < profile.path_patterns.length; i++) {
      if (low.indexOf(String(profile.path_patterns[i]).toLowerCase()) >= 0) return true;
    }
    return false;
  }

  function matchesSystemPrompt(profile, system) {
    const low = system.toLowerCase();
    for (let i = 0; i < profile.triggers.length; i++) {
      if (low.indexOf(String(profile.triggers[i]).toLowerCase()) >= 0) return profile.triggers[i];
    }
    return null;
  }

  function autoSelect(cfg, systemText, activeSlug) {
    if (!cfg.auto_profile_enabled) return { slug: activeSlug, auto: false, trigger: null };
    const _allSlugs = Object.keys(cfg.profiles || {});
    const PRIORITY = [
      ...CATEGORY_PRIORITY.filter(s => _allSlugs.includes(s)),
      ..._allSlugs.filter(s => !CATEGORY_PRIORITY.includes(s)),
    ];
    const cwd = extractWorkingDirectory(systemText);
    if (cwd) {
      for (let i = 0; i < PRIORITY.length; i++) {
        const slug = PRIORITY[i];
        const p = profileFromSlug(cfg, slug);
        if (matchesPath(p, cwd)) {
          const parts = cwd.split('/').filter(Boolean);
          const label = parts.length ? parts[parts.length - 1] : cwd;
          if (slug !== activeSlug) return { slug: slug, auto: true, trigger: label };
          return { slug: activeSlug, auto: false, trigger: null };
        }
      }
    }
    const low = systemText.toLowerCase();
    for (let i = 0; i < PRIORITY.length; i++) {
      const slug = PRIORITY[i];
      const p = profileFromSlug(cfg, slug);
      const trig = matchesSystemPrompt(p, low);
      if (trig) {
        if (slug !== activeSlug) return { slug: slug, auto: true, trigger: String(trig) };
        return { slug: activeSlug, auto: false, trigger: null };
      }
    }
    // Tier 3: embedding-based suggestion from similar past sessions.
    // Written by /api/profile-suggest after the previous request — one request stale
    // by design so it never blocks the hot path.
    const suggestion = loadProfileSuggestion();
    if (
      suggestion &&
      typeof suggestion.profile === 'string' &&
      suggestion.profile &&
      suggestion.based_on >= 2 &&
      suggestion.confidence >= 0.4 &&
      cfg.profiles && cfg.profiles[suggestion.profile]
    ) {
      const slug = suggestion.profile;
      if (slug !== activeSlug) {
        return { slug: slug, auto: true, trigger: 'similar sessions' };
      }
    }

    return { slug: activeSlug, auto: false, trigger: null };
  }


  function loadProfileSuggestion() {
    try {
      const p = path.join(ctxDir(), 'profile-suggestion.json');
      return JSON.parse(fs.readFileSync(p, 'utf8'));
    } catch (_e) { return null; }
  }

  function postProfileSuggest(port, dir, text) {
    if (!dir && !text) return;
    const body = JSON.stringify({ dir: dir || '', text: (text || '').slice(0, 200) });
    const opts = {
      hostname: '127.0.0.1',
      port: port,
      path: '/api/profile-suggest',
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Content-Length': Buffer.byteLength(body, 'utf8'),
      },
    };
    const req = http.request(opts);
    req.on('error', function () { /* dashboard not running — ignore */ });
    req.write(body);
    req.end();
  }

  function filtersTool(toolName, keep) {
    if (!keep.length) return false;
    if (toolName.indexOf('mcp__') !== 0) return false;
    for (let i = 0; i < keep.length; i++) {
      if (toolName.indexOf(keep[i]) === 0) return false;
    }
    return true;
  }

  function serverDisplayFromTool(name) {
    const n = String(name);
    if (n.indexOf('mcp__claude_ai_') !== 0) return null;
    const rest = n.slice('mcp__claude_ai_'.length);
    const idx = rest.indexOf('__');
    if (idx < 0) return null;
    return rest.slice(0, idx).replace(/_/g, ' ');
  }

  function filterTools(o, keep) {
    const removedServers = {};
    const keptServers = {};
    const sentByServer = {};
    if (!o || !Array.isArray(o.tools) || !keep.length) {
      return {
        tools_removed: 0,
        tokens_saved: 0,
        removed_servers: [],
        kept_servers: [],
        tools_sent_count: 0,
        tools_sent_by_server: sentByServer,
      };
    }
    const before = o.tools.length;
    const beforeBytes = Buffer.byteLength(JSON.stringify(o.tools), 'utf8');
    o.tools = o.tools.filter(function (t) {
      const n = t && t.name ? String(t.name) : '';
      const drop = filtersTool(n, keep);
      const sname = serverDisplayFromTool(n);
      if (sname) {
        if (drop) removedServers[sname] = true;
        else {
          keptServers[sname] = true;
          sentByServer[sname] = (sentByServer[sname] || 0) + 1;
        }
      }
      return !drop;
    });
    const afterBytes = Buffer.byteLength(JSON.stringify(o.tools), 'utf8');
    const tools_removed = before - o.tools.length;
    const tokens_saved = tools_removed > 0 ? Math.floor((beforeBytes - afterBytes) / 4) : 0;
    const rs = Object.keys(removedServers).sort();
    const ks = Object.keys(keptServers).sort();
    return {
      tools_removed: tools_removed,
      tokens_saved: tokens_saved,
      removed_servers: rs,
      kept_servers: ks,
      tools_sent_count: o.tools.length,
      tools_sent_by_server: sentByServer,
    };
  }

  function extractUserTexts(messages) {
    const out = [];
    if (!Array.isArray(messages)) return out;
    for (let i = 0; i < messages.length; i++) {
      const m = messages[i];
      if (!m || m.role !== 'user') continue;
      const c = m.content;
      if (typeof c === 'string') out.push(c);
      else if (Array.isArray(c)) {
        const parts = [];
        for (let j = 0; j < c.length; j++) {
          const b = c[j];
          if (b && b.type === 'text' && b.text) parts.push(String(b.text));
        }
        out.push(parts.join(' '));
      }
    }
    return out;
  }

  function hasCorrectionPhrase(text) {
    const low = text.toLowerCase();
    for (let i = 0; i < CORRECTION_PHRASES.length; i++) {
      if (low.indexOf(CORRECTION_PHRASES[i]) >= 0) return true;
    }
    return false;
  }

  function keywords(text) {
    const set = new Set();
    const low = text.toLowerCase().split(/[^a-z0-9]+/);
    for (let i = 0; i < low.length; i++) {
      const w = low[i];
      if (w.length > 2 && !STOP_WORDS.has(w)) set.add(w);
    }
    return set;
  }

  function jaccard(a, b) {
    if (!a.size || !b.size) return 0;
    let inter = 0;
    a.forEach(function (w) {
      if (b.has(w)) inter += 1;
    });
    const union = a.size + b.size - inter;
    return union ? inter / union : 0;
  }

  function coachSuggestion(userTexts) {
    const window = userTexts.slice(-6).reverse();
    let corr = 0;
    for (let i = 0; i < window.length; i++) {
      if (hasCorrectionPhrase(window[i])) corr += 1;
    }
    if (corr >= 2) {
      return {
        kind: 'correction-cascade',
        text:
          'Note: ' +
          corr +
          ' correction turns detected in this session. When the next user message arrives, respond by first stating the specific constraint you will now honor, then proceed. Do not re-attempt without acknowledging what was wrong.',
      };
    }
    if (userTexts.length >= 3) {
      const current = userTexts[userTexts.length - 1];
      const curWords = keywords(current);
      if (curWords.size >= 4) {
        const lookback = userTexts.slice(-4, -1).reverse();
        for (let i = 0; i < lookback.length; i++) {
          if (i === 0) continue;
          const prior = lookback[i];
          const pw = keywords(prior);
          if (pw.size >= 4) {
            const sim = jaccard(curWords, pw);
            if (sim >= 0.4) {
              return {
                kind: 're-ask',
                text:
                  'Note: the user appears to be rephrasing a question from earlier (keyword overlap ' +
                  Math.round(sim * 100) +
                  '%). Their first attempt may not have been answered fully. Address the original intent directly before elaborating.',
              };
            }
          }
        }
      }
    }
    return null;
  }

  function countUserTurns(messages) {
    if (!Array.isArray(messages)) return 0;
    let n = 0;
    for (let i = 0; i < messages.length; i++) {
      if (messages[i] && messages[i].role === 'user') n += 1;
    }
    return n;
  }

  function loadBehaviorHintFile() {
    try {
      const p = path.join(ctxDir(), 'behavior-hints.json');
      const j = JSON.parse(fs.readFileSync(p, 'utf8'));
      if (j && j.hint && typeof j.hint === 'string') return j.hint;
    } catch (_e) {}
    return null;
  }

  function behaviorDedupKey(messages) {
    if (!Array.isArray(messages) || !messages.length) return '';
    const m = messages[0];
    let t = '';
    if (m.content && typeof m.content === 'string') t = m.content;
    else if (Array.isArray(m.content)) {
      const parts = [];
      for (let i = 0; i < m.content.length; i++) {
        const b = m.content[i];
        if (b && b.type === 'text' && b.text) parts.push(b.text);
      }
      t = parts.join(' ');
    }
    return crypto.createHash('sha256').update(t.slice(0, 256)).digest('hex');
  }

  function loadBehaviorWarnedSet() {
    const p = path.join(ctxDir(), 'behavior-warned.json');
    try {
      const j = JSON.parse(fs.readFileSync(p, 'utf8'));
      if (j && Array.isArray(j.keys)) return new Set(j.keys);
    } catch (_e) {}
    return new Set();
  }

  function saveBehaviorWarnedSet(set) {
    try {
      const p = path.join(ctxDir(), 'behavior-warned.json');
      const tmp = p + '.tmp';
      fs.writeFileSync(tmp, JSON.stringify({ keys: Array.from(set).slice(-200) }));
      fs.renameSync(tmp, p);
    } catch (_e) {}
  }

  /** Per-model input rate in USD/MTok — mirrors budget_guard.rs::rate_for_model. */
  function rateForModel(model) {
    const m = String(model || '').toLowerCase();
    if (m.startsWith('claude-opus')) return 15.0;
    if (m.startsWith('claude-haiku')) return 0.80;
    return 3.0; // sonnet and unknown: sonnet input rate
  }

  /** Estimate session cost from the full request object (needs o.model + o.messages). */
  function estimateSessionUsd(o) {
    const messages = o && Array.isArray(o.messages) ? o.messages : [];
    let chars = 0;
    for (let i = 0; i < messages.length; i++) {
      const m = messages[i];
      if (!m || !m.content) continue;
      const c = m.content;
      if (typeof c === 'string') chars += c.length;
      else if (Array.isArray(c)) {
        for (let j = 0; j < c.length; j++) {
          const b = c[j];
          if (b && b.text) chars += String(b.text).length;
        }
      }
    }
    return (chars / 4 / 1000000) * rateForModel(o && o.model);
  }

  function budgetDedupKey(messages) {
    let t = '';
    if (!Array.isArray(messages)) return '';
    for (let i = 0; i < messages.length; i++) {
      const m = messages[i];
      if (!m || m.role !== 'user') continue;
      if (typeof m.content === 'string') {
        t = m.content.slice(0, 500);
        break;
      }
      if (Array.isArray(m.content)) {
        const parts = [];
        for (let j = 0; j < m.content.length; j++) {
          const b = m.content[j];
          if (b && b.text) parts.push(b.text);
        }
        t = parts.join('').slice(0, 500);
        break;
      }
    }
    return crypto.createHash('sha256').update(t).digest('hex');
  }

  function loadBudgetWarnedSet() {
    const p = path.join(ctxDir(), 'budget-warned.json');
    try {
      const j = JSON.parse(fs.readFileSync(p, 'utf8'));
      if (j && Array.isArray(j.keys)) return new Set(j.keys);
    } catch (_e) {}
    return new Set();
  }

  function saveBudgetWarnedSet(set) {
    try {
      const p = path.join(ctxDir(), 'budget-warned.json');
      const tmp = p + '.tmp';
      fs.writeFileSync(tmp, JSON.stringify({ keys: Array.from(set).slice(-200) }));
      fs.renameSync(tmp, p);
    } catch (_e) {}
  }

  function prependToSystem(o, prefix) {
    if (!prefix) return;
    const s = o.system;
    if (typeof s === 'string') {
      o.system = prefix + '\n\n' + s;
    } else if (Array.isArray(s)) {
      o.system = [{ type: 'text', text: prefix }].concat(s);
    } else {
      o.system = prefix;
    }
  }

  function applyGates(cfg, o) {
    let inject_fired = false;
    let coach_kind = null;
    let budget_fired = false;
    let behavior_kind = null;

    if (cfg.inject_enabled) {
      try {
        const fp = path.join(ctxDir(), 'system_prefix.md');
        const prefix = fs.readFileSync(fp, 'utf8').trim();
        if (prefix) {
          prependToSystem(o, prefix);
          inject_fired = true;
        }
      } catch (_e) {}
    }

    const coach = coachSuggestion(extractUserTexts(o.messages || []));
    if (coach) {
      coach_kind = coach.kind;
      prependToSystem(o, coach.text);
    }

    const users = countUserTurns(o.messages || []);
    if (users <= 3) {
      const hint = loadBehaviorHintFile();
      if (hint) {
        const k = behaviorDedupKey(o.messages || []);
        if (k) {
          const warned = loadBehaviorWarnedSet();
          if (!warned.has(k)) {
            warned.add(k);
            saveBehaviorWarnedSet(warned);
            prependToSystem(o, hint);
            behavior_kind = 'historical-pattern';
          }
        }
      }
    }

    const est = estimateSessionUsd(o);
    const th = cfg.session_budget_threshold_usd || 25;
    if (est >= th) {
      const bk = budgetDedupKey(o.messages || []);
      if (bk) {
        const w = loadBudgetWarnedSet();
        if (!w.has(bk)) {
          w.add(bk);
          saveBudgetWarnedSet(w);
          budget_fired = true;
          prependToSystem(
            o,
            '[ctx budget alert] This session has consumed an estimated $' +
              Math.round(est) +
              ' in API costs (before caching discounts -- actual is typically 40-60% lower). Session alert threshold is ~$' +
              Math.round(th) +
              ' (from ~/.ctx/config.toml monthly budget). Use the AskUserQuestion tool BEFORE responding to the user\'s last message. Ask: "This session has used ~$' +
              Math.round(est) +
              ' in estimated API costs. Continue?" with options ["Continue", "Wrap up and start a new session"]. Wait for their choice before proceeding.'
          );
        }
      }
    }

    return { inject_fired: inject_fired, coach_kind: coach_kind, budget_fired: budget_fired, behavior_kind: behavior_kind };
  }

  function appendAnalyticsLine(rec) {
    const pe = process.env.CTX_DASHBOARD_PORT;
    const cfg = loadCfg();
    const port = pe != null && pe !== ''
      ? Number(pe)
      : typeof cfg.dashboard_port === 'number' ? cfg.dashboard_port : 8789;
    postIngestRequest(rec);
    postTriggerIngest(port);
  }

  /**
   * Ask the dashboard to run an incremental ingest so the current turn is immediately
   * visible in all dashboard views. Fire-and-forget — errors are silently ignored.
   * The dashboard debounces concurrent calls with an AtomicBool, so rapid turns are safe.
   */
  function postTriggerIngest(port) {
    const opts = {
      hostname: '127.0.0.1',
      port: port,
      path: '/api/trigger-ingest',
      method: 'POST',
      headers: { 'Content-Length': 0 },
    };
    const req = http.request(opts);
    req.on('error', function () { /* dashboard not running — ignore */ });
    req.end();
  }

  /** Dual-write the same record into ctx.db via the local dashboard (SQLite lives in Rust). */
  function postIngestRequest(rec) {
    const pe = process.env.CTX_DASHBOARD_PORT;
    const cfg = loadCfg();
    const port =
      pe != null && pe !== ''
        ? Number(pe)
        : typeof cfg.dashboard_port === 'number'
          ? cfg.dashboard_port
          : 8789;
    const body = JSON.stringify(rec);
    const opts = {
      hostname: '127.0.0.1',
      port: port,
      path: '/api/ingest-request',
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Content-Length': Buffer.byteLength(body, 'utf8'),
      },
    };
    const req = http.request(opts);
    req.on('error', function () {
      /* dashboard not running — record is dropped; trigger-ingest covers session data */
    });
    req.write(body);
    req.end();
  }

  function writeLastFilter(summary) {
    try {
      const p = path.join(ctxDir(), 'last-filter.json');
      fs.writeFileSync(p, JSON.stringify(summary, null, 2));
    } catch (_e) {}
  }

  function extractToolUsesFromAssistant(bodyBuf) {
    const names = [];
    let root;
    try {
      root = JSON.parse(bodyBuf.toString('utf8'));
    } catch (_e) {
      return names;
    }
    const content = root && root.content;
    if (!Array.isArray(content)) return names;
    for (let i = 0; i < content.length; i++) {
      const b = content[i];
      if (b && b.type === 'tool_use' && b.name && String(b.name).indexOf('mcp__') === 0) {
        names.push(String(b.name));
      }
    }
    return names;
  }

  function processRequestBody(buf, cfgBase) {
    let o;
    try {
      o = JSON.parse(buf.toString('utf8'));
    } catch (_e) {
      return { buf: buf, rec: null, stream: true };
    }
    const systemText = extractSystemText(o);
    const working_directory = extractWorkingDirectory(systemText);
    const activeSlug = cfgBase.profile || 'all';
    const picked = autoSelect(cfgBase, systemText, activeSlug);
    const slug = picked.slug;
    const prof = profileFromSlug(cfgBase, slug);
    const keep = prof.keep.length ? prof.keep : cfgBase.keep;

    const bytesBefore = Buffer.byteLength(JSON.stringify(o), 'utf8');
    const trace = filterTools(o, keep);
    const bytesAfter = Buffer.byteLength(JSON.stringify(o), 'utf8');
    if (trace.tools_removed > 0) {
      trace.tokens_saved = Math.floor((bytesBefore - bytesAfter) / 4);
    }

    const gates = applyGates(cfgBase, o);

    const stream = o.stream === true;
    const rec = {
      ts: new Date().toISOString(),
      tools_removed: trace.tools_removed,
      tokens_saved: trace.tokens_saved,
      compress_chars_saved: 0,
      profile: slug,
      removed_servers: trace.removed_servers,
      kept_servers: trace.kept_servers,
      auto_selected: picked.auto,
      auto_trigger: picked.trigger,
      inject_fired: gates.inject_fired,
      coach_kind: gates.coach_kind,
      budget_fired: gates.budget_fired,
      behavior_kind: gates.behavior_kind,
      working_directory: working_directory,
      tools_sent_count: trace.tools_sent_count,
      mcp_tools_invoked: [],
      tools_sent_by_server: trace.tools_sent_by_server,
    };

    // Extract first user message text for the profile-suggest call — kept out of rec
    // so it is never written to analytics.jsonl.
    let _suggestText = '';
    try {
      const msgs = o.messages;
      if (Array.isArray(msgs)) {
        for (let i = 0; i < msgs.length; i++) {
          const m = msgs[i];
          if (m && m.role === 'user') {
            if (typeof m.content === 'string') { _suggestText = m.content; break; }
            if (Array.isArray(m.content)) {
              for (let j = 0; j < m.content.length; j++) {
                const c = m.content[j];
                if (c && c.type === 'text' && typeof c.text === 'string') { _suggestText = c.text; break; }
              }
            }
            if (_suggestText) break;
          }
        }
      }
    } catch (_e) {}

    const out = Buffer.from(JSON.stringify(o), 'utf8');
    return { buf: out, rec: rec, stream: stream, suggestText: _suggestText };
  }

  function normalizeOptions(options) {
    if (!options || typeof options === 'string') {
      return null;
    }
    return options;
  }

  function requestPort(mod, options) {
    if (options.port != null && options.port !== '') {
      return Number(options.port);
    }
    return mod === https ? 443 : 80;
  }

  function shouldFilter(mod, options) {
    const opts = normalizeOptions(options);
    if (!opts) {
      return false;
    }
    const rawHost = opts.hostname || opts.host || '';
    const h = String(rawHost).split(':')[0];
    const method = (opts.method || 'GET').toUpperCase();
    const p = opts.path || '';
    if (h !== filterHost()) {
      return false;
    }
    const pe = filterPortEnv();
    if (pe != null && pe !== '' && String(requestPort(mod, opts)) !== String(pe)) {
      return false;
    }
    return method === 'POST' && p.includes('/v1/messages');
  }

  function parseEndArgs(args) {
    let chunk;
    let encoding;
    let cb;
    if (args.length === 0) {
      return { chunk: chunk, encoding: encoding, cb: cb };
    }
    if (typeof args[0] === 'function') {
      cb = args[0];
      return { chunk: chunk, encoding: encoding, cb: cb };
    }
    chunk = args[0];
    if (args.length > 1) {
      if (typeof args[1] === 'string') {
        encoding = args[1];
        if (typeof args[2] === 'function') {
          cb = args[2];
        }
      } else if (typeof args[1] === 'function') {
        cb = args[1];
      }
    }
    return { chunk: chunk, encoding: encoding, cb: cb };
  }

  function patch(mod) {
    const orig = mod.request;
    mod.request = function (options, onResponse) {
      if (!shouldFilter(mod, options)) {
        return orig.apply(this, arguments);
      }
      const cfgSnap = loadCfg();
      const bufs = [];
      let pendingRec = null;

      function handleResponse(res) {
        if (pendingRec && !pendingRec.stream && !pendingRec.done) {
          const chunks = [];
          res.on('data', function (c) {
            chunks.push(Buffer.isBuffer(c) ? c : Buffer.from(c));
          });
          res.on('end', function () {
            if (!pendingRec || pendingRec.done) return;
            pendingRec.done = true;
            try {
              const body = Buffer.concat(chunks);
              const inv = extractToolUsesFromAssistant(body);
              if (inv.length) {
                pendingRec.rec.mcp_tools_invoked = inv;
              }
            } catch (_e) {
              /* ignore */
            }
            appendAnalyticsLine(pendingRec.rec);
          });
        }
        if (typeof onResponse === 'function') {
          onResponse(res);
        }
      }

      const realReq = orig.call(this, options, handleResponse);

      const _end = realReq.end.bind(realReq);
      const _write = realReq.write.bind(realReq);

      realReq.write = function (chunk, encoding, callback) {
        let enc = encoding;
        let cb = callback;
        if (typeof enc === 'function') {
          cb = enc;
          enc = undefined;
        }
        if (chunk) {
          bufs.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk, enc));
        }
        if (typeof cb === 'function') {
          cb();
        }
        return true;
      };

      realReq.end = function () {
        const args = Array.prototype.slice.call(arguments);
        const parsed = parseEndArgs(args);
        if (parsed.chunk) {
          const c = parsed.chunk;
          bufs.push(Buffer.isBuffer(c) ? c : Buffer.from(c, parsed.encoding));
        }
        let body = Buffer.concat(bufs);
        let pr = null;
        try {
          pr = processRequestBody(body, cfgSnap);
          body = pr.buf;
        } catch (_e) {
          /* fail-open */
        }
        realReq.write = _write;
        realReq.end = _end;
        try {
          realReq.setHeader('Content-Length', body.length);
        } catch (_e) {
          /* ignore */
        }
        try {
          realReq.removeHeader('Transfer-Encoding');
        } catch (_e) {
          /* ignore */
        }

        if (pr && pr.rec) {
          if (pr.stream) {
            appendAnalyticsLine(pr.rec);
          } else {
            pendingRec = { rec: pr.rec, stream: false, done: false };
          }
          writeLastFilter({
            profile: pr.rec.profile,
            tools_removed: pr.rec.tools_removed,
            tokens_saved: pr.rec.tokens_saved,
            auto_selected: pr.rec.auto_selected,
          });
          // Fire-and-forget: ask the dashboard to compute a profile suggestion for the next request.
          (function () {
            const pe = process.env.CTX_DASHBOARD_PORT;
            const _cfg = loadCfg();
            const _port = pe != null && pe !== '' ? Number(pe)
              : typeof _cfg.dashboard_port === 'number' ? _cfg.dashboard_port : 8789;
            postProfileSuggest(_port, pr.rec.working_directory || '', pr.suggestText || '');
          }());
        }

        _write(body);
        const out = _end(parsed.cb);

        realReq.on('error', function () {
          if (pendingRec && !pendingRec.done) {
            pendingRec.done = true;
            appendAnalyticsLine(pendingRec.rec);
          }
        });

        return out;
      };

      return realReq;
    };
  }

  patch(http);
  patch(https);

  module.exports = {};
}
