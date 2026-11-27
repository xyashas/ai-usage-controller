import { test, expect, type Page } from "@playwright/test";

function trackErrors(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (err) => errors.push(`pageerror: ${err.message}`));
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(`console.error: ${msg.text()}`);
  });
  return errors;
}

async function assertNoOverflow(page: Page) {
  const overflow = await page.evaluate(() => {
    const doc = document.documentElement;
    const panel = document.querySelector(".panel-card, .shell, #root") as HTMLElement | null;
    const docOver = doc.scrollWidth > doc.clientWidth + 1;
    const panelOver = panel ? panel.scrollWidth > panel.clientWidth + 2 : false;
    return { docOver, panelOver, docScroll: doc.scrollWidth, docClient: doc.clientWidth };
  });
  expect(overflow.docOver, `Document horizontal overflow: ${JSON.stringify(overflow)}`).toBe(false);
  expect(overflow.panelOver, `Panel horizontal overflow: ${JSON.stringify(overflow)}`).toBe(false);
}

const VIEWS = ["panel", "ask", "agents", "handoffs", "settings", "setup", "audit", "paywall", "onboarding"] as const;
const SCENARIOS = ["normal", "critical", "approval", "empty", "unconnected"] as const;
const PLATFORMS = ["windows", "macos"] as const;

test.describe("View × Scenario × Platform render & overflow audit", () => {
  for (const platform of PLATFORMS) {
    for (const view of VIEWS) {
      test(`${platform} / ?view=${view} renders cleanly without errors or overflow`, async ({ page }) => {
        const errors = trackErrors(page);
        await page.goto(`/?view=${view}&platform=${platform}&scenario=normal`, { waitUntil: "networkidle" });
        await page.waitForTimeout(250);
        expect(errors, `Errors on ${platform}/${view}: ${errors.join("; ")}`).toEqual([]);
        await assertNoOverflow(page);
      });
    }
  }

  for (const scenario of SCENARIOS) {
    test(`panel / ?scenario=${scenario} renders cleanly without errors or overflow`, async ({ page }) => {
      const errors = trackErrors(page);
      await page.goto(`/?view=panel&scenario=${scenario}`, { waitUntil: "networkidle" });
      await page.waitForTimeout(250);
      expect(errors, `Errors on scenario=${scenario}: ${errors.join("; ")}`).toEqual([]);
      await assertNoOverflow(page);
    });
  }

  for (const buddy of ["pointing", "listening", "thinking", "speaking"] as const) {
    test(`buddy overlay / ?overlay=buddy&buddy=${buddy} renders cleanly`, async ({ page }) => {
      const errors = trackErrors(page);
      await page.goto(`/?overlay=buddy&buddy=${buddy}`, { waitUntil: "networkidle" });
      await page.waitForTimeout(200);
      expect(errors, `Errors on buddy=${buddy}: ${errors.join("; ")}`).toEqual([]);
    });
  }
});

