// ─── Real-time dashboard updates (SSE) ───────────────────
let _traceRefreshTimer = null;

function scheduleTraceRefresh() {
    if (!window._traceLoaded) return;
    if (_traceRefreshTimer) clearTimeout(_traceRefreshTimer);
    _traceRefreshTimer = setTimeout(() => {
        _traceRefreshTimer = null;
        loadTrace().catch(console.error);
    }, 150);
}

function handleDashboardPush(ev) {
    if (!ev || !ev.kind) return;
    loadSavings().catch(console.error);
    scheduleTraceRefresh();
    if (window._psLoaded) loadPromptStats().catch(console.error);
    const pipeline = document.getElementById("tab-pipeline");
    if (pipeline && pipeline.classList.contains("active")) loadPipeline().catch(console.error);
    const profiles = document.getElementById("tab-profiles");
    if (profiles && profiles.classList.contains("active")) loadProfiles().catch(console.error);
    const experiment = document.getElementById("tab-experiment");
    if (experiment && experiment.classList.contains("active")) loadExperimentTab().catch(console.error);
}

function connectDashboardRealtime() {
    if (window._ctxEventSource) return;
    if (typeof EventSource === "undefined") return;
    const es = new EventSource("/api/events/stream");
    window._ctxEventSource = es;
    es.addEventListener("dashboard", (e) => {
        try {
            handleDashboardPush(JSON.parse(e.data));
        } catch (_) {}
    });
    es.addEventListener("connected", () => {
        const sock = document.getElementById("socket-status");
        if (sock) sock.textContent = "Event stream: live";
    });
    es.onerror = () => {
        const sock = document.getElementById("socket-status");
        if (sock) sock.textContent = "Event stream: reconnecting…";
    };
}
connectDashboardRealtime();
