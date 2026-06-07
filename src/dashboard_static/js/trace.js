// ─── Tab 4: Request Trace ────────────────────────────────

const GATE_META = {
    filter: { accent: "#93c043" },
    auto: { accent: "#60a5fa" },
    inject: { accent: "#fbbf24" },
    adaptive: { accent: "#a78bfa" },
    coach: { accent: "#f87171" },
    behavior: { accent: "#c4b5fd" },
    budget: { accent: "#fb923c" },
    compress: { accent: "#22d3ee" },
};

async function loadTrace() {
    const [requests, hookTraces, plan, settings] = await Promise.all([
        fetch(appendSince("/api/requests?limit=100")).then((r) => r.json()),
        fetch(appendSince("/api/hook-traces?limit=100"))
        .then((r) => r.json())
        .catch(() => []),
        fetch("/api/experiment/plan").then((r) => r.json()).catch(() => ({ configured: false })),
        fetch("/api/settings").then((r) => r.json()).catch(() => ({})),
    ]);
    const compressEnabled = settings.compress_enabled !== false;
    const el = document.getElementById("trace-list");

    if (!requests.length && !hookTraces.length) {
        let sub = "No trace events recorded yet. Use Claude Code with ctx hooks enabled. Each turn records a trace row automatically.";
        if (plan.configured && plan.hooks_enabled === false) {
            sub = "Days 1–2 (pre-ctx): hooks are off on purpose. Session spend still ingests, but per-request traces need hooks. They return on day 3 (ctx warmup). Reload your IDE after the phase changes.";
        } else if (plan.configured && plan.phase_applied === false) {
            sub = "Calendar advanced but hooks have not synced yet. Open Experiment or run ctx experiment tick, then reload your IDE.";
        }
        el.innerHTML = `<div class="card" style="padding:20px;margin-bottom:16px;border-color:rgba(147,192,67,.25)">
      <div class="section-head" style="margin-bottom:8px">No trace events</div>
      <div class="section-sub" style="margin-bottom:0">${esc(sub)}</div>
    </div>`;
        return;
    }

    const todayStr = new Date().toISOString().slice(0, 10);
    const todayReqs = requests.filter((r) => r.ts.slice(0, 10) === todayStr);
    const todayHookTraces = hookTraces.filter(
        (h) => h.ts.slice(0, 10) === todayStr,
    );
    const todayTokens = todayReqs.reduce((s, r) => s + r.tokens_saved, 0);
    const todayCompressChars = todayHookTraces.reduce(
        (s, h) => s + (h.compress_chars_saved || 0),
        0,
    );
    const todayCost = todayReqs.reduce((s, r) => s + r.cost_saved, 0);
    const totalToday = todayReqs.length + todayHookTraces.length;
    const autoCount =
        todayReqs.filter((r) => r.auto_selected).length +
        todayHookTraces.filter((h) => h.auto_selected).length;
    const autoLine =
        autoCount > 0 ?
        ` Profile auto-switched ${autoCount} time${autoCount !== 1 ? "s" : ""}.` :
        "";

    let bannerParts = [];
    if (todayReqs.length) bannerParts.push(`${todayReqs.length} proxy traces`);
    if (todayHookTraces.length)
        bannerParts.push(`${todayHookTraces.length} hook traces`);
    if (todayTokens > 0) bannerParts.push(`${fmtK(todayTokens)} schema tok stripped`);
    if (todayCompressChars > 0)
        bannerParts.push(`${fmtK(todayCompressChars)} chars compressed`);

    const banner = totalToday ?
        `
    <div style="background:var(--surface2);border:1px solid var(--border);border-radius:10px;padding:14px 18px;margin-bottom:16px;display:flex;gap:32px;align-items:center">
      <div>
        <div style="font-size:11px;color:var(--t3);text-transform:uppercase;letter-spacing:.05em">Today</div>
        ${todayCost > 0 ? `<div style="font-size:22px;font-weight:700;color:var(--green)">-$${todayCost.toFixed(3)}</div>` : `<div style="font-size:22px;font-weight:700;color:var(--t2)">${totalToday} turns</div>`}
        <div style="font-size:11px;color:var(--t3)">${bannerParts.join(", ")}${autoLine}</div>
      </div>
      <div style="flex:1;text-align:right;font-size:11px;color:var(--t4)">Click any row to expand details.</div>
    </div>` :
        "";

    // Merge into unified timeline
    const unified = [];
    for (const req of requests)
        unified.push({
            type: "request",
            ts: req.ts,
            data: req
        });
    for (const ht of hookTraces)
        unified.push({
            type: "hook_trace",
            ts: ht.ts,
            data: ht
        });
    unified.sort((a, b) => b.ts.localeCompare(a.ts));
    const deduped = dedupeTraceTimeline(unified);

    el.innerHTML =
        banner +
        deduped
        .map((item, i) => {
            if (item.type === "request") return traceRow(item.data, i, compressEnabled);
            return hookTraceRow(item.data, i, compressEnabled);
        })
        .join("");
}

