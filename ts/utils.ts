// 4.3: Connection quality

export type ConnectionQuality = 'good' | 'fair' | 'poor';

export function calculateConnectionQuality(rttMs: number, frameJitter: number): ConnectionQuality {
    if (rttMs < 50 && frameJitter < 5) return 'good';
    if (rttMs < 150 && frameJitter < 20) return 'fair';
    return 'poor';
}

export function getQualityIndicatorColor(quality: ConnectionQuality): string {
    switch (quality) {
        case 'good': return '#00ff88';
        case 'fair': return '#ffaa00';
        case 'poor': return '#ff4444';
    }
}

// 5.1: Onboarding wizard

export function shouldShowOnboarding(): boolean {
    return !localStorage.getItem('rylus-onboarding-done');
}

export function markOnboardingComplete(): void {
    localStorage.setItem('rylus-onboarding-done', 'true');
}

const ONBOARDING_STEPS = [
    { title: 'Welcome to Rylus', body: 'Select the screen or window you want to capture from the dropdown.' },
    { title: 'Choose Your Input', body: 'Enable mouse, stylus, or touch input. Adjust pressure sensitivity if needed.' },
    { title: 'Start Drawing', body: 'Your input is mirrored in real-time. Press the settings handle (⠿) to adjust later.' },
];

export function createOnboardingOverlay(): HTMLElement | null {
    if (!shouldShowOnboarding()) return null;

    const overlay = document.createElement('div');
    overlay.style.cssText = 'position:fixed;inset:0;background:rgba(0,0,0,0.7);z-index:9999;display:flex;align-items:center;justify-content:center;';

    const card = document.createElement('div');
    card.style.cssText = 'background:#fff;color:#111;border-radius:12px;padding:2em;max-width:400px;width:90%;text-align:center;';
    // The overlay is inserted directly into <body>, outside the page's
    // landmarks (<main>/<aside>). role="dialog" + aria-modal makes it its
    // own accessible region instead of stray unlandmarked content.
    card.setAttribute('role', 'dialog');
    card.setAttribute('aria-modal', 'true');

    const title = document.createElement('h2');
    title.id = 'rylus-onboarding-title';
    title.style.margin = '0 0 0.5em';
    card.appendChild(title);
    card.setAttribute('aria-labelledby', title.id);

    const body = document.createElement('p');
    body.id = 'rylus-onboarding-body';
    body.style.margin = '0 0 1.5em';
    card.appendChild(body);
    card.setAttribute('aria-describedby', body.id);

    const indicators = document.createElement('div');
    indicators.style.cssText = 'display:flex;gap:8px;justify-content:center;margin-bottom:1.5em;';
    ONBOARDING_STEPS.forEach((_, i) => {
        const dot = document.createElement('span');
        dot.style.cssText = `width:10px;height:10px;border-radius:50%;background:${i === 0 ? '#00aaff' : '#ccc'};transition:background 0.2s;`;
        dot.dataset.step = String(i);
        indicators.appendChild(dot);
    });
    card.appendChild(indicators);

    const btnContainer = document.createElement('div');
    card.appendChild(btnContainer);

    let currentStep = 0;

    function renderStep() {
        title.textContent = ONBOARDING_STEPS[currentStep].title;
        body.textContent = ONBOARDING_STEPS[currentStep].body;
        indicators.querySelectorAll('span').forEach((dot, i) => {
            dot.style.background = i <= currentStep ? '#00aaff' : '#ccc';
        });
        btnContainer.innerHTML = '';
        const btn = document.createElement('button');
        btn.type = 'button';
        if (currentStep < ONBOARDING_STEPS.length - 1) {
            btn.textContent = 'Next';
            btn.onclick = () => { currentStep++; renderStep(); };
        } else {
            btn.textContent = 'Done';
            btn.onclick = () => { markOnboardingComplete(); overlay.remove(); };
        }
        btn.className = 'rylus-onboarding-btn';
        btnContainer.appendChild(btn);
    }

    renderStep();
    overlay.appendChild(card);
    return overlay;
}

// 2.1: Jitter buffer helper

export function shouldSeekToLatest(bufferSeconds: number, targetLatencyMs: number, thresholdMs: number = 100): boolean {
    return bufferSeconds > (targetLatencyMs / 1000) + (thresholdMs / 1000);
}

// 5.2: Toast notifications

export function ensureToastContainer(): HTMLElement {
    let container = document.getElementById('rylus-toast-container');
    if (!container) {
        container = document.createElement('div');
        container.id = 'rylus-toast-container';
        container.className = 'rylus-toast-container';
        document.body.appendChild(container);
    }
    return container;
}

export function showToast(message: string, type: 'error' | 'info' | 'success' = 'info', duration: number = 3000): void {
    const container = ensureToastContainer();
    const toast = document.createElement('div');
    toast.className = `rylus-toast ${type}`;
    toast.textContent = message;
    container.appendChild(toast);

    setTimeout(() => {
        toast.classList.add('fade-out');
        setTimeout(() => toast.remove(), 300);
    }, duration);
}

// 3.1: Pressure curve customization

export type PressurePreset = 'linear' | 'soft' | 'firm';

export interface PressureCurveConfig {
    preset: PressurePreset;
    controlPoint: { x: number; y: number };
}

export const PRESSURE_PRESETS: Record<PressurePreset, PressureCurveConfig> = {
    linear: { preset: 'linear', controlPoint: { x: 0.5, y: 0.5 } },
    soft: { preset: 'soft', controlPoint: { x: 0.5, y: 0.2 } },
    firm: { preset: 'firm', controlPoint: { x: 0.5, y: 0.8 } },
};

export function applyPressureCurve(pressure: number, config: PressureCurveConfig): number {
    if (config.preset === 'linear') return pressure;
    // Quadratic bezier: P0=(0,0), P1=controlPoint, P2=(1,1)
    const t = pressure;
    const cp = config.controlPoint;
    const oneMinusT = 1 - t;
    return oneMinusT * oneMinusT * 0 + 2 * oneMinusT * t * cp.y + t * t * 1;
}

// 3.4: Client-side palm rejection

export function createPalmRejector(suppressionMs: number = 100): {
    onPenEvent: () => void;
    shouldRejectTouch: () => boolean;
} {
    let lastPenEventTime = 0;
    return {
        onPenEvent: () => { lastPenEventTime = Date.now(); },
        shouldRejectTouch: () => Date.now() - lastPenEventTime < suppressionMs,
    };
}
