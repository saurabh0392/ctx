// ─── Context home (Learning / Earning / Improving) ────────
// Draws only from /api/context. No simulated numbers: honest empty states until
// real decisions accrue.

let _ctxLoaded = false;
let _ctxData = null;

function setContextView(el) {
    document
        .querySelectorAll("#tab-context .loop-step")
        .forEach((s) => s.classList.remove("active"));
    document
        .querySelectorAll("#tab-context .ctx-view")
        .forEach((v) => v.classList.remove("active"));
    el.classList.add("active");
    document
        .getElementById("ctxview-" + el.dataset.view)
        .classList.add("active");
}

async function setContextPreset(preset) {
    try {
        await fetch("/api/context/preset", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ preset }),
        });
    } catch (e) {
        console.error("set preset failed", e);
    }
    loadContext();
}

function ctxToolClass(kind) {
    if (!kind) return "ft-generic";
    if (kind.startsWith("git")) return "ft-git";
    if (kind === "test") return "ft-test";
    if (kind === "grep") return "ft-grep";
    if (kind === "read") return "ft-read";
    if (kind === "mcp") return "ft-mcp";
    return "ft-generic";
}

function ctxToolLabel(row) {
    if (row.kind && row.kind.startsWith("git")) return "git";
    if (row.kind === "test") return "test";
    if (row.kind === "grep") return "grep";
    if (row.kind === "read") return "read";
    if (row.kind === "mcp") return "mcp";
    return (row.tool_name || "tool").slice(0, 8).toLowerCase();
}

function ctxFeedText(row) {
    const keep = row.lines_keep;
    const drop = row.lines_drop;
    const verb = row.applied ? "kept" : "would keep";
    const verb2 = row.applied ? "trimmed" : "would set aside";
    let title;
    if (drop <= 0) {
        title = `ctx looked at this ${ctxToolLabel(row)} output and ${row.applied ? "kept" : "would keep"} <span class="keep">all ${row.lines_total} lines</span>. Nothing worth trimming.`;
    } else {
        title = `ctx ${verb} <span class="keep">${keep} lines</span> and ${verb2} <span class="drop">${drop} lines</span> of ${ctxToolLabel(row)} output.`;
    }
    const why = row.applied
        ? "This tool earned its turn, so the trim is real. The full output is still in your transcript."
        : "Still only watching this tool. Nothing changed; ctx is checking whether dropping those lines would have hurt.";
    const tag = row.applied
        ? '<span class="feed-mode mode-on">trimmed</span>'
        : '<span class="feed-mode mode-watch">watching</span>';
    return { title, why, tag, cls: ctxToolClass(row.kind), label: ctxToolLabel(row) };
}

function renderContextLearning(d) {
    const stats = d.stats || {};
    document.getElementById("ctx-label-count").textContent = (stats.total || 0).toLocaleString();
    document.getElementById("ctx-corr-count").textContent = (stats.corrections_caused || 0).toLocaleString();
    document.getElementById("ctx-judged-count").textContent = (stats.joined || 0).toLocaleString();
    document.getElementById("ctx-today-count").textContent = (stats.today || 0).toLocaleString();

    const feed = document.getElementById("ctx-feed");
    const rows = d.feed || [];
    if (!rows.length) {
        feed.innerHTML =
            '<div class="feed-row"><span class="feed-text"><div class="feed-title">No tool results recorded yet.</div><div class="feed-why">Run some Claude Code turns, then ctx will start watching here. If nothing shows after a few turns, run <code>ctx ingest</code>.</div></span></div>';
    } else {
        feed.innerHTML = rows
            .map((r) => {
                const f = ctxFeedText(r);
                return `<div class="feed-row"><span class="feed-tool ${f.cls}">${esc(f.label)}</span><span class="feed-text"><div class="feed-title">${f.title} ${f.tag}</div><div class="feed-why">${f.why}</div></span></div>`;
            })
            .join("");
    }

    const tools = d.tools || [];
    const studying = tools.filter((t) => !t.active && !t.earned);
    const earned = tools.filter((t) => t.active || t.earned);
    const grid = document.getElementById("ctx-studying");
    if (!studying.length) {
        grid.innerHTML =
            '<div class="drow study"><div class="drow-id"><span class="drow-name notyet">not yet</span></div><div class="drow-desc">No tools are being studied yet. As tools return output, they appear here with their progress toward turning on.</div><div></div></div>';
    } else {
        grid.innerHTML = studying
            .map((t) => {
                const need = t.need || 50;
                const pct = Math.min(100, Math.round((t.joined / need) * 100));
                const amber = pct >= 80 ? " amber" : "";
                const badge = pct >= 80 ? "st-almost" : "st-shadow";
                const state = pct >= 80 ? "Almost ready" : "Watching";
                return `<div class="drow study">
          <div class="drow-id"><span class="drow-name">${esc(t.tool)}</span><span class="badge ${badge}">${state}</span></div>
          <div class="drow-desc">${t.decisions} results watched. ctx is checking whether trimming this tool ever caused you to re-read or correct.</div>
          <div>
            <div class="prog-top"><span class="prog-count"><b>${t.joined}</b> of ${need} judged runs</span><span class="prog-pct">${pct}%</span></div>
            <div class="bar-track"><div class="bar-fill${amber}" style="width:${pct}%"></div></div>
          </div>
        </div>`;
            })
            .join("");
    }

    const note = document.getElementById("ctx-earned-note");
    if (earned.length) {
        note.style.display = "";
        note.innerHTML = `<span class="ok">&#10003;</span> ${earned.map((t) => esc(t.tool)).join(" and ")} ${earned.length > 1 ? "have" : "has"} cleared the bar. See the proof in <b onclick="setContextView(document.querySelector('#tab-context [data-view=earning]'))">Earning</b>.`;
    } else {
        note.style.display = "none";
    }
}

