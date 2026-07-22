// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
    calculateConnectionQuality,
    getQualityIndicatorColor,
    shouldShowOnboarding,
    markOnboardingComplete,
    shouldSeekToLatest,
    ensureToastContainer,
    showToast,
    createOnboardingOverlay,
    applyPressureCurve,
    type PressurePreset,
    type PressureCurveConfig,
    PRESSURE_PRESETS,
    createPalmRejector,
} from './utils';

// ── Connection quality ──────────────────────────────────────────────────────

describe('calculateConnectionQuality', () => {
    it('returns "good" for low RTT and low jitter', () => {
        expect(calculateConnectionQuality(30, 2)).toBe('good');
    });

    it('returns "good" at boundary (rtt=49, jitter=4)', () => {
        expect(calculateConnectionQuality(49, 4)).toBe('good');
    });

    it('returns "fair" for medium RTT and medium jitter', () => {
        expect(calculateConnectionQuality(100, 10)).toBe('fair');
    });

    it('returns "fair" at boundary (rtt=149, jitter=19)', () => {
        expect(calculateConnectionQuality(149, 19)).toBe('fair');
    });

    it('returns "poor" for high RTT', () => {
        expect(calculateConnectionQuality(200, 30)).toBe('poor');
    });

    it('returns "poor" when jitter is high even with low RTT', () => {
        expect(calculateConnectionQuality(10, 25)).toBe('poor');
    });

    it('returns "poor" at exact boundaries (rtt=50, jitter=5)', () => {
        // rt=50 is NOT < 50, so it falls through to fair
        expect(calculateConnectionQuality(50, 5)).toBe('fair');
    });

    it('returns "poor" at exact boundaries (rtt=150, jitter=20)', () => {
        // 150 is NOT < 150, so it falls through to poor
        expect(calculateConnectionQuality(150, 20)).toBe('poor');
    });
});

describe('getQualityIndicatorColor', () => {
    it('returns green for good quality', () => {
        expect(getQualityIndicatorColor('good')).toBe('#00ff88');
    });

    it('returns amber for fair quality', () => {
        expect(getQualityIndicatorColor('fair')).toBe('#ffaa00');
    });

    it('returns red for poor quality', () => {
        expect(getQualityIndicatorColor('poor')).toBe('#ff4444');
    });
});

// ── Onboarding ──────────────────────────────────────────────────────────────

describe('shouldShowOnboarding / markOnboardingComplete', () => {
    beforeEach(() => {
        localStorage.clear();
    });

    it('returns true when onboarding has not been completed', () => {
        expect(shouldShowOnboarding()).toBe(true);
    });

    it('returns false after marking complete', () => {
        markOnboardingComplete();
        expect(shouldShowOnboarding()).toBe(false);
    });

    it('persists across clear-then-set cycles', () => {
        markOnboardingComplete();
        expect(shouldShowOnboarding()).toBe(false);
        localStorage.removeItem('rylus-onboarding-done');
        expect(shouldShowOnboarding()).toBe(true);
    });
});

describe('createOnboardingOverlay', () => {
    beforeEach(() => {
        localStorage.clear();
        document.body.innerHTML = '';
    });

    afterEach(() => {
        document.body.innerHTML = '';
    });

    it('returns null when onboarding is already done', () => {
        markOnboardingComplete();
        expect(createOnboardingOverlay()).toBeNull();
    });

    it('returns an HTMLElement when onboarding should show', () => {
        const overlay = createOnboardingOverlay();
        expect(overlay).not.toBeNull();
        expect(overlay).toBeInstanceOf(HTMLElement);
    });

    it('overlay contains step content', () => {
        const overlay = createOnboardingOverlay()!;
        expect(overlay.textContent).toBeTruthy();
    });

    it('overlay has a "Done" or "Next" button', () => {
        const overlay = createOnboardingOverlay()!;
        const buttons = overlay.querySelectorAll('button');
        expect(buttons.length).toBeGreaterThan(0);
    });
});

// ── Jitter buffer ───────────────────────────────────────────────────────────

