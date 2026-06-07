// ─── Boot ─────────────────────────────────────────────────
let _userProfile = {
    long_session_threshold: 26,
    correction_threshold: 40,
    calibrated: false,
    median_session_turns: 15,
};
fetch("/api/user-profile")
    .then((r) => r.json())
    .then((p) => {
        _userProfile = p;
    })
    .catch(() => {});
initDevModeNav();
loadContext().catch((e) => console.error("loadContext failed:", e));
setInterval(() => {
    const ctxTab = document.getElementById("tab-context");
    if (ctxTab && ctxTab.classList.contains("active")) {
        loadContext().catch(console.error);
    }
}, 15_000);