/** One row per prompt: drop proxy request traces when a hook trace covers the same turn. */
function dedupeTraceTimeline(items) {
    const hooks = items.filter((i) => i.type === "hook_trace");
    return items.filter((item) => {
        if (item.type !== "request") return true;
        const req = item.data;
        return !hooks.some((h) => traceMatchesHookRequest(req, item.ts, h.data, h.ts));
    });
}

function traceMatchesHookRequest(req, reqTs, ht, htTs) {
    if (req.working_directory && ht.working_directory
        && req.working_directory !== ht.working_directory) {
        return false;
    }
    const diffMs = Math.abs(new Date(reqTs).getTime() - new Date(htTs).getTime());
    if (diffMs > 3 * 60 * 1000) return false;
    if (req.profile && ht.profile && req.profile !== ht.profile) return false;
    return true;
}

function renderAbBadges(abGroup) {
    if (!abGroup) return "";
    return abGroup
        .split(/\s+/)
        .filter(Boolean)
        .map((p) => {
            const m = p.match(/^([PIACXM]):([TC])$/);
            if (!m) return "";
            const cls = m[2] === "T" ? "ab-chip-t" : "ab-chip-c";
            return `<span class="ab-chip ${cls}">${esc(p)}</span>`;
        })
        .join("");
}

function renderCtxBulletList(items) {
    return (
        '<ul class="ctx-bullet-list">' +
        items
        .map((it) => {
            const detail = it.detail ?
                `<span class="ctx-bullet-toggle" onclick="event.stopPropagation();this.parentElement.classList.toggle('expanded')">+</span><div class="ctx-bullet-detail">${it.detail}</div>` :
                "";
            return `<li class="ctx-bullet">${it.line}${detail}</li>`;
        })
        .join("") +
        "</ul>"
    );
}

const TRACE_CTX_SAVINGS_RATE = 0.30;
/** Rough chars-to-tokens for compress context (not invoice-grade). */
const TRACE_CHARS_PER_TOKEN = 4;

function estimateCtxTokensFromChars(chars) {
    if (!chars || chars <= 0) return 0;
    return Math.max(1, Math.round(chars / TRACE_CHARS_PER_TOKEN));
}

function traceCostTrio(tokensSaved, costUsd, enriched) {
    const ctxSavings = (tokensSaved / 1_000_000) * TRACE_CTX_SAVINGS_RATE;
    const afterCtx = enriched && costUsd > 0 ? costUsd : null;
    const estimatedTotal = afterCtx != null ? afterCtx + ctxSavings : null;
    return { estimatedTotal, afterCtx, ctxSavings, filterTokens: tokensSaved || 0 };
}

function traceSummaryStat(opts) {
    const parts = [];
    const totalTools = opts.totalTools || 0;
    const toolsRemoved = opts.toolsRemoved || 0;
    const tokensSaved = opts.tokensSaved || 0;
    const compressChars = opts.compressChars || 0;

    if (totalTools > 0 && toolsRemoved > 0) {
        const pct = Math.round((toolsRemoved / totalTools) * 100);
        parts.push(
            `-<strong>${toolsRemoved}</strong> of ${totalTools} tools · <strong>${fmtK(tokensSaved)}</strong> schema tok · ${pct}% cut`,
        );
    } else if (totalTools > 0) {
        parts.push(`${totalTools} tools (no schema cut)`);
    }

    if (compressChars > 0) {
        const est = estimateCtxTokensFromChars(compressChars);
        parts.push(
            `<span class="trace-compress-stat"><strong>${fmtK(compressChars)}</strong> chars out · ~<strong>${fmtK(est)}</strong> ctx</span>`,
        );
    }

    if (parts.length === 0) {
        return "No ctx savings on this turn yet";
    }
    return parts.join(" · ");
}

function expansionReasonLabel(reason) {
    if (reason === "keyword") return "you mentioned it in the prompt";
    if (reason === "semantic") return "similar past sessions used it";
    if (reason === "access_friction") return "Claude could not access it on the last turn";
    return "session recovery";
}

