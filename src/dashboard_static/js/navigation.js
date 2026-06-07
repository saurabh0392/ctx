// ─── Tab navigation ───────────────────────────────────────
function showTab(id, el) {
    document
        .querySelectorAll(".tab-panel")
        .forEach((p) => p.classList.remove("active"));
    document
        .querySelectorAll(".nav-item")
        .forEach((n) => n.classList.remove("active"));
    document.getElementById("tab-" + id).classList.add("active");
    el.classList.add("active");
    if (id === "context") loadContext();
    if (id === "proof") loadProof();
    if (id === "profiles") loadProfiles();
    if (id === "trace" && !window._traceLoaded) {
        window._traceLoaded = true;
        loadTrace();
    }
    if (id === "pipeline") loadPipeline();
    if (id === "settings") loadSettingsTab();
    if (id === "experiment") loadExperimentTab();
    if (id === "simulate") loadSimulateTab();
}