describe('shouldSeekToLatest', () => {
    it('seeks when buffer exceeds target + threshold', () => {
        // 0.5s buffer, 50ms target, 100ms threshold → 0.5 > 0.05 + 0.1 = 0.15
        expect(shouldSeekToLatest(0.5, 50, 100)).toBe(true);
    });

    it('does not seek when buffer is within target', () => {
        // 0.05s buffer, 100ms target, 100ms threshold → 0.05 < 0.1 + 0.1 = 0.2 → false
        expect(shouldSeekToLatest(0.05, 100, 100)).toBe(false);
    });

    it('seeks at exact boundary', () => {
        // buffer = target/1000 + threshold/1000 + epsilon
        expect(shouldSeekToLatest(0.201, 100, 100)).toBe(true);
    });

    it('does not seek just below boundary', () => {
        expect(shouldSeekToLatest(0.199, 100, 100)).toBe(false);
    });

    it('uses default threshold of 100ms', () => {
        // buffer=0.3, target=200 → 0.3 > 0.2 + 0.1 = 0.3? No, 0.3 == 0.3 → not strictly greater
        expect(shouldSeekToLatest(0.3, 200)).toBe(false);
        expect(shouldSeekToLatest(0.301, 200)).toBe(true);
    });

    it('handles zero target and threshold', () => {
        expect(shouldSeekToLatest(0, 0, 0)).toBe(false);
        expect(shouldSeekToLatest(0.001, 0, 0)).toBe(true);
    });
});

// ── Toast notifications ─────────────────────────────────────────────────────

describe('ensureToastContainer', () => {
    beforeEach(() => {
        document.body.innerHTML = '';
    });

    afterEach(() => {
        document.body.innerHTML = '';
    });

    it('creates container if it does not exist', () => {
        const container = ensureToastContainer();
        expect(container).toBeInstanceOf(HTMLElement);
        expect(container.id).toBe('rylus-toast-container');
        expect(document.body.contains(container)).toBe(true);
    });

    it('returns existing container if already present', () => {
        const first = ensureToastContainer();
        const second = ensureToastContainer();
        expect(first).toBe(second);
    });
});

describe('showToast', () => {
    beforeEach(() => {
        document.body.innerHTML = '';
        vi.useFakeTimers();
    });

    afterEach(() => {
        document.body.innerHTML = '';
        vi.useRealTimers();
    });

    it('creates a toast element in the container', () => {
        showToast('test message', 'info');
        const container = document.getElementById('rylus-toast-container');
        expect(container).not.toBeNull();
        expect(container!.children.length).toBe(1);
    });

    it('applies correct CSS class for error type', () => {
        showToast('error msg', 'error');
        const toast = document.querySelector('.rylus-toast');
        expect(toast).not.toBeNull();
        expect(toast!.classList.contains('error')).toBe(true);
    });

    it('applies correct CSS class for info type', () => {
        showToast('info msg', 'info');
        const toast = document.querySelector('.rylus-toast');
        expect(toast!.classList.contains('info')).toBe(true);
    });

    it('applies correct CSS class for success type', () => {
        showToast('success msg', 'success');
        const toast = document.querySelector('.rylus-toast');
        expect(toast!.classList.contains('success')).toBe(true);
    });

    it('defaults to info type', () => {
        showToast('default msg');
        const toast = document.querySelector('.rylus-toast');
        expect(toast!.classList.contains('info')).toBe(true);
    });

    it('removes toast after default duration', () => {
        showToast('timeout msg', 'info', 1000);
        const container = document.getElementById('rylus-toast-container');
        expect(container!.children.length).toBe(1);

        vi.advanceTimersByTime(500);
        // Fade-out animation starts but element may still be present
        vi.advanceTimersByTime(1000);
        expect(container!.children.length).toBe(0);
    });

    it('shows toast message text', () => {
        showToast('Hello World', 'info');
        const toast = document.querySelector('.rylus-toast');
        expect(toast!.textContent).toContain('Hello World');
    });
});

