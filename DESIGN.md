# Design System — Rylus

## Product Context
- **What this is:** A screen-mirroring tool that turns your tablet or smartphone into a graphics tablet / touch screen for your computer
- **Who it's for:** Digital artists, note-takers, and anyone who wants to use their tablet as an input device for their desktop
- **Space/industry:** Creative tools / remote input. Peers: Astropad, Duet Display, Spacedesk, Apple Sidecar
- **Project type:** Desktop utility (native egui settings panel) + web app (browser-based tablet remote client)
- **Two UI surfaces:** Native desktop client (Rust/egui) and browser-based tablet client (TypeScript/HTML/CSS)

## Aesthetic Direction
- **Direction:** Industrial/Utilitarian
- **Decoration level:** Intentional — subtle depth through surface elevation (card shadows in light mode, border highlights in dark mode). No gradients, no brush strokes, no decorative flair.
- **Mood:** Precision tool for creative people. Technical credibility with creative warmth. Respects the user's intelligence. Feels like a well-made instrument, not a lifestyle brand.
- **Reference sites:** Astropad (premium creative, pink accent), Duet Display (corporate productivity, blue), Spacedesk (engineering utility, green). Rylus occupies the space between premium and open-source: technically credible, dark-first, cyan accent that no competitor uses.

## Typography
- **Display/Hero:** Geist (700 weight) — precise, technical, modern sans-serif. Built for developer tools. Not overused in the creative space. Letter-spacing: -0.03em at display sizes.
- **Body:** Geist (400 weight) — same family for cohesion. Clean at small sizes. 15px / 1.6 line-height.
- **UI/Labels:** Geist (500 weight) — 13px for labels, 14px for controls.
- **Data/Tables:** Geist Mono — for log viewer, stats, debug overlay, connection URLs, FPS display. Supports tabular-nums. 13px / 1.8 line-height.
- **Code:** Geist Mono — terminal output, server logs, config values.
- **Loading:** Self-hosted WOFF2. Geist is MIT licensed (github.com/vercel/geist-font). For web client, load via CDN (cdn.jsdelivr.net/npm/geist). For native client, bundle with binary.
- **Scale:**
  - Display: 42-56px / 700 / -0.03em
  - Heading: 24px / 600 / -0.02em
  - Subheading: 18px / 500
  - Body: 15px / 400 / 1.6 line-height
  - UI Label: 13px / 500
  - Small/Caption: 12px / 400
  - Mono Label: 11px / 400 / 0.08em letter-spacing / uppercase

