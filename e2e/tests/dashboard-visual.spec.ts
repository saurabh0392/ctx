import { test, expect } from "@playwright/test";
import { mockDashboardApis, openDashboard, showTab } from "./helpers/dashboard";

test.describe("dashboard visuals", () => {
  test.beforeEach(async ({ page }) => {
    await mockDashboardApis(page);
    await openDashboard(page, true);
  });

  test("experiment feature breakdown", async ({ page }) => {
    await showTab(page, "experiment");
    await page.waitForSelector("#exp-feature-grid .exp-feature-card");
    await expect(page.locator("#exp-body")).toBeVisible();
    await expect(page).toHaveScreenshot("experiment-breakdown.png", {
      fullPage: true,
    });
  });

  test("pipeline features at work", async ({ page }) => {
    await showTab(page, "pipeline");
    await page.waitForSelector("#pipe-feature-grid .exp-feature-card");
    await expect(page).toHaveScreenshot("pipeline-features.png", {
      fullPage: true,
    });
  });

  test("simulate dry run shell", async ({ page }) => {
    await showTab(page, "simulate");
    await expect(page).toHaveScreenshot("simulate-shell.png", {
      fullPage: true,
    });
  });
});
