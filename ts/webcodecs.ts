// Low-latency decode path that bypasses MSE by parsing the server's fMP4
// stream in JS and feeding H.264 access units straight into a VideoDecoder.
// Skips the ~20–50 ms of container + MSE buffering that MSE imposes.
//
// The parser is targeted at the exact mux profile Rylus produces: ISO base
// media with one trak, one sample per fragment, AVCC length-prefixed NALUs,
// and an empty_moov init segment followed by moof+mdat pairs. It does NOT
// aim to be a general-purpose MP4 parser.

declare global {
    interface Window {
        VideoDecoder?: typeof VideoDecoder;
    }
    class VideoDecoder {
        constructor(init: { output: (frame: VideoFrame) => void; error: (e: Error) => void });
        configure(config: VideoDecoderConfig): void;
        decode(chunk: EncodedVideoChunk): void;
        close(): void;
        state: string;
        static isConfigSupported(config: VideoDecoderConfig): Promise<{ supported: boolean }>;
    }
    interface VideoDecoderConfig {
        codec: string;
        description?: BufferSource;
        codedWidth?: number;
        codedHeight?: number;
        hardwareAcceleration?: "no-preference" | "prefer-hardware" | "prefer-software";
        optimizeForLatency?: boolean;
    }
    class EncodedVideoChunk {
        constructor(init: { type: "key" | "delta"; timestamp: number; duration?: number; data: BufferSource });
    }
    class VideoFrame {
        displayWidth: number;
        displayHeight: number;
        timestamp: number;
        close(): void;
    }
}

/**
 * Box header (size + type). 1-byte large-size form is not expected from the
 * Rylus mux, but handled defensively: a size field of 1 means the real 64-bit
 * size follows; size 0 means "box runs to EOF" (only used on streaming files).
 */
interface BoxHeader {
    start: number;      // offset of the box (first size byte)
    bodyStart: number;  // offset of the body (past size+type)
    end: number;        // exclusive end offset
    type: string;
}

function readBoxHeader(view: DataView, offset: number): BoxHeader | null {
    if (offset + 8 > view.byteLength) return null;
    let size = view.getUint32(offset, false);
    const type = String.fromCharCode(
        view.getUint8(offset + 4),
        view.getUint8(offset + 5),
        view.getUint8(offset + 6),
        view.getUint8(offset + 7),
    );
    let bodyStart = offset + 8;
    if (size === 1) {
        if (offset + 16 > view.byteLength) return null;
        // 64-bit size. JS numbers handle anything up to 2^53-1, fine for fMP4.
        const hi = view.getUint32(offset + 8, false);
        const lo = view.getUint32(offset + 12, false);
        size = hi * 0x1_0000_0000 + lo;
        bodyStart = offset + 16;
    } else if (size === 0) {
        size = view.byteLength - offset;
    }
    return { start: offset, bodyStart, end: offset + size, type };
}

function findBox(view: DataView, start: number, end: number, type: string): BoxHeader | null {
    let offset = start;
    while (offset < end) {
        const h = readBoxHeader(view, offset);
        if (!h) return null;
        if (h.type === type) return h;
        offset = h.end;
    }
    return null;
}

function findDescendant(view: DataView, start: number, end: number, path: string[]): BoxHeader | null {
    let scope = { start, end };
    let found: BoxHeader | null = null;
    for (const type of path) {
        found = findBox(view, scope.start, scope.end, type);
        if (!found) return null;
        scope = { start: found.bodyStart, end: found.end };
    }
    return found;
}

/**
 * Extract the AVCDecoderConfigurationRecord (the `description` that WebCodecs
 * wants for length-prefixed H.264) from a moov box.
 *
 * Path: moov → trak → mdia → minf → stbl → stsd → avc1 → avcC.
 *
 * stsd is a full box (version+flags, then entry_count), and avc1 is a
 * visual SampleEntry whose extra fields sit before its child boxes.
 */
function extractAvcC(view: DataView, moov: BoxHeader): Uint8Array | null {
    // moov/trak/mdia/minf/stbl/stsd
    const stsd = findDescendant(view, moov.bodyStart, moov.end, [
        "trak", "mdia", "minf", "stbl", "stsd",
    ]);
    if (!stsd) return null;

    // stsd full-box: 4 bytes (version+flags) + 4 bytes (entry_count), then entries.
    const entriesStart = stsd.bodyStart + 8;
    if (entriesStart >= stsd.end) return null;
    const avc1 = findBox(view, entriesStart, stsd.end, "avc1")
        ?? findBox(view, entriesStart, stsd.end, "avc3");
    if (!avc1) return null;

    // avc1 visual SampleEntry layout:
    //   6 reserved + 2 data_reference_index
    //   16 reserved
    //   2 width + 2 height
    //   4 horizresolution + 4 vertresolution
    //   4 reserved
    //   2 frame_count
    //   32 compressorname
    //   2 depth + 2 pre_defined
    // = 78 bytes before child boxes.
    const childrenStart = avc1.bodyStart + 78;
    const avcC = findBox(view, childrenStart, avc1.end, "avcC");
    if (!avcC) return null;
    const len = avcC.end - avcC.bodyStart;
    return new Uint8Array(view.buffer, view.byteOffset + avcC.bodyStart, len).slice();
}

/** First NALU type in an AVCC-wrapped access unit. 5 = IDR. */
function firstNaluType(data: Uint8Array, nalLenSize: number): number {
    if (data.byteLength < nalLenSize + 1) return 0;
    return data[nalLenSize] & 0x1f;
}

