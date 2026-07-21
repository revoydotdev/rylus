#!/usr/bin/env node
// Real keyboard-only operability check for the settings panel (/settings.html).
//
// Boots a headless Chromium (via Playwright) against a throwaway local static
// server — same pattern as scripts/a11y.mjs — and drives the page using only
// keyboard input (Tab / Shift+Tab / Enter / Space / typed keys). No mouse or
// click() calls are used anywhere in this script.
//
// Verifies:
//   1. Every visible, non-disabled interactive control on the page (inputs,
//      checkboxes, the save button) is reachable via sequential Tab presses,
//      in DOM order, with no gaps and no extras beyond the expected set.
//   2. Text/number/password inputs accept typed keyboard input.
//   3. Checkboxes toggle their checked state on Space.
//   4. The save button fires a real click event on both Enter and Space
//      (native <button> keyboard-activation semantics), tested from a clean
//      (non-disabled) state for each.
//   5. Shift+Tab moves focus backward through the tab sequence correctly.
//
// This is part of M2.P3.S1.T2 ("verify keyboard-only operation of the
// settings panel") and is wired into `npm run a11y` so it runs on every
// invocation of the accessibility gate, not as a one-off script.

import { chromium } from 'playwright';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, '..');
const staticDir = path.join(repoRoot, 'www', 'static');

function startStaticServer(rootDir) {
    const server = createServer(async (req, res) => {
        try {
            const urlPath = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
            const relPath = urlPath.replace(/^\/+/, '');
            const filePath = path.join(rootDir, relPath);
            if (!filePath.startsWith(rootDir)) {
                res.writeHead(403).end();
                return;
            }
            const body = await readFile(filePath);
            res.writeHead(200, { 'Content-Type': 'text/html' });
            res.end(body);
        } catch {
            res.writeHead(404).end('not found');
        }
    });
    return new Promise((resolve) => {
        server.listen(0, '127.0.0.1', () => resolve(server));
    });
}

const failures = [];

function check(label, condition, detail) {
    if (condition) {
        console.log(`  [PASS] ${label}`);
    } else {
        console.log(`  [FAIL] ${label}${detail ? ` — ${detail}` : ''}`);
        failures.push(label);
    }
}

async function getActiveElementId(page) {
    return page.evaluate(() => {
        const el = document.activeElement;
        if (!el || el === document.body) return null;
        return el.id || null;
    });
}

async function waitForEnabled(page, id, timeoutMs = 2000) {
    const start = Date.now();
    while (Date.now() - start < timeoutMs) {
        const disabled = await page.evaluate((elId) => document.getElementById(elId).disabled, id);
        if (!disabled) return true;
        await page.waitForTimeout(20);
    }
    return false;
}

async function installClickCounter(page, id) {
    await page.evaluate((elId) => {
        window.__clickCount = 0;
        document.getElementById(elId).addEventListener('click', () => {
            window.__clickCount += 1;
        });
    }, id);
}