function renderExpansionBlock(entries) {
    if (!entries || !entries.length) return "";
    const lines = entries.map((e) => {
        const name = esc(e.display || e.target || "tool");
        const why = esc(expansionReasonLabel(e.reason));
        return `<div class="trace-expansion-item"><strong>${name}</strong> · ${why}</div>`;
    });
    return `<div class="trace-expansion-block">
      <div class="trace-expansion-title">Tools un-denied for this session</div>
      ${lines.join("")}
    </div>`;
}

function renderTurnPanel(opts) {
    const prompt = opts.prompt || "";
    const meta = opts.meta || "";
    const promptBlock = prompt ?
        `<div class="trace-turn-prompt" onclick="event.stopPropagation();this.classList.toggle('expanded')">${esc(prompt)}</div>` :
        `<div class="trace-turn-prompt trace-turn-prompt-pending">${opts.enriched ? "Prompt not recorded for this turn." : "Prompt and savings fill in after ingest."}</div>`;

    const rows = [];
    const totalTools = opts.totalTools || 0;
    const toolsRemoved = opts.toolsRemoved || 0;
    const toolsKept = opts.toolsKept || 0;
    const tokensSaved = opts.tokensSaved || 0;
    const compressChars = opts.compressChars || 0;
    const compressCount = opts.compressCount || 0;

    if (totalTools > 0 && toolsRemoved > 0) {
        const pct = Math.round((toolsRemoved / totalTools) * 100);
        const schemaUsd = fmtCost((tokensSaved / 1_000_000) * TRACE_CTX_SAVINGS_RATE);
        rows.push(
            `<span class="trace-turn-metric-label">Schema filter</span><span class="trace-turn-metric-val">${toolsKept} of ${totalTools} tools kept (${pct}% cut) · ${fmtK(tokensSaved)} tok · est. ${schemaUsd}/turn</span>`,
        );
    } else if (totalTools > 0) {
        rows.push(
            `<span class="trace-turn-metric-label">Schema filter</span><span class="trace-turn-metric-val">${totalTools} tools, none stripped</span>`,
        );
    }

    if (compressChars > 0) {
        const est = estimateCtxTokensFromChars(compressChars);
        const toolNote =
            compressCount > 0 ?
            ` · ${compressCount} tool call${compressCount === 1 ? "" : "s"}` :
            "";
        rows.push(
            `<span class="trace-turn-metric-label trace-turn-metric-compress">Output compress</span><span class="trace-turn-metric-val trace-turn-metric-compress">${fmtK(compressChars)} chars · ~${fmtK(est)} ctx kept${toolNote}</span>`,
        );
    }

    if (opts.enriched && opts.costUsd > 0) {
        rows.push(
            `<span class="trace-turn-metric-label">Turn cost</span><span class="trace-turn-metric-val">${fmtCost(opts.costUsd)} · ${opts.model ? esc(opts.model) + " · " : ""}${fmtK(opts.inputTok || 0)} in · ${fmtK(opts.outputTok || 0)} out${opts.cacheRead ? " · " + fmtK(opts.cacheRead) + " cache" : ""}</span>`,
        );
    } else if (!opts.enriched && (totalTools > 0 || compressChars > 0)) {
        rows.push(
            `<span class="trace-turn-metric-label">Turn cost</span><span class="trace-turn-metric-val trace-turn-metric-pending">pending ingest</span>`,
        );
    }

    if (opts.ctxLine) {
        rows.push(
            `<span class="trace-turn-metric-label">Profile</span><span class="trace-turn-metric-val">${esc(opts.ctxLine)}</span>`,
        );
    }
    const expansionHtml = renderExpansionBlock(opts.toolsExpanded);
    for (const action of opts.ctxActions || []) {
        rows.push(
            `<span class="trace-turn-metric-label">Ctx</span><span class="trace-turn-metric-val">${esc(action)}</span>`,
        );
    }

    const metrics =
        rows.length > 0 ?
        `<div class="trace-turn-metrics">${rows.map((r) => `<div class="trace-turn-metric-row">${r}</div>`).join("")}</div>` :
        `<div class="trace-turn-metrics trace-turn-metrics-empty">No savings recorded on this turn yet.</div>`;

    const pipeline = opts.pipelineItems && opts.pipelineItems.length ?
        `<details class="trace-pipeline-fold" onclick="event.stopPropagation()">
      <summary>Pipeline steps</summary>
      <div class="trace-flow-inline">${opts.pipelineItems.map((item) => {
            if (item.type === "link") return renderTraceFlowLink(item);
            return renderTraceFlowNode(item);
        }).join("")}</div>
    </details>` :
        "";

    return `<div class="trace-turn-panel">
    ${meta ? `<div class="trace-turn-meta">${meta}</div>` : ""}
    ${promptBlock}
    ${expansionHtml}
    ${metrics}
    ${pipeline}
  </div>`;
}

