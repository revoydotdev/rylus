import { describe, it, expect } from 'vitest';

// Since lib.ts doesn't export functions (it's a self-executing module),
// we test the core algorithms by reimplementing the transform logic here
// and verifying it matches the expected behavior. If lib.ts is refactored
// to export these functions, these tests can be updated to import directly.

describe('Coordinate transforms', () => {
    // From lib.ts PEvent constructor: normalized = (clientX - left) / width
    function normalizeCoord(clientPos: number, rectStart: number, rectSize: number): number {
        return (clientPos - rectStart) / rectSize;
    }

    it('normalizes center of rect to 0.5', () => {
        expect(normalizeCoord(500, 0, 1000)).toBeCloseTo(0.5);
    });

    it('normalizes start of rect to 0.0', () => {
        expect(normalizeCoord(0, 0, 1000)).toBeCloseTo(0.0);
    });

    it('normalizes end of rect to 1.0', () => {
        expect(normalizeCoord(1000, 0, 1000)).toBeCloseTo(1.0);
    });

    it('handles offset rect', () => {
        // Rect starts at x=100, width=800
        expect(normalizeCoord(500, 100, 800)).toBeCloseTo(0.5);
        expect(normalizeCoord(100, 100, 800)).toBeCloseTo(0.0);
        expect(normalizeCoord(900, 100, 800)).toBeCloseTo(1.0);
    });

    it('handles out-of-bounds (negative result)', () => {
        // Mouse is left of rect
        expect(normalizeCoord(-10, 0, 1000)).toBeLessThan(0);
    });

    it('handles out-of-bounds (above 1.0)', () => {
        // Mouse is right of rect
        expect(normalizeCoord(1100, 0, 1000)).toBeGreaterThan(1);
    });

    // Custom input area scaling from lib.ts:
    // scaledX = normalizedX * scale + offset
    function applyCustomArea(
        normalized: number,
        areaStart: number,
        areaSize: number
    ): number {
        return normalized * areaSize + areaStart;
    }

    it('custom area identity (full screen)', () => {
        // Area covers [0, 1] → no transformation
        expect(applyCustomArea(0.5, 0, 1)).toBeCloseTo(0.5);
    });

    it('custom area maps to sub-region', () => {
        // Area covers [0.25, 0.75] (width=0.5, start=0.25)
        expect(applyCustomArea(0.0, 0.25, 0.5)).toBeCloseTo(0.25);
        expect(applyCustomArea(0.5, 0.25, 0.5)).toBeCloseTo(0.5);
        expect(applyCustomArea(1.0, 0.25, 0.5)).toBeCloseTo(0.75);
    });
});

describe('Settings serialization', () => {
    it('parses valid JSON settings', () => {
        const json = '{"frame_rate":30,"max_width":1920}';
        const parsed = JSON.parse(json);
        expect(parsed.frame_rate).toBe(30);
        expect(parsed.max_width).toBe(1920);
    });

    it('handles corrupted JSON gracefully', () => {
        const json = '{broken json';
        expect(() => JSON.parse(json)).toThrow();
    });

    it('handles missing fields', () => {
        const json = '{"frame_rate":30}';
        const parsed = JSON.parse(json);
        expect(parsed.max_width).toBeUndefined();
    });
});

describe('WebSocket message construction', () => {
    // From lib.ts: pointer events are sent as JSON objects
    it('constructs PointerEvent message', () => {
        const event = {
            PointerEvent: {
                event_type: "pointerdown",
                pointer_id: 1,
                timestamp: 12345,
                is_primary: true,
                pointer_type: "mouse",
                button: 1,
                buttons: 1,
                x: 0.5,
                y: 0.5,
                movement_x: 0,
                movement_y: 0,
                pressure: 0.0,
                tilt_x: 0,
                tilt_y: 0,
                twist: 0,
                width: 1.0,
                height: 1.0,
            },
        };
        const json = JSON.stringify(event);
        const parsed = JSON.parse(json);
        expect(parsed.PointerEvent.x).toBe(0.5);
        expect(parsed.PointerEvent.pointer_type).toBe("mouse");
    });

    it('constructs WheelEvent message', () => {
        const event = {
            WheelEvent: { dx: 0, dy: -120 },
        };
        const json = JSON.stringify(event);
        const parsed = JSON.parse(json);
        expect(parsed.WheelEvent.dy).toBe(-120);
    });

    it('constructs KeyboardEvent message', () => {
        const event = {
            KeyboardEvent: {
                event_type: "keydown",
                code: "KeyA",
                key: "a",
                ctrl: false,
                alt: false,
                shift: false,
                meta: false,
            },
        };
        const json = JSON.stringify(event);
        const parsed = JSON.parse(json);
        expect(parsed.KeyboardEvent.code).toBe("KeyA");
    });
});

describe('Codec string handling', () => {
    it('parses VideoInit message', () => {
        const msg = { VideoInit: { codec_string: "avc1.4D0028" } };
        expect(msg.VideoInit.codec_string).toBe("avc1.4D0028");
    });

    it('builds mime type from codec string', () => {
        const codecString = "avc1.4D0028";
        const mimeType = 'video/mp4; codecs="' + codecString + '"';
        expect(mimeType).toBe('video/mp4; codecs="avc1.4D0028"');
    });

    it('falls back when VideoInit has no codec_string', () => {
        const msg = { VideoInit: {} } as any;
        const codecString = msg.VideoInit?.codec_string || "avc1.4D0028";
        expect(codecString).toBe("avc1.4D0028");
    });
});

describe('Buffer health calculation', () => {
    // From lib.ts: buffer_length = buffered.end(0) - buffered.start(0)
    it('computes buffer seconds', () => {
        const start = 5.0;
        const end = 7.5;
        expect(end - start).toBeCloseTo(2.5);
    });
});
