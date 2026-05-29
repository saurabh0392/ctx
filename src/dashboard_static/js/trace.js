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
    const [requests, hookTraces] = await Promise.all([
        fetch(appendSince("/api/requests?limit=100")).then((r) => r.json()),
        fetch(appendSince("/api/hook-traces?limit=100"))
        .then((r) => r.json())
        .catch(() => []),
    ]);
    const el = document.getElementById("trace-list");

    if (!requests.length && !hookTraces.length) {
        el.innerHTML = `<div class="card" style="padding:20px;margin-bottom:16px;border-color:rgba(147,192,67,.25)">
      <div class="section-head" style="margin-bottom:8px">No trace events</div>
      <div class="section-sub" style="margin-bottom:0">No trace events recorded yet. Use Claude Code with ctx hooks enabled. Each turn records a trace row automatically.</div>
    </div>`;
        return;
    }

    const todayStr = new Date().toISOString().slice(0, 10);
    const todayReqs = requests.filter((r) => r.ts.slice(0, 10) === todayStr);
    const todayHookTraces = hookTraces.filter(
        (h) => h.ts.slice(0, 10) === todayStr,
    );
    const todayTokens = todayReqs.reduce((s, r) => s + r.tokens_saved, 0);
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
    if (todayTokens > 0) bannerParts.push(`${fmtK(todayTokens)} tokens stripped`);

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

    el.innerHTML =
        banner +
        unified
        .map((item, i) => {
            if (item.type === "request") return traceRow(item.data, i);
            return hookTraceRow(item.data, i);
        })
        .join("");
}