function renderTraceCostStack(trio, compact, compressChars) {
    const chars = compressChars || 0;
    const hasFilter = (trio.filterTokens || 0) > 0;
    const hasCompress = chars > 0;
    const est = trio.estimatedTotal != null ? fmtCost(trio.estimatedTotal) : "n/a";
    const after = trio.afterCtx != null ? fmtCost(trio.afterCtx) : "n/a";
    const filterSave = fmtCost(trio.ctxSavings);
    const filterLabelShort = hasCompress ? "Schema filter" : "Savings";
    const filterLabelLong = hasCompress ? "Schema filter savings" : "Filter savings (ctx)";
    const filterVal = hasFilter ?
        filterSave :
        (hasCompress ? "none" : filterSave);
    const filterValCls = hasFilter ? " trace-cost-save" : "";
    const compressLine = hasCompress ?
        `<div class="trace-cost-row trace-cost-compress-row"><span class="trace-cost-label">${compact ? "Output compress" : "Output compress (ctx kept)"}</span><span class="trace-cost-val trace-cost-compress">${fmtK(chars)} chars · ~${fmtK(estimateCtxTokensFromChars(chars))} ctx</span></div>` :
        "";
    if (compact) {
        return `<div class="trace-cost-stack trace-cost-stack-compact">
      <div class="trace-cost-row"><span class="trace-cost-label">Est. total</span><span class="trace-cost-val">${est}</span></div>
      <div class="trace-cost-row"><span class="trace-cost-label">After ctx</span><span class="trace-cost-val trace-cost-after">${after}</span></div>
      <div class="trace-cost-row"><span class="trace-cost-label">${filterLabelShort}</span><span class="trace-cost-val${filterValCls}">${filterVal}</span></div>
      ${compressLine}
    </div>`;
    }
    return `<div class="trace-cost-stack">
    <div class="trace-cost-row"><span class="trace-cost-label">Estimated total cost</span><span class="trace-cost-val">${est}</span></div>
    <div class="trace-cost-row"><span class="trace-cost-label">Total cost (after ctx)</span><span class="trace-cost-val trace-cost-after">${after}</span></div>
    <div class="trace-cost-row"><span class="trace-cost-label">${filterLabelLong}</span><span class="trace-cost-val${filterValCls}">${filterVal}</span></div>
    ${compressLine}
  </div>`;
}

function parseSimilarityTrigger(core) {
    const rest = core.slice("similarity:".length);
    const m = rest.match(/^([\d.]+)·(\d+)$/);
    if (m) {
        return { avg: m[1], sessions: parseInt(m[2], 10), legacy: false };
    }
    return { avg: rest, sessions: null, legacy: true };
}

function formatSimilarityDetail(core) {
    const { avg, sessions, legacy } = parseSimilarityTrigger(core);
    if (!legacy && sessions != null) {
        const n = sessions === 1 ? "1 session" : sessions + " sessions";
        return avg + " avg match · " + n;
    }
    return avg + " vote share";
}

function formatAutoTrigger(trigger, profileLabel) {
    const t = trigger || "cwd";
    const confirmed = t.endsWith(":confirmed");
    const core = confirmed ? t.slice(0, -":confirmed".length) : t;
    if (core.startsWith("similarity:")) {
        const detail = formatSimilarityDetail(core);
        if (confirmed) {
            return `Confirmed <strong>${esc(profileLabel)}</strong> via similar sessions (${esc(detail)})`;
        }
        return `Similar sessions (${esc(detail)}) &rarr; ${esc(profileLabel)}`;
    }
    if (core.startsWith("cwd:")) {
        if (confirmed) {
            return `Confirmed <strong>${esc(profileLabel)}</strong> via path ${esc(core.slice(4))}`;
        }
        return `Path ${esc(core.slice(4))} &rarr; ${esc(profileLabel)}`;
    }
    if (core.startsWith("keyword:")) {
        if (confirmed) {
            return `Confirmed <strong>${esc(profileLabel)}</strong> via keyword ${esc(core.slice(8))}`;
        }
        return `Keyword ${esc(core.slice(8))} &rarr; ${esc(profileLabel)}`;
    }
    return `Matched ${esc(t)} &rarr; ${esc(profileLabel)}`;
}