function renderContextEarning(d) {
    const tools = (d.tools || []).filter((t) => t.active || t.earned);
    const el = document.getElementById("ctx-earned");
    const empty = document.getElementById("ctx-earning-empty");
    if (!tools.length) {
        el.innerHTML = "";
        empty.style.display = "";
        empty.innerHTML =
            'No tool has earned its turn yet. While ctx collects, this stays empty on purpose. Watch progress in <b onclick="setContextView(document.querySelector(\'#tab-context [data-view=learning]\'))">Learning</b>.';
        return;
    }
    empty.style.display = "none";
    el.innerHTML = tools
        .map((t) => {
            const stateBadge = t.active ? "On" : "Ready";
            const cap = t.active
                ? `${t.clean_runs} clean runs proved it<br>${t.corrections} corrections caused`
                : `cleared the bar on ${t.joined} runs<br>turn it on with the preset above`;
            return `<div class="drow earn">
        <div class="drow-id"><span class="drow-name">${esc(t.tool)}</span><span class="badge st-active">${stateBadge}</span></div>
        <div class="drow-desc">${t.active ? "Trimming this tool now. Failures, diffs, and the lines you work in are kept." : "Has earned activation on your own data. Flip the preset to turn it on."}</div>
        <div class="row-stat"><div class="row-stat-num green">${t.joined}</div><div class="row-stat-cap">${cap}</div></div>
      </div>`;
        })
        .join("");
}

function renderContextImproving(d) {
    const body = document.getElementById("ctx-improving-body");
    const m = d.model;
    if (!m || !m.version) {
        body.innerHTML = `<div class="hero-card"><div class="hero-eyebrow">Model for this repo</div>
      <div class="hero-number notyet" style="font-size:40px">not yet</div>
      <div class="hero-unit">no model trained yet</div>
      <div class="hero-sub">Once enough of your decisions have outcomes, ctx trains a local model and it sharpens every time you run <code>ctx context learn</code> or ingest. The version history will appear here.</div></div>`;
        return;
    }
    const hist = m.history || [];
    const rows = hist
        .map((h, i) => {
            const badge = i === 0 ? '<span class="badge st-active">latest</span>' : "";
            const auc = (h.holdout_auc != null) ? h.holdout_auc.toFixed(3) : "n/a";
            const base = (h.base_correction_rate != null) ? (h.base_correction_rate * 100).toFixed(1) + "%" : "n/a";
            return `<div class="drow ver"><div class="drow-id"><span class="drow-name">v${h.version}</span>${badge}</div>
        <div class="drow-desc">Trained on ${h.n_train || 0} labeled decisions. Holdout AUC ${auc}. Base correction rate ${base}.</div>
        <div class="drow-when">${esc((h.trained_at || "").slice(0, 10))}</div></div>`;
        })
        .join("");
    body.innerHTML = `<div class="hero-card" style="margin-bottom:24px">
      <div class="hero-eyebrow">Model for this repo</div>
      <div class="hero-number" style="font-size:52px">v${m.version}</div>
      <div class="hero-unit">holdout AUC ${m.holdout_auc.toFixed(3)}</div>
      <div class="hero-sub">The longer you use ctx, the better it gets at knowing what your sessions need. This value lives in your data; a generic trimmer cannot copy it.</div>
      <div class="hero-trust"><span class="trust-pill">base correction rate <b>${(m.base_correction_rate * 100).toFixed(1)}%</b></span></div>
    </div>
    <div class="section-head">How it sharpened</div>
    <div class="section-sub">Each retrain learns from the corrections and re-reads in your newest sessions.</div>
    <div class="row-list">${rows || '<div class="notyet">no history yet</div>'}</div>`;
}

