// ─── Settings + onboarding wizard ─────────────────────────
async function wizPrepareFromServer() {
    try {
        const s = await fetch("/api/settings").then((r) => r.json());
        const homeEl = document.getElementById("wiz-ctx-home");
        if (homeEl) homeEl.textContent = s.ctx_home || "~/.ctx";
        const sp = document.getElementById("wiz-store-prompt");
        if (sp) sp.checked = !!s.store_prompt_text;
        const em = document.getElementById("wiz-embed");
        if (em) em.checked = !!s.embeddings_enabled;
        const wb = document.getElementById("wiz-budget");
        if (wb) wb.value = s.monthly_budget_usd != null ? s.monthly_budget_usd : "";
        const wa = document.getElementById("wiz-actual");
        if (wa)
            wa.value =
            s.monthly_actual_spend_usd != null ? s.monthly_actual_spend_usd : "";
        const inj = document.getElementById("wiz-inject");
        if (inj) inj.checked = !!s.inject_enabled;
        const coach = document.getElementById("wiz-coaching");
        if (coach) coach.checked = s.coaching_enabled !== false;
        const ta = document.getElementById("wiz-prefix");
        if (ta) ta.value = s.system_prefix_preview || "";
        const profs = await fetch("/api/profiles").then((r) => r.json());
        const sel = document.getElementById("wiz-profile");
        if (sel)
            sel.innerHTML = (profs || [])
            .map(
                (p) =>
                `<option value="${esc(p.slug)}"${p.active ? " selected" : ""}>${esc(p.display || p.slug)}</option>`,
            )
            .join("");
    } catch (e) {
        console.warn(e);
    }
}

function wizShowStep(n) {
    for (let i = 1; i <= 5; i++) {
        const e = document.getElementById("wiz-step-" + i);
        if (e) e.style.display = i === n ? "block" : "none";
    }
}

function wizNext(n) {
    wizShowStep(n);
}

function dismissOnboardingWizard() {
    localStorage.setItem("ctx-onboarding-done", "1");
    const w = document.getElementById("onboarding-wrap");
    if (w) w.style.display = "none";
}

function showOnboardingWizardFromSettings() {
    localStorage.removeItem("ctx-onboarding-done");
    const w = document.getElementById("onboarding-wrap");
    if (!w) return;
    w.style.display = "block";
    wizPrepareFromServer();
    wizShowStep(1);
    const navSavings = [...document.querySelectorAll(".nav-item")].find((x) =>
        (x.textContent || "").includes("Savings"),
    );
    if (navSavings) {
        showTab("savings", navSavings);
    }
    w.scrollIntoView({
        behavior: "smooth"
    });
}
async function finishOnboardingWizard() {
    const body = {
        auto_profile_enabled: true,
        inject_enabled: document.getElementById("wiz-inject").checked,
        coaching_enabled: document.getElementById("wiz-coaching").checked,
        store_prompt_text: document.getElementById("wiz-store-prompt").checked,
        embeddings_enabled: document.getElementById("wiz-embed").checked,
        system_prefix: document.getElementById("wiz-prefix").value,
    };
    const slug = (document.getElementById("wiz-profile") || {}).value;
    if (slug) body.active_profile = slug;
    const bRaw = document.getElementById("wiz-budget").value;
    if (bRaw !== "" && !isNaN(parseFloat(bRaw)))
        body.monthly_budget_usd = parseFloat(bRaw);
    const aRaw = document.getElementById("wiz-actual").value;
    if (aRaw !== "" && !isNaN(parseFloat(aRaw)))
        body.monthly_actual_spend_usd = parseFloat(aRaw);
    try {
        await fetch("/api/settings", {
            method: "POST",
            headers: {
                "Content-Type": "application/json"
            },
            body: JSON.stringify(body),
        });
    } catch (e) {
        console.warn(e);
    }
    localStorage.setItem("ctx-onboarding-done", "1");
    const w = document.getElementById("onboarding-wrap");
    if (w) w.style.display = "none";
}