function parseAbArm(abGroup, letter) {
    if (!abGroup) return null;
    const m = String(abGroup).match(new RegExp(`${letter}:([TC])`));
    if (!m) return null;
    return m[1] === "T";
}

function traceFilterDesc(toolsRemoved, toolsKept, filterMode, isHook, removedServerCount) {
    if (toolsRemoved <= 0) return "No tools stripped";
    if (isHook) return `-${toolsRemoved} tools (${esc(filterMode)} mode)`;
    return `-${toolsRemoved} tools from ${removedServerCount} servers, kept ${toolsKept}`;
}

/** Auto-profile → profile-filter consequence for the RHS pipeline diagram. */
function traceAutoFilterConsequence(opts) {
    const {
        profileLabel,
        pinnedProfile,
        effectiveProfile,
        autoSelected,
        autoTrigger,
        toolsRemoved,
        toolsKept,
        filterMode = "soft",
        abGroup,
        isHook,
        removedServerCount = 0,
    } = opts;
    const ga = GATE_META;
    const profileAb = parseAbArm(abGroup, "P");
    const pinned = pinnedProfile || profileLabel;
    const effective = effectiveProfile || profileLabel;
    const confirmed = !!(autoTrigger && String(autoTrigger).endsWith(":confirmed"));
    const filterNodeBase = {
        id: "filter",
        name: "Profile Filter",
        accent: ga.filter?.accent,
        desc: traceFilterDesc(toolsRemoved, toolsKept, filterMode, isHook, removedServerCount),
    };

    if (profileAb === false) {
        const autoPick = effective || pinned;
        return {
            auto: {
                id: "auto",
                name: "Auto-Profile",
                fired: autoSelected,
                superseded: !autoSelected,
                desc: autoSelected ?
                    formatAutoTrigger(autoTrigger, autoPick) :
                    `No match — pinned ${esc(pinned)}`,
                accent: ga.auto?.accent,
            },
            link: {
                kind: "superseded",
                text: "A/B control arm → filter off for this prompt (auto still runs)",
            },
            filter: {
                ...filterNodeBase,
                fired: false,
                superseded: true,
                desc: "Skipped — A/B control arm (all tools)",
            },
            ctxLine: autoSelected ?
                `Auto matched ${autoPick}; A/B control arm disabled filtering for this prompt.` :
                `No auto match. Pinned ${pinned} feeds the filter when not on A/B control.`,
        };
    }

    if (filterMode === "off") {
        return {
            auto: {
                id: "auto",
                name: "Auto-Profile",
                fired: autoSelected,
                desc: autoSelected ?
                    formatAutoTrigger(autoTrigger, effective) :
                    "No match — filter mode off",
                accent: ga.auto?.accent,
            },
            link: { kind: "off", text: "Filter mode off — profile pick does not change deny list" },
            filter: { ...filterNodeBase, fired: false, desc: "Disabled in config" },
            ctxLine: autoSelected ?
                `Auto-profile matched ${effective}, but filtering is off.` :
                `Pinned ${pinned} profile; filtering is off.`,
        };
    }

    if (autoSelected && confirmed) {
        return {
            auto: {
                id: "auto",
                name: "Auto-Profile",
                fired: true,
                desc: formatAutoTrigger(autoTrigger, effective),
                accent: ga.auto?.accent,
            },
            link: {
                kind: "confirms",
                text: `→ confirmed ${effective} — filter uses existing deny list (no resync) →`,
            },
            filter: { ...filterNodeBase, fired: toolsRemoved > 0 },
            ctxLine: `Auto-profile confirmed ${effective}; filter applied without resyncing deny rules.`,
        };
    }

    if (autoSelected && !confirmed) {
        return {
            auto: {
                id: "auto",
                name: "Auto-Profile",
                fired: true,
                desc: formatAutoTrigger(autoTrigger, effective),
                accent: ga.auto?.accent,
            },
            link: {
                kind: "drives",
                text: `→ switched to ${effective} — resynced permissions.deny →`,
            },
            filter: { ...filterNodeBase, fired: toolsRemoved > 0 },
            ctxLine: `Auto-profile switched to ${effective} and resynced the deny list.`,
        };
    }

    return {
        auto: {
            id: "auto",
            name: "Auto-Profile",
            fired: false,
            desc: `No match — using pinned ${esc(pinned)}`,
            accent: ga.auto?.accent,
        },
        link: {
            kind: "manual",
            text: `→ pinned ${pinned} profile → filter applies its keep list`,
        },
        filter: { ...filterNodeBase, fired: toolsRemoved > 0 },
        ctxLine: `No auto match — pinned ${pinned} profile drives filtering.`,
    };
}