## Color
- **Approach:** Restrained — one accent + neutrals. Color is rare and meaningful. Cyan (#00aaff) is the single accent, used for interactive elements, active states, and the stylus pressure visualization.
- **Dark mode (primary):**
  - Background: `#1e1e1e`
  - Surface: `#2a2a2a`
  - Surface raised: `#333333`
  - Border: `#3a3a3a`
  - Border subtle: `#2f2f2f`
  - Text: `#e0e0e0`
  - Text secondary: `#888888`
  - Text muted: `#666666`
- **Light mode:**
  - Background: `#f5f5f5`
  - Surface: `#ffffff`
  - Surface raised: `#ffffff`
  - Border: `#e0e0e0`
  - Border subtle: `#ebebeb`
  - Text: `#1a1a1a`
  - Text secondary: `#666666`
  - Text muted: `#999999`
- **Accent:** `#00aaff` — interactive elements, links, active toggles, focus rings, stylus visualization
- **Accent hover:** `#33bbff`
- **Accent dim:** `rgba(0, 170, 255, 0.15)` — subtle backgrounds for accent-related elements
- **Semantic:**
  - Success: `#22c55e` — server started, client connected
  - Warning: `#f59e0b` — frame drops, performance degradation
  - Error: `#ef4444` — connection failed, invalid input, server crash
  - Info: `#00aaff` — same as accent, informational messages
- **Dark mode strategy:** Dark-mode-first. Every creative tool (Krita, Photoshop, Blender) defaults dark. System preference (`prefers-color-scheme`) determines initial mode. User can toggle.
- **Contrast:** All text/background combinations must meet WCAG AA (4.5:1 for body text, 3:1 for large text and UI components).

## Spacing
- **Base unit:** 4px
- **Density:** Comfortable — not cramped, not spacious. Settings panels need breathing room but shouldn't waste vertical space.
- **Scale:** 2xs(2px) xs(4px) sm(8px) md(16px) lg(24px) xl(32px) 2xl(48px) 3xl(64px)
- **Usage guidelines:**
  - Between related items (e.g., label → input): sm (8px)
  - Between groups (e.g., settings sections): lg (24px)
  - Section padding: lg-xl (24-32px)
  - Page margins: lg (24px) desktop, md (16px) mobile

## Layout
- **Approach:** Grid-disciplined — strict alignment, predictable spacing. Both native and web clients use consistent label-input patterns and grouped sections.
- **Native client (egui):**
  - Window: 660px wide minimum, resizable
  - Hero action (Start/Stop + URL + QR) at top
  - Collapsible settings sections below (Connection, Encoding, Preferences)
  - Log viewer at bottom, collapsible
- **Web client (tablet):**
  - Full-bleed video/canvas as primary surface
  - Settings panel: sidebar (16em) on desktop, bottom sheet on tablet (480-1024px), full-width on phone (<480px)
  - Bottom sheet: half-height default, draggable, tab navigation (Capture | Video | Input | Display)
- **Max content width:** 960px (for any standalone pages like access code entry)
- **Border radius:**
  - sm: 4px — inputs, small cards, badges
  - md: 8px — cards, panels, modals
  - lg: 12px — large containers, mockup frames
  - full: 9999px — pills, toggles, theme toggle

## Motion
- **Approach:** Minimal-functional — only transitions that aid comprehension. No decorative animation. This is a utility tool; motion serves understanding, not delight.
- **Easing:** enter: ease-out / exit: ease-in / move: ease-in-out
- **Duration:**
  - Micro: 50-100ms — hover states, focus rings
  - Short: 150ms — toggle switches, button state changes, color transitions
  - Medium: 200ms — settings panel slide, collapsible section expand/collapse
  - Long: 400ms — theme transition (background/text color)
- **Specific animations:**
  - Settings panel slide-in: 200ms ease-out, translateX (sidebar) or translateY (bottom sheet)
  - Collapsible section: 200ms ease-out, height transition
  - Theme toggle: 200ms on background/color properties
  - Reconnect countdown: no animation, text update only

## Component Patterns

### Buttons
- **Primary:** Accent background (#00aaff), white text. Used for the main action (Start Server).
- **Secondary:** Transparent background, border, text color. Used for secondary actions (Settings, Refresh).
- **Ghost:** No border, muted text. Used for cancel/dismiss.
- **Danger:** Error border, error text. Hover fills with error color + white text. Used for Stop Server.
- **Size:** 10px 20px padding, 14px font, border-radius sm (4px).

### Form Inputs
- Background: page background color (not surface — creates depth)
- Border: border color, transitions to accent on focus
- Error: error border color, red hint text below
- Font: Geist 14px for values, Geist 13px for labels

### Alerts
- Left border (3px) in semantic color
- Surface background
- 13px text
- Used for server events, connection status, errors

### Toggles
- 40x22px, border-radius full
- Off: border color background
- On: accent color background
- White circle indicator, 16px, transitions 150ms

### Log Viewer
- Geist Mono 11px, line-height 1.8
- Background: page background (darker than surface)
- Timestamps in muted text
- Log levels color-coded: INFO=accent, WARN=warning, ERROR=error
- Scrollable, max-height constrained

### QR Code
- Rendered with nearest-neighbor scaling for crisp edges
- Displayed on all platforms (no platform restrictions in egui)
- Encodes full connection URL including access code
- Centered below connection URL in hero section

## Interaction States

Every UI feature must specify these states:

| State | Visual Treatment |
|-------|-----------------|
| Loading | Spinner or "Starting..." text on action element |
| Empty | Warm message + primary action hint (never "No items found.") |
| Error | Inline red text below relevant element (never modal alert dialog) |
| Success | State change on action element + status indicator |
| Partial | Progress indicator or degraded-but-functional display |

### First-Run Experience (Native Client)
On first launch (no saved config): contextual hint text appears below the Start button and settings sections. "Start the server, then scan the QR code on your tablet to connect." Below collapsed sections: "(defaults work for most setups)". Hints disappear after first successful server start. Stored in config.

### Reconnect Experience (Web Client)
Auto-reconnect with exponential backoff (1s, 2s, 4s, 8s, max 30s). Display: "Connection lost. Reconnecting in Xs... (attempt N of 10)". Manual "Retry Now" button. All settings state preserved across reconnections.

## Decisions Log
| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-03-19 | Initial design system created | Created by /design-consultation based on competitive research (Astropad, Duet Display, Spacedesk) and design review findings |
| 2026-03-19 | Geist + Geist Mono as sole type family | Single-font system for extreme cohesion. Geist is precise/technical, positions Rylus as a precision tool. MIT licensed. |
| 2026-03-19 | Dark mode primary (#1e1e1e) | Matches where artists work (Krita, Photoshop, Blender all dark). Darker than original #303030 for better content contrast. |
| 2026-03-19 | Cyan #00aaff as sole accent | Already established in web client. No competitor uses cyan (Astropad=pink, Duet=blue, Spacedesk=green). Distinctive. |
| 2026-03-19 | Inline errors over alert dialogs | Contextual, non-blocking, modern. Alert dialogs feel like 2005 and cause unnecessary alarm. |
| 2026-03-19 | Bottom sheet for tablet settings | Tablets are the primary device. Side drawer covers 25% of video. Bottom sheet is a well-understood mobile pattern. |
| 2026-03-19 | Hero action layout for native client | Start/Stop is the #1 user action; it should be the #1 visual element. Settings grouped in collapsible sections below. |
| 2026-03-19 | QR code on all platforms | FLTK limitation removed by egui migration. QR quick-connect is one of Rylus's best UX features. |