function renderContextPreset(d) {
    const preset = d.preset || "off";
    document.querySelectorAll("#tab-context .preset-btn").forEach((b) => {
        b.classList.toggle("active", b.dataset.preset === preset);
    });
    const note = document.getElementById("ctx-preset-note");
    if (preset === "off") {
        note.textContent = "Collecting only. ctx records decisions but does not change tool output.";
    } else if (preset === "safe") {
        note.textContent = "Safe on. git, test, and grep trim once each clears its bar.";
    } else {
        note.textContent = "Full on. Every supported tool trims once it clears its bar.";
    }
}

function renderHomeStatus(d) {
    const el = document.getElementById("ctx-home-status-text");
    if (!el) return;
    const stats = d.stats || {};
    const total = stats.total || 0;
    const earned = (d.tools || []).filter((t) => t.active || t.earned);
    if (!total) {
        el.textContent =
            "Collecting on your work. No decisions recorded yet, so nothing is being changed. Run a few agent turns to get started.";
        return;
    }
    let tail;
    if (!earned.length) {
        tail = "No tool has earned trimming yet, which is the honest default.";
    } else {
        tail = `${earned.map((t) => t.tool).join(" and ")} ${earned.length > 1 ? "have" : "has"} earned trimming on your data.`;
    }
    el.textContent = `Collecting on your work. ${total.toLocaleString()} decisions recorded, ${(stats.joined || 0).toLocaleString()} judged. ${tail}`;
}

// Pick the one tool worth leading Home with: prefer one with a real after, then most evidence.
function pickHeadlineTool(tools) {
    if (!tools || !tools.length) return null;
    const withAfter = tools.filter((t) => t.trimmed_n > 0);
    const pool = withAfter.length ? withAfter : tools;
    return pool
        .slice()
        .sort((a, b) => b.baseline_n + b.trimmed_n - (a.baseline_n + a.trimmed_n))[0];
}

function renderHomeProof(p) {
    const card = document.getElementById("ctx-home-proof");
    const body = document.getElementById("ctx-home-proof-body");
    const eyebrow = document.getElementById("ctx-home-proof-eyebrow");
    if (!card) return;
    const t = pickHeadlineTool(p && p.tools);
    if (!t) {
        card.style.display = "none";
        return;
    }
    card.style.display = "";
    const runs = t.baseline_n + t.trimmed_n;
    const minN = (p && p.min_trimmed) || 30;
    if (t.verdict === "safe" && t.reread_delta) {
        eyebrow.textContent = `${t.tool} earned it`;
        const rd = t.reread_delta;
        body.innerHTML = `On your own work, trimming <b>${esc(t.tool)}</b> did not make you correct or re-read more often. The re-read change was ${pfSignedPts(rd.diff)} with a 95% interval of [${pfSignedPts(rd.lo)}, ${pfSignedPts(rd.hi)}], at or below zero. That is evidence from ${runs} of your would-trim runs, not a benchmark.`;
    } else if (t.verdict === "harmful") {
        eyebrow.textContent = `${t.tool} did not pass`;
        body.innerHTML = `ctx tested <b>${esc(t.tool)}</b> and the numbers said trimming it cost you re-reads or corrections, so ctx keeps it off. This is the safety layer working: a tool only trims when your data clears it.`;
    } else if (t.trialing || t.verdict === "collecting" || t.trimmed_n > 0) {
        eyebrow.textContent = `Proving ${t.tool} now`;
        body.innerHTML = `ctx is testing <b>${esc(t.tool)}</b> live. So far ${t.trimmed_n} trimmed runs against ${t.baseline_n} left alone. Once each side reaches about ${minN} runs, you will see whether trimming it is safe on your work.`;
    } else {
        eyebrow.textContent = "What we will prove";
        body.innerHTML = `ctx has wanted to trim <b>${esc(t.tool)}</b> on ${t.baseline_n} of your runs and left it alone every time. Start a live test on the Proof page and ctx will measure whether cutting it ever costs you a re-read or a correction.`;
    }
}

async function loadHomeProof() {
    try {
        const r = await fetch("/api/context/proof");
        const p = await r.json();
        renderHomeProof(p);
    } catch (e) {
        console.error("loadHomeProof failed", e);
    }
}

async function loadContext() {
    try {
        const r = await fetch("/api/context");
        const d = await r.json();
        _ctxData = d;
        renderContextPreset(d);
        renderHomeStatus(d);
        renderContextLearning(d);
        renderContextEarning(d);
        renderContextImproving(d);
    } catch (e) {
        console.error("loadContext failed", e);
    }
    loadHomeProof();
}