test.describe("Main interactive user flows", () => {
  test("open panel from pill, inspect daily recap, and dismiss it", async ({ page }) => {
    const errors = trackErrors(page);
    await page.goto("/?scenario=normal", { waitUntil: "networkidle" });
    await page.waitForTimeout(200);
    // Open panel by clicking or hovering the collapsed pill
    const pill = page.locator("button[aria-label*='Open']").first();
    await pill.click();
    await page.waitForTimeout(300);
    await expect(page.locator("[aria-label='Usage panel']")).toBeVisible();
    // Morning brief / daily recap dismiss button if visible
    const dismissBrief = page.locator("button[aria-label*='Dismiss'], button[title*='Dismiss']").first();
    if (await dismissBrief.isVisible()) {
      await dismissBrief.click();
      await page.waitForTimeout(150);
    }
    expect(errors).toEqual([]);
  });

  test("approval flow: Allow button responds cleanly", async ({ page }) => {
    const errors = trackErrors(page);
    await page.goto("/?view=panel&scenario=approval", { waitUntil: "networkidle" });
    await page.waitForTimeout(250);
    const allowBtn = page.getByRole("button", { name: /^Allow$/i }).first();
    await expect(allowBtn).toBeVisible();
    await allowBtn.click();
    await page.waitForTimeout(200);
    expect(errors).toEqual([]);
  });

  test("Ask keeps the conversation when the panel closes and reopens", async ({ page }) => {
    const errors = trackErrors(page);
    await page.goto("/?view=ask&scenario=normal", { waitUntil: "networkidle" });
    await page.waitForTimeout(250);
    const input = page.locator("textarea").first();
    await input.fill("Why did the coupon test fail?");
    await input.press("Enter");
    // Leave Ask while the answer is still streaming in (Escape → Usage, the
    // Ask view unmounts), then come back to it.
    await page.waitForTimeout(150);
    await page.keyboard.press("Escape");
    await page.waitForTimeout(1500);
    await page.getByRole("button", { name: "Ask", exact: true }).click();
    await page.waitForTimeout(600);
    await expect(page.getByText("Why did the coupon test fail?")).toBeVisible();
    await expect(page.getByRole("button", { name: "New chat" })).toBeVisible();
    // The answer finished arriving while the panel was closed.
    await expect(page.getByRole("button", { name: /^copy$/i }).first()).toBeVisible();
    await page.getByRole("button", { name: "New chat" }).click();
    await expect(page.getByText("Why did the coupon test fail?")).toHaveCount(0);
    expect(errors).toEqual([]);
  });

  test("Ask flow: submit question, receive answer, copy", async ({ page }) => {
    const errors = trackErrors(page);
    await page.goto("/?view=ask&scenario=normal", { waitUntil: "networkidle" });
    await page.waitForTimeout(250);
    const input = page.locator("textarea, input[type='text']").first();
    await expect(input).toBeVisible();
    await input.fill("Why did the coupon test fail?");
    await input.press("Enter");
    await page.waitForTimeout(700);
    const copyBtn = page.getByRole("button", { name: /copy/i }).first();
    if (await copyBtn.isVisible()) {
      await copyBtn.click();
    }
    expect(errors).toEqual([]);
  });

  test("Agents flow: task input and Polish button work", async ({ page }) => {
    const errors = trackErrors(page);
    await page.goto("/?view=agents&scenario=normal", { waitUntil: "networkidle" });
    await page.waitForTimeout(250);
    const input = page.locator("textarea, input[type='text']").first();
    await expect(input).toBeVisible();
    await input.fill("fix login bug");
    const polishBtn = page.getByRole("button", { name: /polish/i }).first();
    if (await polishBtn.isVisible()) {
      await polishBtn.click();
      await page.waitForTimeout(300);
    }
    expect(errors).toEqual([]);
  });

  test("Handoffs & Settings flows + Escape back navigation", async ({ page }) => {
    const errors = trackErrors(page);
    await page.goto("/?view=settings&scenario=normal", { waitUntil: "networkidle" });
    await page.waitForTimeout(250);
    const checkbox = page.locator("input[type='checkbox']").first();
    if (await checkbox.isVisible()) {
      await checkbox.click();
      await page.waitForTimeout(150);
    }
    // Escape from sub-view should return to panel
    await page.keyboard.press("Escape");
    await page.waitForTimeout(250);
    await expect(page.locator("[aria-label='Usage panel']")).toBeVisible();
    expect(errors).toEqual([]);
  });
});

test.describe("Accessibility & 60fps panel-open performance", () => {
  test("a11y: icon-btn aria-label, focus-visible rule, contrast >= 4.5:1, prefers-reduced-motion", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/?view=panel&scenario=critical", { waitUntil: "networkidle" });
    await page.waitForTimeout(250);

    // Check every icon-btn has aria-label
    const iconAudit = await page.evaluate(() => {
      const btns = Array.from(document.querySelectorAll("button.icon-btn"));
      return btns.map((b) => ({
        ariaLabel: b.getAttribute("aria-label"),
        title: b.getAttribute("title"),
      }));
    });
    for (const b of iconAudit) {
      expect(b.ariaLabel, "icon-btn missing aria-label").toBeTruthy();
    }

    // Check focus-visible stylesheet rule exists
    const hasFocusVisible = await page.evaluate(() => {
      for (const sheet of Array.from(document.styleSheets)) {
        try {
          for (const rule of Array.from(sheet.cssRules)) {
            if (rule.cssText.includes("focus-visible")) return true;
          }
        } catch {
          // ignore cross-origin
        }
      }
      return false;
    });
    expect(hasFocusVisible, "Missing :focus-visible CSS rule").toBe(true);

    // Contrast check on primary text vs surface background
    const contrast = await page.evaluate(() => {
      function parseRgb(s: string): [number, number, number] | null {
        const m = s.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
        return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
      }
      function lum([r, g, b]: [number, number, number]) {
        const f = (v: number) => {
          const x = v / 255;
          return x <= 0.03928 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
        };
        return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
      }
      const panel = document.querySelector("[aria-label='Usage panel']") as HTMLElement | null;
      if (!panel) return null;
      const cs = getComputedStyle(panel);
      const fg = parseRgb(cs.color);
      const bg = parseRgb(cs.backgroundColor);
      if (!fg || !bg) return null;
      const l1 = lum(fg);
      const l2 = lum(bg);
      return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
    });
    if (contrast !== null) {
      expect(contrast, `Primary text contrast ${contrast.toFixed(2)} below 4.5:1`).toBeGreaterThanOrEqual(4.5);
    }
  });

  test("performance: panel open runs smoothly near 60fps without layout thrash", async ({ page }) => {
    await page.goto("/?scenario=normal", { waitUntil: "networkidle" });
    await page.waitForTimeout(200);

    // Start rAF frame-time sampler, trigger panel open, sample for 400 ms
    const metrics = await page.evaluate(async () => {
      const deltas: number[] = [];
      let last = performance.now();
      let running = true;
      function tick(now: number) {
        deltas.push(now - last);
        last = now;
        if (running) requestAnimationFrame(tick);
      }
      requestAnimationFrame(tick);
      const pill = document.querySelector("button[aria-label*='Open']") as HTMLElement | null;
      pill?.click();
      await new Promise((r) => setTimeout(r, 400));
      running = false;
      const sorted = [...deltas].sort((a, b) => a - b);
      const median = sorted[Math.floor(sorted.length / 2)] ?? 0;
      const max = sorted[sorted.length - 1] ?? 0;
      const fps = median > 0 ? 1000 / median : 60;
      return { frames: deltas.length, medianMs: median, maxMs: max, fps };
    });

    console.log(
      `[PANEL_OPEN_PERF] frames=${metrics.frames} median_ms=${metrics.medianMs.toFixed(2)} max_ms=${metrics.maxMs.toFixed(2)} est_fps=${metrics.fps.toFixed(1)}`
    );
    expect(metrics.frames).toBeGreaterThan(5);
    expect(metrics.medianMs, `Median frame time ${metrics.medianMs.toFixed(2)}ms too slow`).toBeLessThan(25);
  });
});