function renderAbBadges(abGroup) {
    if (!abGroup) return "";
    return abGroup
        .split(/\s+/)
        .filter(Boolean)
        .map((p) => {
            const m = p.match(/^([PIAC]):([TC])$/);
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

function traceCostTrio(tokensSaved, costUsd, enriched) {
    const ctxSavings = (tokensSaved / 1_000_000) * TRACE_CTX_SAVINGS_RATE;
    const afterCtx = enriched && costUsd > 0 ? costUsd : null;
    const estimatedTotal = afterCtx != null ? afterCtx + ctxSavings : null;
    return { estimatedTotal, afterCtx, ctxSavings };
}

function renderTraceCostStack(trio, compact) {
    const est = trio.estimatedTotal != null ? fmtCost(trio.estimatedTotal) : "—";
    const after = trio.afterCtx != null ? fmtCost(trio.afterCtx) : "—";
    const save = fmtCost(trio.ctxSavings);
    if (compact) {
        return `<div class="trace-cost-stack trace-cost-stack-compact">
      <div class="trace-cost-row"><span class="trace-cost-label">Est. total</span><span class="trace-cost-val">${est}</span></div>
      <div class="trace-cost-row"><span class="trace-cost-label">After ctx</span><span class="trace-cost-val trace-cost-after">${after}</span></div>
      <div class="trace-cost-row"><span class="trace-cost-label">Savings</span><span class="trace-cost-val trace-cost-save">${save}</span></div>
    </div>`;
    }
    return `<div class="trace-cost-stack">
    <div class="trace-cost-row"><span class="trace-cost-label">Estimated total cost</span><span class="trace-cost-val">${est}</span></div>
    <div class="trace-cost-row"><span class="trace-cost-label">Total cost (after ctx)</span><span class="trace-cost-val trace-cost-after">${after}</span></div>
    <div class="trace-cost-row"><span class="trace-cost-label">Savings (ctx)</span><span class="trace-cost-val trace-cost-save">${save}</span></div>
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
    if (opts.compressCharsSaved != null) {
        items.push({
            type: "node",
            id: "compress",
            name: "Bash Compress",
            fired: opts.compressCharsSaved > 0,
            desc: opts.compressCharsSaved > 0 ?
                `${fmtK(opts.compressCharsSaved)} chars compressed` :
                UI_EMPTY,
            accent: ga.compress?.accent,
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
    return { items, consequence };
}

function hookTraceRow(ht, i) {
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
    const totalTools = toolsKept + toolsRemoved;
    const pctCut =
        totalTools > 0 ? Math.round((toolsRemoved / totalTools) * 100) : 0;
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
    });

    const ctxItems = [];
    ctxItems.push({
        line: pipeline.consequence.ctxLine,
        detail: toolsRemoved > 0 ?
            `${esc(filterMode)} filter: kept ${toolsKept} tools, stripped ${toolsRemoved} (${pctCut}% cut).` :
            "All MCP tools allowed for this prompt.",
    });
    if (ht.inject_fired) {
        ctxItems.push({
            line: "Prepended <strong>system_prefix.md</strong>.",
            detail: "Static prefix from ~/.ctx/system_prefix.md.",
        });
    }
    if (ht.adaptive_fired) {
        ctxItems.push({
            line: "Appended <strong>adaptive_prefix.md</strong>.",
            detail: "Behavioral profile from indexed sessions.",
        });
    }
    if (ht.coach_kind) {
        ctxItems.push({
            line: `Coaching: <strong>${esc(ht.coach_kind)}</strong>.`,
            detail: "Suggestion injected into additionalContext.",
        });
    }
    if (ht.budget_fired)
        ctxItems.push({
            line: "Session cost alert fired.",
            detail: ""
        });
    const ctxBullets = renderCtxBulletList(ctxItems);

    const promptPreview = ht.human_text_prefix ?
        `<div class="trace-prompt-preview" onclick="event.stopPropagation();this.classList.toggle('expanded')">${esc(ht.human_text_prefix)}</div>` :
        '<div class="trace-prompt-preview" style="color:var(--t4)">Prompt text available after next ingest.</div>';
    const abBadges = renderAbBadges(ht.ab_group);

    const savingsBar =
        totalTools > 0 ?
        `<div class="trace-token-impact">
      <div class="trace-token-impact-title">Token impact</div>
      <div class="trace-token-bar">
        <div class="trace-token-bar-removed" style="width:${((toolsRemoved / totalTools) * 100).toFixed(1)}%"></div>
        <div class="trace-token-bar-kept" style="width:${((toolsKept / totalTools) * 100).toFixed(1)}%"></div>
      </div>
      <div class="trace-token-label">
        <strong>${totalTools}</strong> tools to <strong>${toolsKept}</strong> tools (${pctCut}% cut)<br>
        <strong>${fmtK(tokensSaved)}</strong> tokens stripped, saving <strong>${fmtCost(costTrio.ctxSavings)}</strong>/turn
      </div>
    </div>` :
        "";

    let costLine = "";
    if (ht.enriched) {
        costLine = `<div class="trace-token-impact">
      <div class="trace-token-impact-title">Turn cost</div>
      ${renderTraceCostStack(costTrio, false)}
      <div class="trace-token-label" style="margin-top:10px">
        ${model ? `<strong>${esc(model)}</strong> &middot; ` : ""}
        <strong>${fmtK(inputTok)}</strong> input &middot;
        <strong>${fmtK(outputTok)}</strong> output &middot;
        <strong>${fmtK(cacheRead)}</strong> cache read
      </div>
    </div>`;
    } else {
        costLine = `<div style="font-size:11px;color:var(--t4);margin-top:8px">Turn cost data will appear after the next ingest cycle.</div>`;
    }

    const storyPanel = `<div class="trace-story">
    <div class="trace-story-eyebrow">Your interaction</div>
    <div class="trace-story-context">
      ${ht.working_directory ? `<code>${esc(ht.working_directory)}</code><br>` : ""}
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

    const flowPanel = renderTraceFlowPanel(pipeline.items);

    const savingsSummary =
        toolsRemoved > 0 ?
        `-<strong>${toolsRemoved}</strong> of ${totalTools} tools &middot; <strong>${fmtK(tokensSaved)}</strong> tok &middot; ${pctCut}% cut` :
        `${totalTools} tools (no filter)`;

    return `<div class="trace-row" id="trace-${i}" onclick="toggleTraceReq(${i})">
    <div class="trace-summary">
      <div class="trace-ts">${ts}</div>
      <div class="trace-profile-chip">${profileLabel}</div>
      ${modeChip}
      ${autoChip}
      ${abBadges}
      ${enrichedBadge}
      <div class="trace-stat">${savingsSummary}</div>
      ${renderTraceCostStack(costTrio, true)}
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

function traceRow(req, i) {
    const ts = fmtTs(req.ts);
    const profileLabel = req.profile || "all";
    const autoChip = req.auto_selected ?
        `<span class="trace-auto-chip">auto: ${req.auto_trigger || "matched"}</span>` :
        "";

    const totalTools = req.tools_removed + (req.tools_sent_count || 0);
    const keptTools = req.tools_sent_count || 0;
    const pctCut =
        totalTools > 0 ? Math.round((req.tools_removed / totalTools) * 100) : 0;

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
        compressCharsSaved: req.compress_chars_saved,
    });

    const ctxItems = [];
    ctxItems.push({
        line: pipeline.consequence.ctxLine,
        detail: req.kept_servers.length ?
            `Kept: ${req.kept_servers.map((s) => esc(serverDisplayName(s))).join(", ")}. Removed: ${req.removed_servers.map((s) => esc(serverDisplayName(s))).join(", ") || "none"}.` :
            (req.tools_removed > 0 ? `Stripped ${req.tools_removed} tools.` : "All MCP tools allowed."),
    });
    if (req.inject_fired)
        ctxItems.push({
            line: "Prepended <strong>system_prefix.md</strong>.",
            detail: "",
        });
    if (req.coach_kind)
        ctxItems.push({
            line: `Coaching: <strong>${esc(req.coach_kind)}</strong>.`,
            detail: "",
        });
    if (req.budget_fired)
        ctxItems.push({
            line: "Session cost alert fired.",
            detail: ""
        });
    if (req.behavior_kind)
        ctxItems.push({
            line: `Behavior guard: <strong>${esc(req.behavior_kind)}</strong>.`,
            detail: "",
        });
    if (req.compress_chars_saved > 0)
        ctxItems.push({
            line: `Compressed <strong>${fmtK(req.compress_chars_saved)}</strong> chars of bash output.`,
            detail: "",
        });
    const ctxBullets = renderCtxBulletList(ctxItems);

    let responseHtml = "";
    if (req.mcp_tools_invoked && req.mcp_tools_invoked.length) {
        const names = req.mcp_tools_invoked.map((n) => serverDisplayName(n));
        const unique = [...new Set(names)];
        responseHtml = `Claude used <strong>${unique.length}</strong> MCP tool${unique.length !== 1 ? "s" : ""}: ${unique.map((n) => esc(n)).join(", ")}`;
    } else {
        responseHtml = "Claude responded (streaming; tool use not captured)";
    }

    const barTotal = Math.max(totalTools, 1);
    const removedPct = ((req.tools_removed / barTotal) * 100).toFixed(1);
    const keptPct = ((keptTools / barTotal) * 100).toFixed(1);

    const storyPanel = `<div class="trace-story">
    <div class="trace-story-eyebrow">Your interaction</div>
    <div class="trace-story-context">
      ${req.working_directory ? `<code>${esc(req.working_directory)}</code><br>` : ""}
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
        <strong>${totalTools}</strong> tools to <strong>${keptTools}</strong> tools (${pctCut}% cut)<br>
        <strong>${fmtK(req.tokens_saved)}</strong> tokens stripped, saving <strong>${fmtCost(costTrio.ctxSavings)}</strong>
      </div>
    </div>
    ${renderTraceCostStack(costTrio, false)}
  </div>`;

    const flowPanel = renderTraceFlowPanel(pipeline.items);

    return `<div class="trace-row" id="trace-${i}" onclick="toggleTraceReq(${i})">
    <div class="trace-summary">
      <div class="trace-ts">${ts}</div>
      <div class="trace-profile-chip">${profileLabel}</div>
      ${autoChip}
      <div class="trace-stat">-<strong>${req.tools_removed}</strong> of ${totalTools} tools &middot; <strong>${fmtK(req.tokens_saved)}</strong> tok &middot; ${pctCut}% cut</div>
      ${renderTraceCostStack(costTrio, true)}
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
    document.getElementById("trace-" + i).classList.toggle("expanded");
}