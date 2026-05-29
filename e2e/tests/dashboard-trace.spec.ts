import { test, expect } from "@playwright/test";
import { mockDashboardApis, openDashboard, showTab } from "./helpers/dashboard";

const hookTraceSample = [
  {
    ts: "2026-05-28T21:00:00.000Z",
    profile: "design",
    pinned_profile: "design",
    effective_profile: "design",
    auto_selected: false,
    tools_kept: 12,
    tools_removed: 4,
    tokens_saved: 2400,
    inject_fired: true,
    adaptive_fired: false,
    enriched: true,
    cost_usd: 0.08,
    input_tokens: 8000,
    output_tokens: 600,
    cache_read_tokens: 10000,
    human_text_prefix: "Fix the trace tab layout",
    working_directory: "/Users/you/project",
  },
];

test.describe("trace tab", () => {
  test.beforeEach(async ({ page }) => {
    await mockDashboardApis(page);
    await page.route("**/api/requests**", (route) =>
      route.fulfill({ contentType: "application/json", body: "[]" }),
    );
    await page.route("**/api/hook-traces**", (route) =>
      route.fulfill({
        contentType: "application/json",
        body: JSON.stringify(hookTraceSample),
      }),
    );
    await openDashboard(page, false);
  });

  test("renders hook trace rows without JS errors", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (err) => errors.push(err.message));

    await showTab(page, "trace");
    await page.waitForSelector("#trace-list .trace-row");

    await expect(page.locator("#trace-list .trace-row")).toHaveCount(1);
    await expect(page.locator(".trace-cost-stack")).toHaveCount(1);
    await expect(page.locator(".trace-flow-panel")).toHaveCount(1);
    expect(errors.filter((e) => e.includes("GATE_META"))).toEqual([]);
    expect(errors).toEqual([]);
  });

  test("expands trace row on click", async ({ page }) => {
    await showTab(page, "trace");
    await page.waitForSelector("#trace-list .trace-row");

    const row = page.locator("#trace-0");
    await row.click();
    await expect(row).toHaveClass(/expanded/);
    await expect(row.locator(".trace-cost-stack")).toBeVisible();
    await expect(row.locator(".trace-flow-link-manual")).toContainText("pinned design");
  });
});
