// Minimal service worker for Rylus.
//
// The tablet page is basically three assets (index HTML, lib.js, style.css).
// This SW precaches them so "pull tablet out of bag" works even before the
// local network WiFi connects, and so a brief connection blip on the server
// side doesn't blank the page. We intentionally DO NOT try to cache /ws or
// video frames — streaming state must always be live.

const CACHE_NAME = "rylus-shell-v2";
// Static assets only. `/` is deliberately excluded — it returns either the
// authenticated tablet HTML or the access-code login depending on session
// state, and caching whichever the SW saw first would leak stale auth state
// on reload.
const SHELL = ["/style.css", "/lib.js", "/manifest.webmanifest"];

self.addEventListener("install", (event: any) => {
    event.waitUntil(
        caches.open(CACHE_NAME).then((cache) => cache.addAll(SHELL)).catch(() => {
            // Best-effort; don't block install if any one asset fails.
        }),
    );
    (self as any).skipWaiting();
});

self.addEventListener("activate", (event: any) => {
    event.waitUntil(
        (async () => {
            const keys = await caches.keys();
            await Promise.all(keys.filter((k) => k !== CACHE_NAME).map((k) => caches.delete(k)));
            await (self as any).clients.claim();
        })(),
    );
});

self.addEventListener("fetch", (event: any) => {
    const req = event.request as Request;
    const url = new URL(req.url);

    // Never intercept WebSocket upgrades or streaming endpoints.
    if (url.pathname === "/ws" || url.pathname.startsWith("/api/")) return;
    // Don't cache POSTs or non-GET verbs.
    if (req.method !== "GET") return;

    // `/` is auth-dependent and must never be cached. Let it go straight to
    // the network; the browser will naturally display the access-code page
    // when offline.
    if (url.pathname === "/") return;

    event.respondWith(
        (async () => {
            try {
                const live = await fetch(req);
                if (live.ok && SHELL.includes(url.pathname)) {
                    const copy = live.clone();
                    caches.open(CACHE_NAME).then((c) => c.put(req, copy)).catch(() => {});
                }
                return live;
            } catch (_e) {
                const cached = await caches.match(req);
                if (cached) return cached;
                return new Response("offline", { status: 503, statusText: "Offline" });
            }
        })(),
    );
});