/** Number of length-prefix bytes encoded in an avcC record. */
function nalLengthSize(avcC: Uint8Array): number {
    // byte 4: configurationVersion(1) — skip. byte 5: profile, 6: profile_compat,
    // 7: level, 8: 6 reserved bits + 2 length-size-minus-one.
    if (avcC.byteLength < 5) return 4;
    return (avcC[4] & 0x03) + 1;
}

export interface WebCodecsPipe {
    /** Feed one WebSocket frame (may be init segment or a moof+mdat pair). */
    feed(buf: ArrayBuffer): void;
    /** Replace the codec description, e.g. after a server-initiated restart. */
    reset(): void;
    /** Render into this canvas element. */
    canvas: HTMLCanvasElement;
    close(): void;
}

export async function webCodecsAvailable(codec: string): Promise<boolean> {
    if (typeof window === "undefined" || !(window as any).VideoDecoder) return false;
    try {
        const res = await (VideoDecoder as any).isConfigSupported({
            codec,
            optimizeForLatency: true,
        });
        return !!res?.supported;
    } catch {
        return false;
    }
}

/**
 * Create a WebCodecs-backed decode pipeline rendering into the given canvas.
 * The caller is responsible for feeding WebSocket binary payloads via
 * `feed(buf)` in the exact order they arrive.
 */
export function createWebCodecsPipe(
    canvas: HTMLCanvasElement,
    codec: string,
    onError: (msg: string) => void,
): WebCodecsPipe {
    const ctx = canvas.getContext("2d", { alpha: false, desynchronized: true });
    let decoder: VideoDecoder | null = null;
    let nalLenSize = 4;
    let timestamp = 0;
    const FRAME_DURATION_US = 33_333; // 30 fps nominal; only advisory.
    let seenKeyframe = false;

    function ensureDecoder(avcC: Uint8Array, codedWidth?: number, codedHeight?: number) {
        if (decoder) {
            try { decoder.close(); } catch { /* already closed */ }
        }
        decoder = new VideoDecoder({
            output: (frame: VideoFrame) => {
                try {
                    if (canvas.width !== frame.displayWidth || canvas.height !== frame.displayHeight) {
                        canvas.width = frame.displayWidth;
                        canvas.height = frame.displayHeight;
                    }
                    // Drawing a VideoFrame is implementation-defined but
                    // widely supported in Canvas 2D + WebGL.
                    ctx?.drawImage(frame as any, 0, 0);
                } finally {
                    frame.close();
                }
            },
            error: (e: Error) => onError(e.message || String(e)),
        });
        decoder.configure({
            codec,
            description: avcC,
            codedWidth,
            codedHeight,
            optimizeForLatency: true,
            hardwareAcceleration: "prefer-hardware",
        });
        nalLenSize = nalLengthSize(avcC);
        seenKeyframe = false;
    }

    function feedInitSegment(view: DataView, moov: BoxHeader) {
        const avcC = extractAvcC(view, moov);
        if (!avcC) {
            onError("webcodecs: could not find avcC in init segment");
            return;
        }
        // Optional: read width/height from tkhd for the decoder hint.
        const tkhd = findDescendant(view, moov.bodyStart, moov.end, ["trak", "tkhd"]);
        let w: number | undefined;
        let h: number | undefined;
        if (tkhd) {
            // tkhd is a full box; version at byte 0 of body.
            const version = view.getUint8(tkhd.bodyStart);
            const base = tkhd.bodyStart + (version === 1 ? 32 : 20) + 4 + 8 + 36 + 2 + 2 + 4;
            // That's a rough offset; rather than chase it precisely, just
            // fall back to undefined if it's out of range.
            if (base + 8 <= tkhd.end) {
                const width_fp = view.getUint32(base, false);
                const height_fp = view.getUint32(base + 4, false);
                w = width_fp >>> 16;
                h = height_fp >>> 16;
            }
        }
        ensureDecoder(avcC, w, h);
    }

    function feedMediaFragment(data: Uint8Array) {
        if (!decoder) return;
        const naluType = firstNaluType(data, nalLenSize);
        const isKey = naluType === 5;
        if (!seenKeyframe && !isKey) {
            // Can't start on a delta frame — wait.
            return;
        }
        seenKeyframe = true;
        try {
            decoder.decode(new EncodedVideoChunk({
                type: isKey ? "key" : "delta",
                timestamp,
                duration: FRAME_DURATION_US,
                data,
            }));
        } catch (e: any) {
            onError(`decode failed: ${e?.message ?? e}`);
        }
        timestamp += FRAME_DURATION_US;
    }

    return {
        canvas,
        feed(buf: ArrayBuffer) {
            const view = new DataView(buf);
            // Walk top-level boxes.
            let offset = 0;
            let pendingMdatData: Uint8Array | null = null;
            while (offset < view.byteLength) {
                const h = readBoxHeader(view, offset);
                if (!h) break;
                if (h.type === "moov") {
                    feedInitSegment(view, h);
                } else if (h.type === "mdat") {
                    const len = h.end - h.bodyStart;
                    pendingMdatData = new Uint8Array(buf, h.bodyStart, len);
                }
                // moof, ftyp, styp, sidx, prft are skipped — their contents
                // aren't needed for a single-track stream because the mdat
                // payload for Rylus is exactly one complete access unit per
                // fragment.
                offset = h.end;
            }
            if (pendingMdatData) feedMediaFragment(pendingMdatData);
        },
        reset() {
            seenKeyframe = false;
            timestamp = 0;
        },
        close() {
            try { decoder?.close(); } catch { /* already closed */ }
            decoder = null;
        },
    };
}