function renderTraceFlowNode(n) {
    const isAnchor = !!n.anchor;
    let cls = "trace-flow-node";
    if (isAnchor) cls += " anchor fired";
    else if (n.superseded) cls += " superseded";
    else if (n.fired) cls += " fired";
    else cls += " dimmed";
    const style = (n.fired || n.superseded) && n.accent ? `--node-accent:${n.accent}` : "";
    return `<div class="${cls}" style="${style}">
    <div class="trace-flow-node-name">${esc(n.name)}</div>
    <div class="trace-flow-node-desc">${n.desc}</div>
  </div>`;
}

function renderTraceFlowLink(link) {
    if (!link) return "";
    const bridgeCls = ["manual", "confirms", "drives"].includes(link.kind) ?
        " trace-flow-bridge" :
        "";
    return `<div class="trace-flow-link trace-flow-link-${esc(link.kind)}${bridgeCls}">${esc(link.text)}</div>`;
}

function renderTraceFlowPanel(items) {
    const body = items
        .map((item) => {
            if (item.type === "link") return renderTraceFlowLink(item);
            return renderTraceFlowNode(item);
        })
        .join("");
    return `<div class="trace-flow-panel">
    <div class="trace-flow-eyebrow">ctx pipeline</div>
    ${body}
  </div>`;
}

function buildTracePipelineItems(opts) {
    const ga = GATE_META;
    const consequence = traceAutoFilterConsequence(opts);
    const { auto, link, filter } = consequence;
    const items = [{
            type: "node",
            id: "in",
            name: "Request in",
            fired: true,
            anchor: true,
            desc: opts.requestInDesc,
        },
        { type: "node", ...auto },
        { type: "link", ...link },
        { type: "node", ...filter },
    ];

    if (opts.injectFired != null) {
        items.push({
            type: "node",
            id: "inject",
            name: "Inject",
            fired: opts.injectFired,
            desc: opts.injectFired ? "system_prefix.md prepended" : "Not active",
            accent: ga.inject?.accent,
        });
    }
    if (opts.adaptiveFired != null) {
        items.push({
            type: "node",
            id: "adaptive",
            name: "Adaptive",
            fired: opts.adaptiveFired,
            desc: opts.adaptiveFired ? "adaptive_prefix.md appended" : "Not active",
            accent: ga.adaptive?.accent,
        });
    }
    if (opts.coachKind != null) {
        items.push({
            type: "node",
            id: "coach",
            name: "Coaching",
            fired: !!opts.coachKind,
            desc: opts.coachKind ? esc(opts.coachKind) : UI_EMPTY,
            accent: ga.coach?.accent,
        });
    }
    if (opts.behaviorKind != null) {
        items.push({
            type: "node",
            id: "behavior",
            name: "Behavior Guard",
            fired: !!opts.behaviorKind,
            desc: opts.behaviorKind ? esc(opts.behaviorKind) : UI_EMPTY,
            accent: ga.behavior?.accent,
        });
    }
    if (opts.budgetFired != null) {
        items.push({
            type: "node",
            id: "budget",
            name: "Budget Guard",
            fired: opts.budgetFired,
            desc: opts.budgetFired ? "Cost alert fired" : UI_EMPTY,
            accent: ga.budget?.accent,
        });
    }

    items.push({
        type: "node",
        id: "out",
        name: "Sent to API",
        fired: true,
        anchor: true,
        desc: opts.sentDesc,
    });

    if (opts.compressEnabled || (opts.compressCharsSaved != null && opts.compressCharsSaved > 0)) {
        const saved = opts.compressCharsSaved || 0;
        const count = opts.compressEventCount || 0;
        const countSuffix =
            saved > 0 && count > 0 ?
            ` (${count} tool${count === 1 ? "" : "s"})` :
            "";
        items.push({
            type: "link",
            kind: "post",
            text: "after tool output →",
        });
        items.push({
            type: "node",
            id: "compress",
            name: "Output Compress",
            fired: saved > 0,
            desc: saved > 0 ?
                `${fmtK(saved)} chars compressed${countSuffix}` :
                "Armed on PostToolUse",
            accent: ga.compress?.accent,
        });
    }
    return { items, consequence };
}

