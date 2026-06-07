// ─── Proof (the causal before/after, CTX-2) ───────────────
// Draws only from /api/context/proof. Every rate, interval, and verdict is computed
// server-side so this page can never disagree with the live activation gate.

let _proofMin = { baseline: 30, trimmed: 30 };

function pfPct(x) {
    return (x * 100).toFixed(1) + "%";
}

function pfSignedPts(x) {
    const v = (x * 100).toFixed(1);
    return (x >= 0 ? "+" : "") + v + " pts";
}

function pfMetricRow(name, m) {
    if (!m) {
        return `<div class="ba-metric"><span class="ba-mname">${name}</span><span class="ba-mright"><div class="ba-ci">not yet</div></span></div>`;
    }
    return `<div class="ba-metric"><span class="ba-mname">${name}</span><span class="ba-mright"><div class="ba-mval">${pfPct(m.rate)}</div><div class="ba-ci">[${pfPct(m.lo)}, ${pfPct(m.hi)}]</div></span></div>`;
}

function pfVerdictPill(v) {
    switch (v) {
        case "safe":
            return '<span class="verdict v-safe">Trimming looks safe</span>';
        case "harmful":
            return '<span class="verdict v-harm">Trimming hurt, kept off</span>';
        case "unclear":
            return '<span class="verdict v-wait">Too close to call</span>';
        case "collecting":
            return '<span class="verdict v-wait">Collecting the after</span>';
        default:
            return '<span class="verdict v-none">Not tested yet</span>';
    }
}

function pfDeltaSpan(delta) {
    // Up is bad (more corrections/re-reads when trimmed), down is good.
    const cls = delta.diff > 0 ? "up" : "down";
    return `<span class="${cls}">${pfSignedPts(delta.diff)}</span> [${pfSignedPts(delta.lo)}, ${pfSignedPts(delta.hi)}]`;
}

function pfDeltaSentence(t) {
    const parts = [];
    if (t.reread_delta) {
        parts.push(`re-reads changed by ${pfDeltaSpan(t.reread_delta)}`);
    }
    if (t.correction_delta) {
        parts.push(`corrections changed by ${pfDeltaSpan(t.correction_delta)}`);
    }
    if (!parts.length) return "";
    let lead = `When ctx trimmed <b>${esc(t.tool)}</b>, ${parts.join(" and ")}.`;
    let tail;
    if (t.verdict === "safe") {
        tail =
            " Both intervals sit at or below zero, so trimming is not measurably worse than leaving it alone. That is enough to earn it.";
    } else if (t.verdict === "harmful") {
        tail =
            " An interval sits clearly above zero, so trimming measurably hurt. ctx keeps this tool off.";
    } else if (t.verdict === "collecting") {
        tail = ` Still collecting the trimmed side. ctx wants about ${_proofMin.trimmed} trimmed runs and ${_proofMin.baseline} left-alone runs before it calls this honestly.`;
    } else {
        tail =
            " The intervals still straddle zero with too much spread, so it is too close to call. A few more trimmed runs will settle it.";
    }
    return `<div class="delta-row">${lead}${tail}</div>`;
}

function pfTrimmedCol(t) {
    if (t.trimmed_n > 0) {
        return `<div class="ba-col"><div class="ba-label">Trimmed (cut)</div><div class="ba-n">${t.trimmed_n} runs</div>${pfMetricRow("Corrections", t.trimmed_corrections)}${pfMetricRow("Re-reads", t.trimmed_rereads)}</div>`;
    }
    const start = t.trialing
        ? ""
        : `<br><button class="tb-btn" style="margin-top:10px" onclick="proofTrial('${esc(t.tool)}', true)">Start a live test on ${esc(t.tool)}</button>`;
    return `<div class="ba-col empty"><div class="ba-label">Trimmed (cut)</div><div class="ba-empty-text">Nothing trimmed yet, so there is no honest after to show. Start a live test and ctx will trim ${esc(t.tool)} for real and measure what happens next.${start}</div></div>`;
}

function pfBaselineCol(t) {
    if (t.baseline_n > 0) {
        return `<div class="ba-col"><div class="ba-label">Left alone (baseline)</div><div class="ba-n">${t.baseline_n} runs</div>${pfMetricRow("Corrections", t.baseline_corrections)}${pfMetricRow("Re-reads", t.baseline_rereads)}</div>`;
    }
    return `<div class="ba-col empty"><div class="ba-label">Left alone (baseline)</div><div class="ba-empty-text">No would-trim runs left alone yet for this tool.</div></div>`;
}

function renderProofBanner(d) {
    const el = document.getElementById("proof-banner");
    const trials = d.trial_tools || [];
    if (!trials.length) {
        el.innerHTML = "";
        return;
    }
    const names = trials.map((t) => `<b>${esc(t)}</b>`).join(", ");
    el.innerHTML = `<div class="trial-banner">
      <div class="tb-text">A live test is running on ${names}. ctx is trimming it for real so it can collect the after. This is a real change to your output, scoped to one tool.</div>
      <div class="tb-actions"><button class="tb-btn ghost" onclick="proofTrial('${esc(trials[0])}', false)">Stop the test</button></div>
    </div>`;
}

function renderProofList(d) {
    const el = document.getElementById("proof-list");
    const tools = d.tools || [];
    if (!tools.length) {
        el.innerHTML =
            '<div class="proof-tool"><div class="ba-empty-text">Nothing to prove yet. ctx only has a before and after once its heuristic wants to trim a tool during your real work. Keep using your agents, and the first tool will show up here.</div></div>';
        return;
    }
    el.innerHTML = tools
        .map((t) => {
            const pop = `${t.baseline_n + t.trimmed_n} would-trim runs`;
            const delta = pfDeltaSentence(t);
            return `<div class="proof-tool">
        <div class="proof-head">
          <span class="proof-name">${esc(t.tool)}</span>
          ${pfVerdictPill(t.verdict)}
          <span class="proof-pop">${pop}</span>
        </div>
        <div class="ba-grid">${pfBaselineCol(t)}${pfTrimmedCol(t)}</div>
        ${delta}
      </div>`;
        })
        .join("");
}

async function proofTrial(tool, on) {
    try {
        const r = await fetch("/api/context/trial", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ tool, on }),
        });
        if (!r.ok && r.status !== 204) {
            console.error("trial toggle failed", await r.text());
        }
    } catch (e) {
        console.error("trial toggle failed", e);
    }
    loadProof();
}

async function loadProof() {
    try {
        const r = await fetch("/api/context/proof");
        const d = await r.json();
        _proofMin = { baseline: d.min_baseline || 30, trimmed: d.min_trimmed || 30 };
        renderProofBanner(d);
        renderProofList(d);
    } catch (e) {
        console.error("loadProof failed", e);
    }
}
