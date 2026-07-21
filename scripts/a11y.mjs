#!/usr/bin/env node
// Real axe-core accessibility audit against the web client's rendered routes.
//
// Boots a headless Chromium (via Playwright) against a throwaway local static
// server, navigates it to each client route, injects the real axe-core
// engine into the live page, and runs axe.run() against the actual DOM.
// This is a genuine scan, not a canned report. The process exits non-zero
// if any route has 1+ violations, so this is a real CI/local gate — not
// just a printout.
//
// Routes covered:
//   /               -> www/templates/index.html (Handlebars template)
//   /settings.html  -> www/static/settings.html
//   /access_code.html -> www/static/access_code.html
//
// index.html is a server-rendered Handlebars template ({{log_level}} and
// {{#if ...}}/{{/if}} conditionals). There is no running Rylus server to
// render it against here, so per the T1 scope note, we substitute
// representative dummy values for a honest static-markup audit:
//   - {{log_level}} -> "info"
//   - {{#if (not X)}}...{{/if}} blocks are stripped so the guarded
//     `class="hide"` never applies, i.e. we audit the "fully revealed"
//     variant of the settings panel (superset of visible elements).
// This is a client-side a11y lint of the authored markup, not a full
// server-integration test.

import { chromium } from 'playwright';
import { createServer } from 'node:http';
import { readFile, mkdtemp, rm, writeFile, cp } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve('axe-core/axe.min.js'), 'utf8');

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, '..');
const staticDir = path.join(repoRoot, 'www', 'static');
const templatesDir = path.join(repoRoot, 'www', 'templates');

function renderIndexTemplate(source) {
    return source
        .replace(/\{\{#if \(not [a-zA-Z_]+\)\}\}([\s\S]*?)\{\{\/if\}\}/g, '')
        .replace(/\{\{log_level\}\}/g, 'info');
}

async function buildClientBundle() {
    // Ensure lib.js/sw.js exist so index.html's real client script is part
    // of the rendered page (not a 404), matching what a user's browser
    // would actually load.
    execFileSync('npm', ['run', 'build:dev'], { cwd: repoRoot, stdio: 'inherit' });
}

async function prepareAuditRoot() {
    const tmpRoot = await mkdtemp(path.join(os.tmpdir(), 'rylus-a11y-'));
    await cp(staticDir, tmpRoot, { recursive: true });
    const indexSource = await readFile(path.join(templatesDir, 'index.html'), 'utf8');
    await writeFile(path.join(tmpRoot, 'index.html'), renderIndexTemplate(indexSource), 'utf8');
    return tmpRoot;
}

const MIME = {
    '.html': 'text/html',
    '.css': 'text/css',
    '.js': 'text/javascript',
    '.webmanifest': 'application/manifest+json',
};

function startStaticServer(rootDir) {
    const server = createServer(async (req, res) => {
        try {
            const urlPath = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
            const relPath = urlPath === '/' ? 'index.html' : urlPath.replace(/^\/+/, '');
            const filePath = path.join(rootDir, relPath);
            if (!filePath.startsWith(rootDir)) {
                res.writeHead(403).end();
                return;
            }
            const ext = path.extname(filePath);
            const body = await readFile(filePath);
            res.writeHead(200, { 'Content-Type': MIME[ext] || 'application/octet-stream' });
            res.end(body);
        } catch (err) {
            res.writeHead(404).end('not found');
        }
    });
    return new Promise((resolve) => {
        server.listen(0, '127.0.0.1', () => resolve(server));
    });
}

const ROUTES = [
    { name: '/ (index, "fully revealed" template render)', path: '/' },
    { name: '/settings.html', path: '/settings.html' },
    { name: '/access_code.html', path: '/access_code.html' },
];

async function auditRoute(page, baseUrl, route) {
    await page.goto(baseUrl + route.path, { waitUntil: 'load' });
    await page.addScriptTag({ content: axeSource });
    const results = await page.evaluate(async () => {
        // eslint-disable-next-line no-undef
        return await axe.run();
    });
    return results;
}

function printReport(routeName, results) {
    console.log(`\n=== ${routeName} ===`);
    console.log(`  Violations: ${results.violations.length}  |  Passes: ${results.passes.length}  |  Incomplete: ${results.incomplete.length}`);
    for (const v of results.violations) {
        console.log(`  [${v.impact ?? 'unknown'}] ${v.id} — ${v.help} (${v.nodes.length} node${v.nodes.length === 1 ? '' : 's'})`);
        console.log(`    ${v.helpUrl}`);
        for (const node of v.nodes) {
            console.log(`    - ${node.target.join(' ')}`);
        }
    }
    return results.violations.length;
}

async function main() {
    await buildClientBundle();
    const auditRoot = await prepareAuditRoot();
    const server = await startStaticServer(auditRoot);
    const { port } = server.address();
    const baseUrl = `http://127.0.0.1:${port}`;

    const browser = await chromium.launch();
    let totalViolations = 0;
    try {
        const page = await browser.newPage();
        for (const route of ROUTES) {
            const results = await auditRoute(page, baseUrl, route);
            totalViolations += printReport(route.name, results);
        }
    } finally {
        await browser.close();
        server.close();
        await rm(auditRoot, { recursive: true, force: true });
    }

    console.log(`\naxe-core audit complete: ${totalViolations} total violation group(s) across ${ROUTES.length} route(s).`);

    if (totalViolations > 0) {
        console.error(`\naxe-core audit FAILED: ${totalViolations} violation group(s) found.`);
        process.exitCode = 1;
    }
}

main().catch((err) => {
    console.error('a11y audit failed to run:', err);
    process.exit(1);
});
