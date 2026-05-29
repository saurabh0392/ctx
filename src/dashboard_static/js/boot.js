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
loadSavings().catch((e) => {
    console.error("loadSavings failed:", e);
    const kicker = document.getElementById("story-kicker");
    if (kicker) kicker.textContent = "Could not load savings data.";
});
setInterval(() => loadSavings().catch(console.error), 30_000);