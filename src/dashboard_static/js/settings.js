function fmtBytes(n) {
    if (n >= 1e9) return (n / 1e9).toFixed(2) + " GB";
    if (n >= 1e6) return (n / 1e6).toFixed(2) + " MB";
    if (n >= 1e3) return (n / 1e3).toFixed(1) + " KB";
    return n + " B";
}

async function loadSettingsTab() {
    const box = document.getElementById("set-data-body");
    if (box) box.textContent = "Loading…";
    try {
        const s = await fetch("/api/settings").then((r) => r.json());
        document.getElementById("set-budget").value =
            s.monthly_budget_usd != null ? s.monthly_budget_usd : "";
        document.getElementById("set-actual").value =
            s.monthly_actual_spend_usd != null ? s.monthly_actual_spend_usd : "";
        document.getElementById("set-dash-port").textContent =
            s.dashboard_port != null ? String(s.dashboard_port) : "8789 (default)";
        document.getElementById("set-proxy-port").textContent =
            s.proxy_port != null ? String(s.proxy_port) : "8788 (default)";
        document.getElementById("set-store-prompt").checked = !!s.store_prompt_text;
        document.getElementById("set-embed").checked = !!s.embeddings_enabled;
        document.getElementById("set-auto-prof").checked = !!s.auto_profile_enabled;
        document.getElementById("set-inject").checked = !!s.inject_enabled;
        const setCoach = document.getElementById("set-coaching");
        if (setCoach) setCoach.checked = s.coaching_enabled !== false;
        const setAdapt = document.getElementById("set-adaptive");
        if (setAdapt) setAdapt.checked = s.adaptive_prefix_enabled !== false;
        const setAdaptMax = document.getElementById("set-adaptive-max");
        if (setAdaptMax)
            setAdaptMax.value =
            s.adaptive_prefix_max_chars != null ?
            String(s.adaptive_prefix_max_chars) :
            "";
        const adaptPrev = document.getElementById("set-adaptive-preview");
        if (adaptPrev) adaptPrev.value = s.adaptive_prefix_preview || "";
        const adaptMeta = document.getElementById("set-adaptive-meta");
        if (adaptMeta) {
            const c =
                typeof s.adaptive_prefix_char_count === "number" ?
                s.adaptive_prefix_char_count :
                0;
            const b =
                typeof s.adaptive_prefix_char_budget === "number" ?
                s.adaptive_prefix_char_budget :
                2000;
            adaptMeta.textContent =
                "Adaptive prefix: " +
                c.toLocaleString() +
                " / " +
                b.toLocaleString() +
                " chars";
        }
        const sinceDisp = document.getElementById("set-ctx-since-display");
        if (sinceDisp) sinceDisp.textContent = s.ctx_active_since || "(none)";
        const ab = s.ab_test || {};
        const setPct = (id, v) => {
            const el = document.getElementById(id);
            if (el) el.value = String(v ?? 100);
        };
        setPct("ab-profile-pct", ab.profile_pct);
        setPct("ab-inject-pct", ab.inject_pct);
        setPct("ab-adaptive-pct", ab.adaptive_pct);
        setPct("ab-coaching-pct", ab.coaching_pct);
        syncAbSliderLabels();
        const devCk = document.getElementById("set-dev-mode");
        if (devCk) devCk.checked = !!s.dev_mode;
        const modeSel = document.getElementById("set-mode");
        const modesEmpty = document.getElementById("set-modes-empty");
        if (modeSel) {
            const modes = s.modes || [];
            if (modes.length) {
                modeSel.innerHTML =
                    '<option value="">(none)</option>' +
                    modes
                    .map(
                        (m) =>
                        `<option value="${esc(m.name)}"${s.active_mode === m.name ? " selected" : ""}>${esc(m.name)}. ${esc(m.profile)}</option>`,
                    )
                    .join("");
                if (modesEmpty) modesEmpty.style.display = "none";
                modeSel.disabled = false;
            } else {
                modeSel.innerHTML = "";
                if (modesEmpty) modesEmpty.style.display = "block";
                modeSel.disabled = true;
            }
        }
        const autoApply = document.getElementById("set-auto-apply");
        if (autoApply) autoApply.checked = !!s.auto_apply_recommendations;
        renderTuningRecommendations(s.tuning_recommendations);
        document.getElementById("set-prefix").value = s.system_prefix_preview || "";
        const profs = await fetch("/api/profiles").then((r) => r.json());
        const sel = document.getElementById("set-profile");
        sel.innerHTML = (profs || [])
            .map(
                (p) =>
                `<option value="${esc(p.slug)}"${p.active ? " selected" : ""}>${esc(p.display || p.slug)}</option>`,
            )
            .join("");
        const rc = s.row_counts || {};
        const files = (s.files_under_ctx || [])
            .map(
                (f) =>
                `<li><code>${esc(f.name)}</code>. ${fmtBytes(f.size_bytes)}</li>`,
            )
            .join("");
        box.innerHTML = `
      <p><strong>ctx home:</strong> <code>${esc(s.ctx_home)}</code></p>
      <p><strong>Database:</strong> ${fmtBytes(s.db_size_bytes || 0)}</p>
      <p><strong>Last ingest:</strong> ${esc(s.last_ingest_at || "never")}</p>
      <p><strong>Rows:</strong> sessions ${rc.sessions || 0}, turns ${rc.turns || 0}, tools ${rc.tool_invocations || 0}, embeddings ${rc.session_embeddings || 0}, requests ${rc.requests || 0}</p>
      <p><strong>Prompt text stored:</strong> ${s.store_prompt_text ? "yes" : "no"}</p>
      <p><strong>Reads:</strong> <code>~/.claude/projects/**/*.jsonl</code> (Claude Code logs)</p>
      <p><strong>Files under ctx home:</strong></p><ul style="margin:6px 0;padding-left:20px">${files || "<li>(empty)</li>"}</ul>
      <p><strong>Network:</strong> no telemetry from ctx. Chart.js loads from a CDN for charts only.</p>`;
    } catch (e) {
        if (box) box.textContent = "Could not load settings: " + e;
    }
}
async function saveSettingsGeneral() {
    const body = {};
    const b = document.getElementById("set-budget").value;
    if (b !== "" && !isNaN(parseFloat(b)))
        body.monthly_budget_usd = parseFloat(b);
    const a = document.getElementById("set-actual").value;
    if (a !== "" && !isNaN(parseFloat(a)))
        body.monthly_actual_spend_usd = parseFloat(a);
    await fetch("/api/settings", {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify(body),
    });
    await loadSettingsTab();
    alert("Saved.");
}
async function saveSettingsPrivacy() {
    const body = {
        store_prompt_text: document.getElementById("set-store-prompt").checked,
        embeddings_enabled: document.getElementById("set-embed").checked,
    };
    await fetch("/api/settings", {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify(body),
    });
    await loadSettingsTab();
    alert(
        "Privacy settings saved. Run ctx ingest again to refresh rows if you turned prompt storage off.",
    );
}
async function saveSettingsFiltering() {
    const body = {
        active_profile: document.getElementById("set-profile").value,
        auto_profile_enabled: document.getElementById("set-auto-prof").checked,
        inject_enabled: document.getElementById("set-inject").checked,
        coaching_enabled: document.getElementById("set-coaching").checked,
        adaptive_prefix_enabled: document.getElementById("set-adaptive").checked,
        adaptive_prefix_max_chars: parseInt(document.getElementById("set-adaptive-max").value, 10) || 0,
        system_prefix: document.getElementById("set-prefix").value,
    };
    const r = await fetch("/api/settings", {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify(body),
    });
    if (!r.ok) {
        const t = await r.text();
        alert("Save failed: " + t);
        return;
    }
    await loadSettingsTab();
    alert("Filtering saved.");
}
async function purgePrompts() {
    if (!confirm("Clear stored prompt text and embeddings from SQLite?")) return;
    await fetch("/api/settings/purge-prompts", {
        method: "POST"
    });
    await loadSettingsTab();
    alert("Purged.");
}
async function deleteAllData() {
    if (
        !confirm(
            "Delete ALL indexed sessions, turns, embeddings, and requests from ctx.db?",
        )
    )
        return;
    await fetch("/api/settings/delete-data", {
        method: "POST"
    });
    await loadSettingsTab();
    alert("Deleted.");
}