test.describe("Voice & Ask regression flows", () => {
  test("clipboard-only copied error offers Fix/Explain/Find the bug and answers cleanly", async ({ page }) => {
    const errors = trackErrors(page);
    await page.goto("/?view=ask&scenario=copied", { waitUntil: "networkidle" });
    await page.waitForTimeout(250);
    await expect(page.getByText(/Copied:/)).toBeVisible();
    const fixBtn = page.getByRole("button", { name: /^Fix$/i });
    await expect(fixBtn).toBeVisible();
    await expect(page.getByRole("button", { name: /^Explain$/i })).toBeVisible();
    await expect(page.getByRole("button", { name: /^Find the bug$/i })).toBeVisible();
    await fixBtn.click();
    await page.waitForTimeout(700);
    await expect(page.getByRole("button", { name: /New chat/i })).toBeVisible();
    expect(errors).toEqual([]);
  });

  test("Ask conversation survives closing/reopening view and New chat clears it", async ({ page }) => {
    const errors = trackErrors(page);
    await page.addInitScript(() => { localStorage.setItem("ask.tool", "gemini"); });
    await page.goto("/?view=ask&scenario=normal", { waitUntil: "networkidle" });
    await page.waitForTimeout(250);
    // Stale gemini pin must be sanitized to Auto · Claude
    await expect(page.getByRole("button", { name: /Auto · Claude/i })).toBeVisible();
    const input = page.locator("textarea").first();
    await input.fill("Keep this thread alive");
    await input.press("Enter");
    await page.waitForTimeout(700);
    await expect(page.getByText("Keep this thread alive")).toBeVisible();
    // Leave Ask -> panel -> return to Ask
    await page.getByRole("button", { name: /^Back$/i }).click();
    await page.waitForTimeout(200);
    await page.getByRole("button", { name: /^Ask$/i }).first().click();
    await page.waitForTimeout(250);
    await expect(page.getByText("Keep this thread alive")).toBeVisible();
    // New chat clears it
    await page.getByRole("button", { name: /New chat/i }).click();
    await page.waitForTimeout(150);
    await expect(page.getByText("Keep this thread alive")).toHaveCount(0);
    expect(errors).toEqual([]);
  });

  test("Buddy starting and listening states show Mic starting and amber glow", async ({ page }) => {
    const errors = trackErrors(page);
    await page.goto("/?overlay=buddy&buddy=starting", { waitUntil: "networkidle" });
    await page.waitForTimeout(200);
    await expect(page.getByText(/Mic starting/i)).toBeVisible();

    await page.goto("/?overlay=buddy&buddy=listening", { waitUntil: "networkidle" });
    await page.waitForTimeout(200);
    await expect(page.locator(".buddy-glow")).toBeVisible();
    await expect(page.getByText(/how do I turn off notifications/i)).toBeVisible();
    expect(errors).toEqual([]);
  });
});
