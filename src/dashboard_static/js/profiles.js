// ─── Tab 3: Profiles ─────────────────────────────────────
let _allServers = [];

async function loadProfiles() {
    const profiles = await fetch("/api/profiles").then((r) => r.json());
    const grid = document.getElementById("profiles-grid");

    _allServers = [
        ...new Set(
            profiles.flatMap((p) => [...p.servers_included, ...p.servers_excluded]),
        ),
    ].sort();

    grid.innerHTML = profiles
        .map((p) => {
            const savePct = (p.savings_pct * 100).toFixed(0);
            const meterPct = (1 - p.savings_pct) * 100;
            return `<div class="profile-card ${p.active ? "is-active" : ""}">
      ${p.active ? `<div class="profile-active-pip">Active</div>` : ""}
      <div class="profile-name">${p.display}</div>
      <div class="profile-desc">${p.description}</div>
      <div class="profile-meter">
        <div class="profile-meter-label">
          <span>${p.tool_count} tools</span>
          <span style="color:var(--green)">${savePct}% saved vs all</span>
        </div>
        <div class="profile-meter-track">
          <div class="profile-meter-fill" style="width:${meterPct}%"></div>
        </div>
      </div>
      <div class="server-wrap">
        ${p.servers_included
          .slice(0, 6)
          .map((s) => `<span class="server-tag in">${s}</span>`)
          .join("")}
        ${p.servers_included.length > 6 ? `<span class="server-tag out">+${p.servers_included.length - 6}</span>` : ""}
        ${p.servers_excluded
          .slice(0, 3)
          .map((s) => `<span class="server-tag out">${s}</span>`)
          .join("")}
        ${p.servers_excluded.length > 3 ? `<span class="server-tag out">+${p.servers_excluded.length - 3} hidden</span>` : ""}
      </div>
      <button class="profile-switch-btn" ${p.active ? "disabled" : ""} onclick="switchProfile('${p.slug}')">
        ${p.active ? "Currently active" : "Switch to this profile"}
      </button>
    </div>`;
        })
        .join("");
}

async function switchProfile(slug) {
    await fetch("/api/profiles/switch", {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify({
            slug
        }),
    });
    loadProfiles();
}

function openNewProfileModal() {
    document.getElementById("server-checklist").innerHTML = _allServers
        .map(
            (s) => `
    <label class="server-check-item"><input type="checkbox" value="${s}"> ${s}</label>
  `,
        )
        .join("");
    document.getElementById("new-profile-modal").classList.add("open");
}

function closeNewProfileModal() {
    document.getElementById("new-profile-modal").classList.remove("open");
}
async function createProfile() {
    const name = document.getElementById("new-profile-name").value.trim();
    if (!name) return;
    const checked = [
        ...document.querySelectorAll("#server-checklist input:checked"),
    ].map((i) => i.value);
    if (!checked.length) {
        alert("Select at least one server.");
        return;
    }
    await fetch("/api/profiles/create", {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify({
            name,
            servers: checked.map(
                (s) => "mcp__claude_ai_" + s.replace(/ /g, "_") + "__",
            ),
        }),
    });
    closeNewProfileModal();
    loadProfiles();
}