function exportDb() {
    window.open("/api/settings/export", "_blank");
}

async function regenerateAdaptivePrefix() {
    const r = await fetch("/api/settings/refresh-adaptive-prefix", {
        method: "POST",
    }).catch(() => null);
    if (!r || !r.ok) {
        alert("Regenerate failed");
        return;
    }
    await loadSettingsTab();
    alert("Adaptive prefix file updated from SQLite.");
}

async function resetDashboardWatermark() {
    if (
        !confirm(
            "Clear the install watermark? Charts will include all indexed rows until the next hook or filtered request stamps again.",
        )
    )
        return;
    const r = await fetch("/api/settings/reset-watermark", {
        method: "POST",
    }).catch(() => null);
    if (!r || !r.ok) {
        alert("Reset failed");
        return;
    }
    await loadSettingsTab();
    applyDashboardSinceReload();
    alert("Watermark cleared.");
}

function readAbTestFromSliders() {
    const v = (id) => parseInt(document.getElementById(id).value, 10) || 0;
    return {
        profile_pct: v("ab-profile-pct"),
        inject_pct: v("ab-inject-pct"),
        adaptive_pct: v("ab-adaptive-pct"),
        coaching_pct: v("ab-coaching-pct"),
    };
}

function syncAbSliderLabels() {
    const pairs = [
        ["ab-profile-pct", "ab-profile-val"],
        ["ab-inject-pct", "ab-inject-val"],
        ["ab-adaptive-pct", "ab-adaptive-val"],
        ["ab-coaching-pct", "ab-coaching-val"],
    ];
    const ab = readAbTestFromSliders();
    pairs.forEach(([sid, lid]) => {
        const el = document.getElementById(lid);
        if (el)
            el.textContent = (document.getElementById(sid).value || "100") + "%";
    });
    const banner = document.getElementById("ab-experiment-banner");
    if (!banner) return;
    const active =
        ab.profile_pct < 100 ||
        ab.inject_pct < 100 ||
        ab.adaptive_pct < 100 ||
        ab.coaching_pct < 100;
    if (active) {
        banner.style.display = "block";
        banner.textContent =
            "Experiment active. Some requests skip gates below 100%. Check the Experiment tab for results. Stop when you have enough data.";
    } else {
        banner.style.display = "none";
    }
}