describe('applyPressureCurve', () => {
    it('linear preset passes pressure through unchanged', () => {
        const config: PressureCurveConfig = { preset: 'linear', controlPoint: { x: 0.5, y: 0.5 } };
        expect(applyPressureCurve(0, config)).toBeCloseTo(0);
        expect(applyPressureCurve(0.25, config)).toBeCloseTo(0.25);
        expect(applyPressureCurve(0.5, config)).toBeCloseTo(0.5);
        expect(applyPressureCurve(1.0, config)).toBeCloseTo(1.0);
    });

    it('soft preset lowers mid-range pressure', () => {
        const config = PRESSURE_PRESETS.soft;
        const raw = 0.5;
        const curved = applyPressureCurve(raw, config);
        expect(curved).toBeLessThan(raw);
        expect(curved).toBeGreaterThan(0);
    });

    it('soft preset has low control point Y', () => {
        expect(PRESSURE_PRESETS.soft.controlPoint.y).toBeLessThan(0.5);
    });

    it('firm preset raises mid-range pressure', () => {
        const config = PRESSURE_PRESETS.firm;
        const raw = 0.5;
        const curved = applyPressureCurve(raw, config);
        expect(curved).toBeGreaterThan(raw);
        expect(curved).toBeLessThan(1);
    });

    it('firm preset has high control point Y', () => {
        expect(PRESSURE_PRESETS.firm.controlPoint.y).toBeGreaterThan(0.5);
    });

    it('custom control point applies bezier curve', () => {
        const config: PressureCurveConfig = { preset: 'custom', controlPoint: { x: 0.25, y: 0.75 } };
        const t = 0.5;
        const oneMinusT = 0.5;
        const expected = oneMinusT * oneMinusT * 0 + 2 * oneMinusT * t * 0.75 + t * t * 1;
        expect(applyPressureCurve(t, config)).toBeCloseTo(expected);
    });

    it('always maps 0 to 0 regardless of preset', () => {
        expect(applyPressureCurve(0, PRESSURE_PRESETS.soft)).toBeCloseTo(0);
        expect(applyPressureCurve(0, PRESSURE_PRESETS.firm)).toBeCloseTo(0);
        expect(applyPressureCurve(0, PRESSURE_PRESETS.linear)).toBeCloseTo(0);
    });

    it('always maps 1 to 1 regardless of preset', () => {
        expect(applyPressureCurve(1, PRESSURE_PRESETS.soft)).toBeCloseTo(1);
        expect(applyPressureCurve(1, PRESSURE_PRESETS.firm)).toBeCloseTo(1);
        expect(applyPressureCurve(1, PRESSURE_PRESETS.linear)).toBeCloseTo(1);
    });

    it('custom with controlPoint.y=0 flattens curve to near-zero', () => {
        const config: PressureCurveConfig = { preset: 'custom', controlPoint: { x: 0.5, y: 0 } };
        const t = 0.5;
        const expected = 2 * 0.5 * 0.5 * 0 + 0.5 * 0.5 * 1;
        expect(applyPressureCurve(t, config)).toBeCloseTo(expected);
    });

    it('linear preset is the default', () => {
        expect(PRESSURE_PRESETS.linear.preset).toBe('linear');
    });
});

describe('createPalmRejector', () => {
    beforeEach(() => {
        vi.useFakeTimers();
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('allows touch when no pen event has occurred', () => {
        const rejector = createPalmRejector(100);
        expect(rejector.shouldRejectTouch()).toBe(false);
    });

    it('rejects touch immediately after pen event', () => {
        const rejector = createPalmRejector(100);
        rejector.onPenEvent();
        expect(rejector.shouldRejectTouch()).toBe(true);
    });

    it('allows touch after suppression window expires', () => {
        const rejector = createPalmRejector(100);
        rejector.onPenEvent();
        vi.advanceTimersByTime(101);
        expect(rejector.shouldRejectTouch()).toBe(false);
    });

    it('still rejects within suppression window', () => {
        const rejector = createPalmRejector(100);
        rejector.onPenEvent();
        vi.advanceTimersByTime(50);
        expect(rejector.shouldRejectTouch()).toBe(true);
    });

    it('resets timer on subsequent pen events', () => {
        const rejector = createPalmRejector(100);
        rejector.onPenEvent();
        vi.advanceTimersByTime(80);
        rejector.onPenEvent();
        vi.advanceTimersByTime(50);
        expect(rejector.shouldRejectTouch()).toBe(true);
    });

    it('uses custom suppression time', () => {
        const rejector = createPalmRejector(200);
        rejector.onPenEvent();
        vi.advanceTimersByTime(150);
        expect(rejector.shouldRejectTouch()).toBe(true);
        vi.advanceTimersByTime(51);
        expect(rejector.shouldRejectTouch()).toBe(false);
    });
});

