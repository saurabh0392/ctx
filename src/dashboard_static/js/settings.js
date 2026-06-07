function fmtBytes(n) {
    if (n >= 1e9) return (n / 1e9).toFixed(2) + " GB";
    if (n >= 1e6) return (n / 1e6).toFixed(2) + " MB";
    if (n >= 1e3) return (n / 1e3).toFixed(1) + " KB";
    return n + " B";
}

function bindSettingGateRows() {
    document.querySelectorAll(".set-gate-row").forEach((row) => {
        const input = row.querySelector('input[type="checkbox"]');
        if (!input) return;
        const sync = () => row.classList.toggle("is-on", input.checked);
        input.addEventListener("change", sync);
        sync();
    });
    const compress = document.getElementById("set-compress");
    if (compress) compress.addEventListener("change", syncCompressSgrState);
    syncCompressSgrState();
}

function syncCompressSgrState() {
    const compress = document.getElementById("set-compress");
    const sgrRow = document.getElementById("set-compress-sgr-row");
    const sgr = document.getElementById("set-compress-sgr");
    if (!compress || !sgrRow) return;
    sgrRow.classList.toggle("is-disabled", !compress.checked);
    if (!compress.checked && sgr) {
        sgr.checked = false;
        sgrRow.classList.remove("is-on");
    }
}

function countGatesOn(s) {
    let n = 0;
    if (s.auto_profile_enabled) n++;
    if (s.inject_enabled !== false) n++;
    if (s.coaching_enabled !== false) n++;
    if (s.adaptive_prefix_enabled !== false) n++;
    if (s.compress_enabled !== false) n++;
    if (s.compress_sgr_enabled) n++;
    return n;
}

function renderSettingsHero(s, activeProfileName) {
    const body = document.getElementById("set-hero-body");
    const pills = document.getElementById("set-hero-pills");
    if (!body || !pills) return;
    const prof = activeProfileName || s.active_profile || "all";
    const gates = countGatesOn(s);
    const compressOn = s.compress_enabled !== false;
    const ab = s.ab_test || {};
    const expActive =
        ab.profile_pct < 100 ||
        ab.inject_pct < 100 ||
        ab.adaptive_pct < 100 ||
        ab.coaching_pct < 100 ||
        (ab.compress_pct != null && ab.compress_pct < 100) ||
        (ab.compress_sgr_pct != null && ab.compress_sgr_pct < 100);
    body.innerHTML =
        `Profile <strong>${esc(prof)}</strong>. ` +
        `<strong>${gates}</strong> gate${gates === 1 ? "" : "s"} on. ` +
        (compressOn ?
            "Output compression armed for large tool results." :
            "Output compression off.") +
        (expActive ?
            " An experiment is splitting traffic on some prompts." :
            "");
    const pillData = [
        ["Profile", prof],
        ["Gates on", String(gates)],
        ["Compress", compressOn ? "on" : "off"],
        ["SGR", s.compress_sgr_enabled ? "on" : "off"],
        ["Experiment", expActive ? "active" : "off"],
        ["Prompt storage", s.store_prompt_text ? "on" : "off"],
    ];
    pills.innerHTML = pillData
        .map(
            ([label, val]) =>
            `<span class="narrative-pill">${esc(label)} <span>${esc(val)}</span></span>`,
        )
        .join("");
}

function renderSettingsDataGrid(s, rc, files) {
    const box = document.getElementById("set-data-body");
    if (!box) return;
    const fileList =
        files ||
        `<li>(empty)</li>`;
    box.innerHTML = `
    <div class="set-data-stat">
      <div class="set-data-stat-label">ctx home</div>
      <div class="set-data-stat-val"><code>${esc(s.ctx_home)}</code></div>
    </div>
    <div class="set-data-stat">
      <div class="set-data-stat-label">Database</div>
      <div class="set-data-stat-val">${fmtBytes(s.db_size_bytes || 0)}</div>
      <div class="set-data-stat-sub">Last ingest ${esc(s.last_ingest_at || "never")}</div>
    </div>
    <div class="set-data-stat">
      <div class="set-data-stat-label">Indexed rows</div>
      <div class="set-data-stat-val">${(rc.sessions || 0).toLocaleString()} sessions</div>
      <div class="set-data-stat-sub">${(rc.turns || 0).toLocaleString()} turns, ${(rc.tool_invocations || 0).toLocaleString()} tools, ${(rc.requests || 0).toLocaleString()} requests</div>
    </div>
    <div class="set-data-stat">
      <div class="set-data-stat-label">Privacy</div>
      <div class="set-data-stat-val">${s.store_prompt_text ? "Prompts stored" : "Prompts not stored"}</div>
      <div class="set-data-stat-sub">${s.embeddings_enabled !== false ? "Embeddings on" : "Embeddings off"}</div>
    </div>
    <div class="set-data-stat set-data-wide">
      <div class="set-data-stat-label">Files under ctx home</div>
      <ul class="set-data-files">${fileList}</ul>
      <div class="set-data-stat-sub" style="margin-top:10px">Reads <code>~/.claude/projects/**/*.jsonl</code>. No telemetry. Chart.js loads from a CDN for charts only.</div>
    </div>`;
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
        const setCompress = document.getElementById("set-compress");
        if (setCompress) setCompress.checked = s.compress_enabled !== false;
        const setCompressSgr = document.getElementById("set-compress-sgr");
        if (setCompressSgr) setCompressSgr.checked = !!s.compress_sgr_enabled;
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
        setPct("ab-compress-pct", ab.compress_pct != null ? ab.compress_pct : 100);
        setPct("ab-compress-sgr-pct", ab.compress_sgr_pct != null ? ab.compress_sgr_pct : 100);
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
        const activeProf = (profs || []).find((p) => p.active);
        sel.innerHTML = (profs || [])
            .map(
                (p) =>
                `<option value="${esc(p.slug)}"${p.active ? " selected" : ""}>${esc(p.display || p.slug)}</option>`,
            )
            .join("");
        renderSettingsHero(
            s,
            activeProf ? activeProf.display || activeProf.slug : s.active_profile,
        );
        const rc = s.row_counts || {};
        const files = (s.files_under_ctx || [])
            .map(
                (f) =>
                `<li><code>${esc(f.name)}</code> ${fmtBytes(f.size_bytes)}</li>`,
            )
            .join("");
        renderSettingsDataGrid(s, rc, files || "<li>(empty)</li>");
        bindSettingGateRows();
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
        compress_enabled: document.getElementById("set-compress").checked,
        compress_sgr_enabled: document.getElementById("set-compress-sgr")?.checked ?? false,
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
        compress_pct: v("ab-compress-pct"),
        compress_sgr_pct: v("ab-compress-sgr-pct"),
    };
}

function syncAbSliderLabels() {
    const pairs = [
        ["ab-profile-pct", "ab-profile-val"],
        ["ab-inject-pct", "ab-inject-val"],
        ["ab-adaptive-pct", "ab-adaptive-val"],
        ["ab-coaching-pct", "ab-coaching-val"],
        ["ab-compress-pct", "ab-compress-val"],
        ["ab-compress-sgr-pct", "ab-compress-sgr-val"],
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
        ab.coaching_pct < 100 ||
        ab.compress_pct < 100 ||
        ab.compress_sgr_pct < 100;
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
        "ab-compress-pct",
        "ab-compress-sgr-pct",
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
        "ab-compress-pct",
        "ab-compress-sgr-pct",
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

