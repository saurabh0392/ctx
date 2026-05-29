import { test, expect } from "@playwright/test";
import {
  mockDashboardApis,
  openDashboard,
  showTab,
  elementWidth,
} from "./helpers/dashboard";

async function box(page: import("@playwright/test").Page, selector: string, nth = 0) {
  const el = page.locator(selector).nth(nth);
  await expect(el).toBeVisible();
  const b = await el.boundingBox();
  expect(b).toBeTruthy();
  return b!;
}

test.describe("dashboard layout", () => {
  test.beforeEach(async ({ page }) => {
    await mockDashboardApis(page);
    await openDashboard(page, true);
  });

  test("experiment content is left-aligned and uses panel width", async ({ page }) => {
    await showTab(page, "experiment");
    await page.waitForSelector("#exp-feature-grid .exp-feature-card");
    await expect(page.locator("#exp-feature-grid .exp-feature-card")).toHaveCount(4);

    const panel = await box(page, "#tab-experiment");
    const grid = await box(page, "#exp-feature-grid");
    const hero = await box(page, "#exp-hero");
    const daily = await box(page, "#exp-body .card", 0);

    expect(Math.abs(grid.x - hero.x)).toBeLessThan(8);
    expect(Math.abs(daily.x - hero.x)).toBeLessThan(8);
    expect(grid.width).toBeGreaterThan(panel.width * 0.85);
    expect(daily.width).toBeGreaterThan(panel.width * 0.85);
  });

  test("experiment cards use two columns on wide viewports", async ({ page }) => {
    await showTab(page, "experiment");
    await page.waitForSelector("#exp-feature-grid .exp-feature-card");

    const cards = page.locator("#exp-feature-grid .exp-feature-card");
    const first = await cards.nth(0).boundingBox();
    const second = await cards.nth(1).boundingBox();
    expect(first).toBeTruthy();
    expect(second).toBeTruthy();

    expect(Math.abs(first!.y - second!.y)).toBeLessThan(8);
    expect(second!.x).toBeGreaterThan(first!.x + first!.width * 0.5);
  });

  test("pipeline feature grid renders cards", async ({ page }) => {
    await showTab(page, "pipeline");
    await page.waitForSelector("#pipe-feature-grid .exp-feature-card");
    await expect(page.locator("#pipe-feature-grid .exp-feature-card")).toHaveCount(8);
    await expect(page.locator("#pipe-hero")).not.toBeEmpty();
  });

  test("simulate tab shows redesigned shell", async ({ page }) => {
    await showTab(page, "simulate");
    await expect(page.locator("#tab-simulate .page-title")).toHaveText("What would ctx do?");
    await expect(page.locator("#sim-btn-preview")).toBeVisible();
    await expect(page.locator(".sim-idle-callout")).toBeVisible();
  });
});