async function main() {
    const server = await startStaticServer(staticDir);
    const { port } = server.address();
    const baseUrl = `${'http'}://127.0.0.1:${port}`;

    const browser = await chromium.launch();
    try {
        const page = await browser.newPage();
        await page.goto(`${baseUrl}/settings.html`, { waitUntil: 'load' });

        // Compute the expected set of visible, keyboard-reachable controls
        // directly from the live DOM, so this check tracks the real markup
        // rather than a hand-maintained list that can drift.
        const expectedIds = await page.evaluate(() => {
            const candidates = Array.from(document.querySelectorAll('input, button, select, textarea, a[href]'));
            return candidates
                .filter((el) => el.offsetParent !== null && !el.disabled)
                .map((el) => el.id)
                .filter(Boolean);
        });
        console.log(`Expected tab-reachable controls (DOM order): ${expectedIds.join(', ')}`);

        // --- Pass 1: forward traversal, typing, checkbox toggling, Enter activation ---
        await installClickCounter(page, 'save_btn');

        const visited = [];
        for (let i = 0; i < expectedIds.length + 1; i += 1) {
            await page.keyboard.press('Tab');
            const id = await getActiveElementId(page);
            visited.push(id);

            if (id === 'try_vaapi' || id === 'try_nvenc' || id === 'try_videotoolbox' || id === 'try_mediafoundation') {
                const before = await page.evaluate((elId) => document.getElementById(elId).checked, id);
                await page.keyboard.press('Space');
                const after = await page.evaluate((elId) => document.getElementById(elId).checked, id);
                check(`Space toggles checkbox #${id}`, after === !before, `checked ${before} -> ${after}`);
                await page.keyboard.press('Space'); // restore
            }

            if (id === 'bind_address' || id === 'web_port' || id === 'access_code') {
                const testValue = id === 'web_port' ? '9' : 'kb-test';
                await page.evaluate((elId) => { document.getElementById(elId).value = ''; }, id);
                await page.keyboard.type(testValue);
                const value = await page.evaluate((elId) => document.getElementById(elId).value, id);
                check(`Keyboard typing reaches input #${id}`, value === testValue, `got "${value}"`);
            }

            if (id === 'save_btn') {
                await page.keyboard.press('Enter');
                const count = await page.evaluate(() => window.__clickCount);
                check('Enter activates #save_btn (native button semantics)', count === 1, `click count ${count}`);
                // saveConfig() disables the button while its fetch is in
                // flight; wait for it to re-enable before continuing so the
                // next Tab press isn't racing an in-progress save.
                await waitForEnabled(page, 'save_btn');
            }
        }

        check(
            'Every visible interactive control is reachable via Tab, in DOM order, no gaps',
            JSON.stringify(visited.slice(0, expectedIds.length)) === JSON.stringify(expectedIds),
            `expected ${JSON.stringify(expectedIds)}, got ${JSON.stringify(visited.slice(0, expectedIds.length))}`,
        );

        // One Tab past the last control should not land on some unexpected
        // extra element (e.g. a hidden control that shouldn't be reachable).
        // Browsers vary on whether this wraps to the first control or leaves
        // the page entirely (headless has no browser chrome to receive
        // focus) — both are legitimate; landing on a stray extra id is not.
        const afterLast = visited[expectedIds.length];
        check(
            'No extra/unexpected control is reachable after the last known one',
            afterLast === null || afterLast === expectedIds[0],
            `got #${afterLast}`,
        );

        // --- Pass 2: fresh reload for unambiguous backward-nav + Space activation ---
        // (Reloading gives a clean, non-disabled save button and a clean
        // click counter, avoiding any race with pass 1's in-flight save.)
        await page.goto(`${baseUrl}/settings.html`, { waitUntil: 'load' });
        await installClickCounter(page, 'save_btn');
        for (let i = 0; i < expectedIds.length; i += 1) {
            await page.keyboard.press('Tab');
        }
        const landedOnSave = await getActiveElementId(page);
        check('Fresh Tab sequence lands on #save_btn after N presses', landedOnSave === 'save_btn', `got #${landedOnSave}`);

        await page.keyboard.press('Shift+Tab');
        const backOne = await getActiveElementId(page);
        const expectedPrev = expectedIds[expectedIds.length - 2];
        check(`Shift+Tab from #save_btn moves back to #${expectedPrev}`, backOne === expectedPrev, `got #${backOne}`);

        await page.keyboard.press('Tab');
        const forwardAgain = await getActiveElementId(page);
        check('Tab from there returns to #save_btn', forwardAgain === 'save_btn', `got #${forwardAgain}`);

        await page.keyboard.press('Space');
        const spaceCount = await page.evaluate(() => window.__clickCount);
        check('Space activates #save_btn (native button semantics)', spaceCount === 1, `click count ${spaceCount}`);
    } finally {
        await browser.close();
        server.close();
    }

    console.log(`\nkeyboard-only operability check complete: ${failures.length} failure(s).`);
    if (failures.length > 0) {
        console.error(`\nkeyboard-only operability check FAILED: ${failures.join('; ')}`);
        process.exitCode = 1;
    }
}

main().catch((err) => {
    console.error('keyboard-only operability check failed to run:', err);
    process.exit(1);
});
