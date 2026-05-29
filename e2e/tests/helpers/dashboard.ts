import type { Page } from "@playwright/test";
import settings from "../../fixtures/settings-active-ab.json";
import abReport from "../../fixtures/ab-report-sample.json";
import gates from "../../fixtures/gates-sample.json";

const emptyList = "[]";
const emptyObj = "{}";

/** Deterministic API responses so screenshots and layout checks are stable. */
export async function mockDashboardApis(page: Page) {
  await page.route("**/api/settings", (route) =>
    route.fulfill({ contentType: "application/json", body: JSON.stringify(settings) }),
  );
  await page.route("**/api/ab-report**", (route) =>
    route.fulfill({ contentType: "application/json", body: JSON.stringify(abReport) }),
  );
  await page.route("**/api/ab-daily**", (route) =>
    route.fulfill({ contentType: "application/json", body: emptyList }),
  );
  await page.route("**/api/hook-traces**", (route) =>
    route.fulfill({ contentType: "application/json", body: emptyList }),
  );
  await page.route("**/api/gates**", (route) =>
    route.fulfill({ contentType: "application/json", body: JSON.stringify(gates) }),
  );
  await page.route("**/api/profiles/readiness", (route) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        ready: false,
        tool_calls: 24,
        tool_calls_needed: 20,
        servers: 1,
        servers_needed: 3,
        mcp_sessions: 1,
        mcp_sessions_needed: 2,
      }),
    }),
  );
  await page.route("**/api/stats**", (route) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({ active_profile: "all", sessions_fallback: true }),
    }),
  );
  await page.route("**/api/profiles", (route) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify([
        { slug: "all", display: "All tools" },
        { slug: "personal", display: "Personal" },
      ]),
    }),
  );
  await page.route("**/api/allowance**", (route) =>
    route.fulfill({ contentType: "application/json", body: emptyObj }),
  );
  await page.route("**/api/savings**", (route) =>
    route.fulfill({ contentType: "application/json", body: emptyObj }),
  );
}

export async function openDashboard(page: Page, dev = true) {
  await page.goto(dev ? "/?dev=1" : "/");
  await page.waitForFunction(() => typeof (window as any).showTab === "function");
  if (dev) {
    await page.waitForFunction(() => localStorage.getItem("ctx_dev") === "1");
    await page.waitForSelector("#nav-dev-experiment:not([style*='display: none'])", {
      timeout: 10_000,
    });
  }
}

export async function showTab(page: Page, tabId: string) {
  await page.evaluate((id) => {
    const nav = document.querySelector(
      `.nav-item[onclick*="showTab('${id}'"]`,
    ) as HTMLElement | null;
    if (!nav) throw new Error(`nav item for tab ${id} not found`);
    (window as any).showTab(id, nav);
  }, tabId);
  await page.waitForSelector(`#tab-${tabId}.active`, { timeout: 10_000 });
}

/** Horizontal center of element relative to its offset parent. */
export async function elementCenterX(page: Page, selector: string): Promise<number> {
  return page.evaluate((sel) => {
    const el = document.querySelector(sel) as HTMLElement | null;
    if (!el) throw new Error(`missing ${sel}`);
    const r = el.getBoundingClientRect();
    return r.left + r.width / 2;
  }, selector);
}

export async function elementWidth(page: Page, selector: string): Promise<number> {
  return page.evaluate((sel) => {
    const el = document.querySelector(sel) as HTMLElement | null;
    if (!el) throw new Error(`missing ${sel}`);
    return el.getBoundingClientRect().width;
  }, selector);
}