async function saveAbExperiment() {
    const body = {
        ab_test: readAbTestFromSliders()
    };
    const devCk = document.getElementById("set-dev-mode");
    if (devCk) body.dev_mode = devCk.checked;
    await fetch("/api/settings", {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify(body),
    });
    await loadSettingsTab();
    initDevModeNav();
    alert("Experiment settings saved.");
}

async function startAbExperiment5050() {
    [
        "ab-profile-pct",
        "ab-inject-pct",
        "ab-adaptive-pct",
        "ab-coaching-pct",
    ].forEach((id) => {
        const el = document.getElementById(id);
        if (el) el.value = "50";
    });
    syncAbSliderLabels();
    await saveAbExperiment();
}

async function stopAbExperiment() {
    [
        "ab-profile-pct",
        "ab-inject-pct",
        "ab-adaptive-pct",
        "ab-coaching-pct",
    ].forEach((id) => {
        const el = document.getElementById(id);
        if (el) el.value = "100";
    });
    syncAbSliderLabels();
    await saveAbExperiment();
}

function initDevModeNav() {
    const params = new URLSearchParams(window.location.search);
    if (params.get("dev") === "1") localStorage.setItem("ctx_dev", "1");
    let show = localStorage.getItem("ctx_dev") === "1";
    const devCk = document.getElementById("set-dev-mode");
    if (devCk && devCk.checked) show = true;
    fetch("/api/settings")
        .then((r) => r.json())
        .then((s) => {
            if (s.dev_mode) show = true;
            const sec = document.getElementById("nav-dev-section");
            const nav = document.getElementById("nav-dev-experiment");
            const navSim = document.getElementById("nav-dev-simulate");
            if (sec) sec.style.display = show ? "block" : "none";
            if (nav) nav.style.display = show ? "flex" : "none";
            if (navSim) navSim.style.display = show ? "flex" : "none";
        })
        .catch(() => {});
}


async function saveSettingsMode() {
    const mode = document.getElementById("set-mode")?.value;
    if (!mode) {
        alert("Select a mode first.");
        return;
    }
    const r = await fetch("/api/settings/mode", {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify({
            mode
        }),
    });
    if (!r.ok) {
        alert("Mode switch failed: " + (await r.text()));
        return;
    }
    await loadSettingsTab();
    alert("Mode applied.");
}

function renderTuningRecommendations(results) {
    const card = document.getElementById("set-tuning-card");
    const body = document.getElementById("set-tuning-body");
    if (!card || !body) return;
    if (!results || !results.features || !results.features.length) {
        card.style.display = "none";
        return;
    }
    card.style.display = "block";
    body.innerHTML = results.features
        .map((f) => {
            const cls = f.verdict === "beneficial" ? "insight-card" : "section-sub";
            return `<div class="${cls}" style="margin-bottom:10px"><div style="font-size:11px;text-transform:uppercase;color:var(--t3)">${esc(f.verdict)}, ${esc(f.feature)}</div><div style="font-size:13px;color:var(--t2);margin-top:4px">${esc(f.message)}</div></div>`;
        })
        .join("");
    if (results.auto_applied_log && results.auto_applied_log.length) {
        body.innerHTML +=
            '<p class="section-sub" style="margin-top:8px"><strong>Auto-applied:</strong> ' +
            results.auto_applied_log.map(esc).join("; ") +
            "</p>";
    }
}

async function saveAutoApplyTuning() {
    const body = {
        auto_apply_recommendations: document.getElementById("set-auto-apply").checked,
    };
    await fetch("/api/settings", {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify(body),
    });
    alert("Saved auto-apply preference.");
}

async function applyTuningRecommendations() {
    alert("Run: ctx experiment apply");
}

