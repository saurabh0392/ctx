// ─── Theme toggle ─────────────────────────────────────────
function applyTheme(theme) {
    document.documentElement.setAttribute("data-theme", theme);
    const isDark = theme === "dark";
    document.getElementById("theme-icon").textContent = isDark ? "☀️" : "🌙";
    document.getElementById("theme-label").textContent = isDark ?
        "Light mode" :
        "Dark mode";
}

function toggleTheme() {
    const current = document.documentElement.getAttribute("data-theme") || "dark";
    const next = current === "dark" ? "light" : "dark";
    localStorage.setItem("ctx-theme", next);
    applyTheme(next);
}
(function() {
    const saved = localStorage.getItem("ctx-theme") || "dark";
    applyTheme(saved);
})();