function hookTraceRow(ht, i, compressEnabled) {
    const ts = fmtTs(ht.ts);
    const profileLabel = ht.profile || "all";
    const pinnedProfile = ht.pinned_profile || profileLabel;
    const effectiveProfile = ht.effective_profile || profileLabel;
    const filterMode = ht.filter_mode || "soft";
    const autoChip = ht.auto_selected ?
        `<span class="trace-auto-chip">auto: ${ht.auto_trigger || "matched"}</span>` :
        "";
    const modeChip = ht.mode ?
        `<span class="trace-profile-chip" title="This request used the ${esc(ht.mode)} mode (profile and feature toggles bundled)">${esc(ht.mode)}</span>` :
        "";

    const enrichedBadge = ht.enriched ?
        "" :
        '<span style="font-size:9px;padding:1px 6px;border-radius:3px;background:#3a3a20;color:#c8b44a;margin-left:8px">awaiting ingest</span>';

    const costUsd = ht.cost_usd || 0;
    const inputTok = ht.input_tokens || 0;
    const outputTok = ht.output_tokens || 0;
    const cacheRead = ht.cache_read_tokens || 0;
    const model = ht.model || "";

    const toolsKept = ht.tools_kept || 0;
    const toolsRemoved = ht.tools_removed || 0;
    const tokensSaved = ht.tokens_saved || 0;
    const compressChars = ht.compress_chars_saved || 0;
    const compressCount = ht.compress_event_count || 0;
    const totalTools = toolsKept + toolsRemoved;
    const costTrio = traceCostTrio(tokensSaved, costUsd, ht.enriched);

    const pipeline = buildTracePipelineItems({
        profileLabel,
        pinnedProfile,
        effectiveProfile,
        autoSelected: ht.auto_selected,
        autoTrigger: ht.auto_trigger,
        toolsRemoved,
        toolsKept,
        filterMode,
        abGroup: ht.ab_group,
        isHook: true,
        requestInDesc: `${totalTools} tools from all servers`,
        sentDesc: `${toolsKept} tools, ~${fmtK(tokensSaved)} tokens saved`,
        injectFired: ht.inject_fired,
        adaptiveFired: !!ht.adaptive_fired,
        coachKind: ht.coach_kind,
        budgetFired: ht.budget_fired,
        compressEnabled,
        compressCharsSaved: compressChars,
        compressEventCount: compressCount,
    });

    const ctxActions = [];
    if (ht.inject_fired) ctxActions.push("Prepended system_prefix.md");
    if (ht.adaptive_fired) ctxActions.push("Appended adaptive_prefix.md");
    if (ht.coach_kind) ctxActions.push(`Coaching: ${ht.coach_kind}`);
    if (ht.budget_fired) ctxActions.push("Session cost alert fired");

    const abBadges = renderAbBadges(ht.ab_group);
    const metaParts = [];
    if (ht.working_directory) metaParts.push(`<code>${esc(ht.working_directory)}</code>`);
    metaParts.push(esc(ts));

    const turnPanel = renderTurnPanel({
        prompt: ht.human_text_prefix,
        meta: metaParts.join(" · "),
        enriched: ht.enriched,
        model,
        inputTok,
        outputTok,
        cacheRead,
        totalTools,
        toolsKept,
        toolsRemoved,
        tokensSaved,
        compressChars,
        compressCount,
        costUsd,
        ctxLine: pipeline.consequence.ctxLine,
        ctxActions,
        toolsExpanded: ht.tools_expanded || [],
        pipelineItems: pipeline.items,
    });

    const savingsSummary = traceSummaryStat({
        totalTools,
        toolsRemoved,
        tokensSaved,
        compressChars,
        compressCount,
    });

    return `<div class="trace-row" id="trace-${i}" onclick="toggleTraceReq(${i})">
    <div class="trace-summary">
      <div class="trace-ts">${ts}</div>
      <div class="trace-profile-chip">${profileLabel}</div>
      ${modeChip}
      ${autoChip}
      ${abBadges}
      ${enrichedBadge}
      <div class="trace-stat">${savingsSummary}</div>
      ${renderTraceCostStack(costTrio, true, compressChars)}
      <div class="trace-chevron">▼</div>
    </div>
    <div class="trace-detail">
      ${turnPanel}
    </div>
  </div>`;
}

function fmtTs(ts) {
    const d = new Date(ts);
    const now = new Date();
    const diffMs = now - d;
    const diffH = diffMs / 3600000;
    if (diffH < 1) return `${Math.round(diffMs / 60000)}m ago`;
    if (diffH < 24) return `${Math.round(diffH)}h ago`;
    return (
        d.toLocaleDateString("en-US", {
            month: "short",
            day: "numeric"
        }) +
        " " +
        d.toLocaleTimeString("en-US", {
            hour: "numeric",
            minute: "2-digit"
        })
    );
}

function fmtK(n) {
    return n >= 1000 ? (n / 1000).toFixed(1) + "K" : String(n);
}

function serverDisplayName(s) {
    return s
        .replace(/^mcp__claude_ai_/, "")
        .replace(/__$/, "")
        .replace(/_/g, " ");
}

function traceRow(req, i, compressEnabled) {
    const ts = fmtTs(req.ts);
    const profileLabel = req.profile || "all";
    const autoChip = req.auto_selected ?
        `<span class="trace-auto-chip">auto: ${req.auto_trigger || "matched"}</span>` :
        "";

    const totalTools = req.tools_removed + (req.tools_sent_count || 0);
    const keptTools = req.tools_sent_count || 0;
    const compressChars = req.compress_chars_saved || 0;

    const costTrio = traceCostTrio(req.tokens_saved || 0, 0, false);
    if (req.cost_saved > 0) costTrio.ctxSavings = req.cost_saved;

    const pipeline = buildTracePipelineItems({
        profileLabel,
        pinnedProfile: profileLabel,
        effectiveProfile: profileLabel,
        autoSelected: req.auto_selected,
        autoTrigger: req.auto_trigger,
        toolsRemoved: req.tools_removed,
        toolsKept: keptTools,
        filterMode: "soft",
        abGroup: req.ab_group,
        isHook: false,
        removedServerCount: req.removed_servers.length,
        requestInDesc: `${totalTools} tools from ${req.removed_servers.length + req.kept_servers.length} servers`,
        sentDesc: `${keptTools} tools, ~${fmtK(req.tokens_saved)} tokens saved`,
        injectFired: req.inject_fired,
        coachKind: req.coach_kind,
        behaviorKind: req.behavior_kind,
        budgetFired: req.budget_fired,
        compressEnabled,
        compressCharsSaved: compressChars,
        compressEventCount: compressChars > 0 ? 1 : 0,
    });

    const ctxActions = [];
    if (req.inject_fired) ctxActions.push("Prepended system_prefix.md");
    if (req.coach_kind) ctxActions.push(`Coaching: ${req.coach_kind}`);
    if (req.budget_fired) ctxActions.push("Session cost alert fired");
    if (req.behavior_kind) ctxActions.push(`Behavior guard: ${req.behavior_kind}`);

    const metaParts = [];
    if (req.working_directory) metaParts.push(`<code>${esc(req.working_directory)}</code>`);
    metaParts.push(esc(ts));

    let responseNote = "";
    if (req.mcp_tools_invoked && req.mcp_tools_invoked.length) {
        const names = req.mcp_tools_invoked.map((n) => serverDisplayName(n));
        const unique = [...new Set(names)];
        responseNote = `Claude used ${unique.length} MCP tool${unique.length !== 1 ? "s" : ""}: ${unique.join(", ")}`;
    } else {
        responseNote = "Claude responded (streaming; tool use not captured)";
    }
    ctxActions.push(responseNote);

    const turnPanel = renderTurnPanel({
        prompt: req.human_text_prefix || "",
        meta: metaParts.join(" · "),
        enriched: true,
        totalTools,
        toolsKept: keptTools,
        toolsRemoved: req.tools_removed,
        tokensSaved: req.tokens_saved || 0,
        compressChars,
        compressCount: compressChars > 0 ? 1 : 0,
        costUsd: req.cost_saved > 0 ? req.cost_saved : 0,
        ctxLine: pipeline.consequence.ctxLine,
        ctxActions,
        pipelineItems: pipeline.items,
    });

    const savingsSummary = traceSummaryStat({
        totalTools,
        toolsRemoved: req.tools_removed,
        tokensSaved: req.tokens_saved || 0,
        compressChars,
    });

    return `<div class="trace-row" id="trace-${i}" onclick="toggleTraceReq(${i})">
    <div class="trace-summary">
      <div class="trace-ts">${ts}</div>
      <div class="trace-profile-chip">${profileLabel}</div>
      ${autoChip}
      <div class="trace-stat">${savingsSummary}</div>
      ${renderTraceCostStack(costTrio, true, compressChars)}
      <div class="trace-chevron">▼</div>
    </div>
    <div class="trace-detail">
      ${turnPanel}
    </div>
  </div>`;
}

function toggleTraceReq(i) {
    document.getElementById("trace-" + i).classList.toggle("expanded");
}