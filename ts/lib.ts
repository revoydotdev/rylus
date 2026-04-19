import {
    applyPressureCurve,
    calculateConnectionQuality,
    createPalmRejector,
    getQualityIndicatorColor,
    PRESSURE_PRESETS,
    PressurePreset,
    showToast,
} from "./utils";

interface Window {
    ManagedMediaSource: any;
}

enum LogLevel {
    ERROR = 0,
    WARN,
    INFO,
    DEBUG,
    TRACE,
}

let log_pre: HTMLPreElement;
let log_level: LogLevel = LogLevel.ERROR;
let no_log_messages: boolean = true;

let fps_out: HTMLOutputElement;
let frame_count = 0;
let last_fps_calc: number = performance.now();

let check_video: HTMLInputElement;

let activeWebSocket: WebSocket | null = null;

// Connection-quality telemetry: recorded on Heartbeat send, resolved on
// HeartbeatAck arrival. Drives the corner pip that tells the user whether
// their network or the server is the cause of any lag.
let last_heartbeat_sent_at: number | null = null;
let current_rtt_ms: number | null = null;
const rtt_samples: number[] = [];

function wsSend(data: string) {
    if (activeWebSocket && activeWebSocket.readyState === WebSocket.OPEN) {
        activeWebSocket.send(data);
    }
}

function send_heartbeat() {
    last_heartbeat_sent_at = performance.now();
    wsSend('"Heartbeat"');
}

function on_heartbeat_ack() {
    if (last_heartbeat_sent_at === null) return;
    const rtt = performance.now() - last_heartbeat_sent_at;
    last_heartbeat_sent_at = null;
    current_rtt_ms = rtt;
    rtt_samples.push(rtt);
    if (rtt_samples.length > 8) rtt_samples.shift();
    update_connection_pip();
}

function rtt_jitter_ms(): number {
    if (rtt_samples.length < 2) return 0;
    const mean = rtt_samples.reduce((a, b) => a + b, 0) / rtt_samples.length;
    const variance = rtt_samples.reduce((s, x) => s + (x - mean) ** 2, 0) / rtt_samples.length;
    return Math.sqrt(variance);
}

function update_connection_pip() {
    const pip = document.getElementById("conn_quality");
    if (!pip || current_rtt_ms === null) return;
    const quality = calculateConnectionQuality(current_rtt_ms, rtt_jitter_ms());
    pip.style.background = getQualityIndicatorColor(quality);
    pip.title = `${quality} — RTT ${Math.round(current_rtt_ms)} ms, jitter ${Math.round(rtt_jitter_ms())} ms`;
    pip.dataset.quality = quality;
}

function run(level: string) {
    window.onload = () => {
        log_pre = document.getElementById("log") as HTMLPreElement;
        log_pre.textContent = "";
        log_level = LogLevel[level];
        fps_out = document.getElementById("fps") as HTMLOutputElement;
        check_video = document.getElementById("enable_video") as HTMLInputElement;
        window.addEventListener("error", (e: ErrorEvent | Event | UIEvent) => {
            if ((e as ErrorEvent).error) {
                let err = e as ErrorEvent;
                log(LogLevel.ERROR, err.filename + ":L" + err.lineno + ":" + err.colno + ": " + err.message + " Error object: " + JSON.stringify(err.error));
            } else if ((e as Event | UIEvent).target) {
                let ev = e as Event;
                let src = (e.target as any).src;
                if (ev.target instanceof HTMLVideoElement)
                    log(LogLevel.ERROR, "Failed to decode video, try reducing resolution or disabling hardware acceleration and reload the page. Error src: " + src);
                else
                    log(LogLevel.ERROR, "Failed to obtain resource, target: " + ev.target + " type: " + ev.type + " src: " + src + " Error object: " + JSON.stringify(ev));
            } else {
                log(LogLevel.WARN, "Got unknown event: " + JSON.stringify(e));
            }
            return false;
        }, true)
        init();
    };
}

function log(level: LogLevel, msg: string) {
    if (level > log_level)
        return;

    if (level == LogLevel.TRACE)
        console.trace(msg);
    else if (level == LogLevel.DEBUG)
        console.debug(msg);
    else if (level == LogLevel.INFO)
        console.info(msg);
    else if (level == LogLevel.WARN)
        console.warn(msg);
    else if (level == LogLevel.ERROR)
        console.error(msg);

    if (no_log_messages) {
        no_log_messages = false;
        document.getElementById("log_section").classList.remove("hide");
    }
    log_pre.textContent += LogLevel[level] + ": " + msg + "\n";
}

function frame_rate_scale(x: number) {
    return Math.pow(x / 100, 1.5);
}

function frame_rate_scale_inv(x: number) {
    return 100 * Math.pow(x, 2 / 3);
}


function calc_max_video_resolution(scale: number) {
    return [
        Math.round(scale * window.innerWidth * window.devicePixelRatio),
        Math.round(scale * window.innerHeight * window.devicePixelRatio)
    ];
}

function fresh_canvas() {
    let canvas_old = document.getElementById("canvas");
    let canvas = document.createElement("canvas");
    canvas.id = canvas_old.id;
    canvas_old.classList.forEach((cls) => canvas.classList.add(cls));
    canvas_old.replaceWith(canvas);
    return canvas;
}

class Rect {
    x: number;
    y: number;
    w: number;
    h: number;
}

class CustomInputAreas {
    mouse: Rect;
    touch: Rect;
    pen: Rect;
}

class Settings {
    webSocket: WebSocket;
    checks: Map<string, HTMLInputElement>;
    capturable_select: HTMLSelectElement;
    frame_rate_input: HTMLInputElement;
    frame_rate_output: HTMLOutputElement;
    scale_video_input: HTMLInputElement;
    scale_video_output: HTMLOutputElement;
    range_min_pressure: HTMLInputElement;
    check_aggressive_seek: HTMLInputElement;
    client_name_input: HTMLInputElement;
    pressure_curve_select: HTMLSelectElement | null = null;
    pressure_curve_preset: PressurePreset = "linear";
    visible: boolean;
    custom_input_areas: CustomInputAreas;
    settings: HTMLElement;

    constructor(webSocket: WebSocket) {
        this.webSocket = webSocket;
        this.checks = new Map<string, HTMLInputElement>();
        this.capturable_select = document.getElementById("window") as HTMLSelectElement;
        this.frame_rate_input = document.getElementById("frame_rate") as HTMLInputElement;
        this.frame_rate_input.min = frame_rate_scale_inv(0).toString();
        this.frame_rate_input.max = frame_rate_scale_inv(120).toString();
        this.frame_rate_output = this.frame_rate_input.nextElementSibling as HTMLOutputElement;
        this.scale_video_input = document.getElementById("scale_video") as HTMLInputElement;
        this.scale_video_output = this.scale_video_input.nextElementSibling as HTMLOutputElement;
        this.range_min_pressure = document.getElementById("min_pressure") as HTMLInputElement;
        this.client_name_input = document.getElementById("client_name") as HTMLInputElement;
        this.pressure_curve_select = document.getElementById(
            "pressure_curve",
        ) as HTMLSelectElement | null;
        if (this.pressure_curve_select) {
            this.pressure_curve_select.onchange = () => {
                this.pressure_curve_preset = this.pressure_curve_select!.value as PressurePreset;
                this.save_settings();
            };
        }
        this.frame_rate_input.oninput = () => {
            this.frame_rate_output.value = Math.round(frame_rate_scale(this.frame_rate_input.valueAsNumber)).toString();
        }
        this.scale_video_input.oninput = () => {
            let [w, h] = calc_max_video_resolution(this.scale_video_input.valueAsNumber)
            this.scale_video_output.value = w + "x" + h
        }
        this.visible = true;

        // Settings UI
        this.settings = document.getElementById("settings");
        this.settings.onclick = (e) => e.stopPropagation();
        let handle = document.getElementById("handle");

        // Settings elements
        this.settings.querySelectorAll("input[type=checkbox]").forEach(
            (elem, _key, _parent) => this.checks.set(elem.id, elem as HTMLInputElement)
        );

        this.load_settings();

        // event handling

        // client only
        handle.onclick = () => { this.toggle() };
        this.checks.get("lefty").onchange = (e) => {
            if ((e.target as HTMLInputElement).checked)
                this.settings.classList.add("lefty");
            else
                this.settings.classList.remove("lefty");
            this.save_settings();
        }

        document.getElementById("vanish").onclick = () => {
            this.settings.classList.add("vanish");
        }

        this.checks.get("stretch").onchange = () => {
            stretch_video();
            this.save_settings();
        };

        this.checks.get("enable_debug_overlay").onchange = (e) => {
            let enabled = (e.target as HTMLInputElement).checked;
            if (enabled) {
                debug_overlay.classList.remove("hide");
            } else {
                debug_overlay.classList.add("hide");
            }
            this.save_settings();
        };

        this.check_aggressive_seek = this.checks.get("aggressive_seeking");
        this.check_aggressive_seek.onchange = () => {
            this.save_settings();
        };

        this.checks.get("enable_video").onchange = (e) => {
            let enabled = (e.target as HTMLInputElement).checked;
            document.getElementById("video").classList.toggle("vanish", !enabled);
            document.getElementById("canvas").classList.toggle("vanish", enabled);
            this.save_settings();
            if (enabled) {
                wsSend('"ResumeVideo"');
            } else {
                wsSend('"PauseVideo"');
            }
        }

        let upd_pointer = () => {
            this.save_settings();
            new PointerHandler(this.webSocket);
        }
        this.checks.get("enable_mouse").onchange = upd_pointer;
        this.checks.get("enable_stylus").onchange = upd_pointer;
        this.checks.get("enable_touch").onchange = upd_pointer;
        const palm_check = this.checks.get("enable_palm_rejection");
        if (palm_check) palm_check.onchange = () => this.save_settings();

        this.checks.get("energysaving").onchange = (e) => {
            this.save_settings();
            this.toggle_energysaving((e.target as HTMLInputElement).checked);
        };

        this.checks.get("enable_custom_input_areas").onchange = () => {
            this.save_settings();
        };

        this.frame_rate_input.onchange = () => this.save_settings();
        this.range_min_pressure.onchange = () => this.save_settings();

        // server
        let upd_server_config = () => { this.save_settings(); this.send_server_config() };
        this.checks.get("uinput_support").onchange = upd_server_config;
        this.checks.get("capture_cursor").onchange = upd_server_config;
        this.scale_video_input.onchange = upd_server_config;
        this.client_name_input.onchange = upd_server_config;
        this.frame_rate_input.onchange = upd_server_config;

        document.getElementById("refresh").onclick = () => wsSend('"GetCapturableList"');
        document.getElementById("custom_input_areas").onclick = () => {
            wsSend('"ChooseCustomInputAreas"');
        };
        this.capturable_select.onchange = () => this.send_server_config();
    }

    send_server_config() {
        if (this.capturable_select.options.length === 0) {
            log(LogLevel.DEBUG, "No capturables available, skipping config send.");
            showToast(
                "No screens or windows available yet. Tap \"Refresh List\" once the capture source is ready.",
                "info",
                3500,
            );
            return;
        }
        let config = new Object(null);
        config["capturable_id"] = Number(this.capturable_select.value);
        for (const key of [
            "uinput_support",
            "capture_cursor"])
            config[key] = this.checks.get(key).checked;
        let [w, h] = calc_max_video_resolution(this.scale_video_input.valueAsNumber);
        config["max_width"] = w;
        config["max_height"] = h;
        config["frame_rate"] = frame_rate_scale(this.frame_rate_input.valueAsNumber);
        if (this.client_name_input.value)
            config["client_name"] = this.client_name_input.value;
        wsSend(JSON.stringify({ "Config": config }));
    }

    save_settings() {
        let settings = Object(null);
        for (const [key, elem] of this.checks.entries())
            settings[key] = elem.checked;
        settings["frame_rate"] = frame_rate_scale(this.frame_rate_input.valueAsNumber).toString();
        settings["scale_video"] = this.scale_video_input.value;
        settings["min_pressure"] = this.range_min_pressure.value;
        settings["custom_input_areas"] = this.custom_input_areas;
        settings["client_name"] = this.client_name_input.value;
        settings["pressure_curve"] = this.pressure_curve_preset;
        localStorage.setItem("settings", JSON.stringify(settings));
    }

    load_settings() {
        let settings_string = localStorage.getItem("settings");
        if (settings_string === null) {
            this.frame_rate_input.value = frame_rate_scale_inv(30).toString();
            this.frame_rate_output.value = (30).toString();
            let [w, h] = calc_max_video_resolution(this.scale_video_input.valueAsNumber)
            this.scale_video_output.value = w + "x" + h;
            return;
        }
        try {
            let settings = JSON.parse(settings_string);
            if (typeof settings !== "object" || settings === null) {
                console.warn("Failed to load settings from localStorage: parsed value is not an object");
                return;
            }
            for (const [key, elem] of this.checks.entries()) {
                if (typeof settings[key] === "boolean")
                    elem.checked = settings[key];
            }
            let upd_limit = settings["frame_rate"];
            if (upd_limit)
                this.frame_rate_input.value = frame_rate_scale_inv(upd_limit).toString();
            else
                this.frame_rate_input.value = frame_rate_scale_inv(30).toString();
            this.frame_rate_output.value = Math.round(frame_rate_scale(this.frame_rate_input.valueAsNumber)).toString();

            let scale_video = settings["scale_video"];
            if (scale_video)
                this.scale_video_input.value = scale_video;
            let [w, h] = calc_max_video_resolution(this.scale_video_input.valueAsNumber);
            this.scale_video_output.value = w + "x" + h;

            let min_pressure = settings["min_pressure"];
            if (min_pressure)
                this.range_min_pressure.value = min_pressure;

            this.custom_input_areas = settings["custom_input_areas"];

            if (this.checks.get("lefty").checked) {
                this.settings.classList.add("lefty");
            }

            if (!this.checks.get("enable_video").checked || this.checks.get("energysaving").checked) {
                this.checks.get("enable_video").checked = false;
                if (this.checks.get("energysaving").checked)
                    this.checks.get("enable_video").disabled = true;
                document.getElementById("video").classList.add("vanish");
                document.getElementById("canvas").classList.remove("vanish");
            }

            if (this.checks.get("energysaving").checked) {
                this.toggle_energysaving(true);
            }

            if (this.checks.get("enable_debug_overlay").checked) {
                debug_overlay.classList.remove("hide");
            }


            if (document.getElementById("custom_input_areas").classList.contains("hide")) {
                this.checks.get("enable_custom_input_areas").checked = false;
            }

            let client_name = settings["client_name"];
            if (client_name)
                this.client_name_input.value = client_name;

            let pressure_curve = settings["pressure_curve"];
            if (pressure_curve === "linear" || pressure_curve === "soft" || pressure_curve === "firm") {
                this.pressure_curve_preset = pressure_curve;
                if (this.pressure_curve_select)
                    this.pressure_curve_select.value = pressure_curve;
            }

        } catch (e) {
            console.warn("Failed to load settings from localStorage:", e);
            log(LogLevel.DEBUG, "Failed to load settings.")
            return;
        }
    }

    stretched_video() {
        return this.checks.get("stretch").checked
    }

    pointer_types() {
        let ptrs = [];
        if (this.checks.get("enable_mouse").checked)
            ptrs.push("mouse");
        if (this.checks.get("enable_stylus").checked)
            ptrs.push("pen");
        if (this.checks.get("enable_touch").checked)
            ptrs.push("touch");
        return ptrs;
    }

    toggle() {
        this.settings.classList.toggle("hide");
        this.visible = !this.visible;
    }

    onCapturableList(window_names: string[]) {
        let current_selection = undefined;
        if (this.capturable_select.selectedOptions[0])
            current_selection = this.capturable_select.selectedOptions[0].textContent;
        let new_index: number;
        this.capturable_select.innerText = "";
        window_names.forEach((name, i) => {
            let option = document.createElement("option");
            option.value = String(i);
            option.innerText = name;
            this.capturable_select.appendChild(option);
            if (name === current_selection)
                new_index = i;
        });
        if (new_index !== undefined)
            this.capturable_select.value = String(new_index);
        else if (current_selection)
            // Can't find the window, so don't select anything
            this.capturable_select.value = "";

        // Auto-send config now that capturables are available
        if (window_names.length > 0) {
            this.send_server_config();
        } else {
            // Surface empty-state guidance instead of silently leaving the
            // dropdown blank. Covers Wayland portal waiting for the user to
            // approve the request, and capture backends that haven't
            // enumerated windows yet.
            showToast(
                "No capture sources found. On Wayland, approve the screen-capture prompt on your computer, then tap \"Refresh List\".",
                "info",
                5000,
            );
        }
    }

    toggle_energysaving(energysaving: boolean) {
        let canvas = fresh_canvas();
        if (energysaving) {
            let ctx = canvas.getContext("2d");
            ctx.fillStyle = "#000";
            ctx.fillRect(0, 0, canvas.width, canvas.height);
        }

        if (energysaving) {
            this.checks.get("enable_video").checked = false;
            this.checks.get("enable_video").disabled = true;
            this.checks.get("enable_video").dispatchEvent(new Event("change"));
        } else {
            this.checks.get("enable_video").disabled = false;
            this.checks.get("enable_video").checked = true;
            this.checks.get("enable_video").dispatchEvent(new Event("change"));
        }
        if (settings)
            new PointerHandler(this.webSocket);
    }

    video_enabled(): boolean {
        return this.checks.get("enable_video").checked;
    }
}

let settings: Settings;
let debug_overlay: HTMLElement;
let last_pointer_data: Object;

// The currently-active Painter (if any). Exposed at module scope so the
// reconnect handler can drop locally-drawn partial strokes when the
// WebSocket goes down — without a reference the user would keep dragging
// a visible line the server never received.
let active_painter: Painter | null = null;

// Palm rejection: kept at module scope so the 100ms suppression window
// survives PointerHandler re-instantiation when the user toggles input
// checkboxes. `createPalmRejector` is defined in utils.ts.
const palm_rejector = createPalmRejector(100);

class PEvent {
    event_type: string;
    pointer_id: number;
    timestamp: number;
    is_primary: boolean;
    pointer_type: string;
    button: number;
    buttons: number;
    x: number;
    y: number;
    // movement_x: number;
    // movement_y: number;
    pressure: number;
    tilt_x: number;
    tilt_y: number;
    twist: number;
    width: number;
    height: number;

    constructor(eventType: string, event: PointerEvent, targetRect: DOMRect) {
        let diag_len = Math.sqrt(targetRect.width * targetRect.width + targetRect.height * targetRect.height) || 1
        this.event_type = eventType.toString();
        this.pointer_id = event.pointerId;
        this.timestamp = Math.round(event.timeStamp * 1000);
        this.is_primary = event.isPrimary;
        this.pointer_type = event.pointerType;
        let btn = event.button;
        // for some reason the secondary and auxiliary buttons are ordered differently for
        // the button and buttons properties
        if (btn == 2)
            btn = 1;
        else if (btn == 1)
            btn = 2;
        this.button = (btn < 0 ? 0 : 1 << btn);
        this.buttons = event.buttons;
        let x_offset = 0;
        let y_offset = 0;
        let x_scale = 1;
        let y_scale = 1;
        if (settings.checks.get("enable_custom_input_areas").checked) {
            let custom_input_area: Rect = null;
            if (event.pointerType == "mouse") {
                custom_input_area = settings.custom_input_areas.mouse;
            } else if (event.pointerType == "touch") {
                custom_input_area = settings.custom_input_areas.touch;
            } else if (event.pointerType == "pen") {
                custom_input_area = settings.custom_input_areas.pen;
            }
            if (custom_input_area) {
                x_scale = custom_input_area.w;
                y_scale = custom_input_area.h;
                x_offset = custom_input_area.x;
                y_offset = custom_input_area.y;
            }
        }
        this.x = (event.clientX - targetRect.left) / targetRect.width * x_scale + x_offset;
        this.y = (event.clientY - targetRect.top) / targetRect.height * y_scale + y_offset;
        // this.movement_x = event.movementX ? event.movementX : 0;
        // this.movement_y = event.movementY ? event.movementY : 0;
        let raw_pressure = event.pressure === 0 ? 0 : Math.max(event.pressure, settings.range_min_pressure.valueAsNumber);
        let curve_preset = settings.pressure_curve_preset;
        if (event.pointerType === "pen" && curve_preset !== "linear" && raw_pressure > 0) {
            raw_pressure = applyPressureCurve(raw_pressure, PRESSURE_PRESETS[curve_preset]);
        }
        this.pressure = raw_pressure;
        this.tilt_x = event.tiltX;
        this.tilt_y = event.tiltY;
        this.width = event.width / diag_len;
        this.height = event.height / diag_len;
        this.twist = event.twist;
    }
}

class WEvent {
    dx: number;
    dy: number;
    timestamp: number;

    constructor(event: WheelEvent) {
        /* The WheelEvent can have different scrolling modes that affect how much scrolling
         * should be done. Unfortunately there is not always a way to accurately convert the scroll
         * distance into pixels. Thus the following is a guesstimate and scales the WheelEvent's
         * deltaX/Y values accordingly.
         */
        let scale = 1;
        switch (event.deltaMode) {
            case 0x01: // DOM_DELTA_LINE
                scale = 10;
                break;
            case 0x02: // DOM_DELTA_PAGE
                scale = 1000;
                break;
            default: // DOM_DELTA_PIXEL
        }
        this.dx = Math.round(scale * event.deltaX);
        this.dy = Math.round(scale * event.deltaY);
        this.timestamp = Math.round(event.timeStamp * 1000);
    }
}

// in milliseconds
const fade_time = 5000;

const vs_source = `
  attribute vec3 aVertex;
  uniform float uTime;
  varying lowp vec4 vColor;

  void main() {
    float dt = uTime - aVertex[2];
    gl_Position = vec4(aVertex[0], aVertex[1], 1.0, 1.0);
    vColor = vec4(0.0, 170.0/255.0, 1.0, 1.0) * max(1.0 - dt/${fade_time}.0, 0.0);
  }
`;

const fs_source = `
  varying lowp vec4 vColor;

  void main() {
    gl_FragColor = vColor;
  }
`;

class Painter {
    canvas: HTMLCanvasElement;
    gl: WebGLRenderingContext;

    /* Store lines currently being drawn.
     *
     * Keys are pointerIds, values are an array of the last position (x, y), thickness and event
     * time and another array with vertices to be used by webgl. Each vertex is made of 3 floats, x
     * and y coordinates and the event time. Vertices always come in pairs of two. Two such vertices
     * describe the edges of the line to be drawn with regard to it's thickness. TRIANGLE_STRIP is
     * then used to connect them and draw an actual line with some thickness depending on the
     * pressure applied.
     */
    lines_active: Map<number, [[number, number, number, number], number[]]>

    // Array of vertices that are not actively drawn anymore and do not need updates, except
    // removing them after they faded away.
    lines_old: number[][];
    vertex_attr: GLint;
    vertex_buffer: WebGLBuffer;
    time_attr: WebGLUniformLocation;
    initialized: boolean;
    animFrameId: number;

    constructor(canvas: HTMLCanvasElement) {
        this.canvas = canvas;
        canvas.width = window.innerWidth * window.devicePixelRatio;
        canvas.height = window.innerHeight * window.devicePixelRatio;
        this.gl = canvas.getContext("webgl");
        if (this.gl) {
            this.lines_active = new Map();
            this.lines_old = [];
            this.setupWebGL();
        }
    }

    loadShader(type, source): WebGLShader {
        let gl = this.gl;
        const shader = gl.createShader(type);
        gl.shaderSource(shader, source);
        gl.compileShader(shader);
        if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
            log(LogLevel.WARN, "Failed to compile shaders: " + gl.getShaderInfoLog(shader));
            gl.deleteShader(shader);
            return null;
        }
        return shader;
    }

    setupWebGL() {
        let gl = this.gl;
        gl.enable(gl.BLEND);
        gl.clearColor(0, 0, 0, 0);
        gl.clear(gl.COLOR_BUFFER_BIT);

        const vertex_shader = this.loadShader(gl.VERTEX_SHADER, vs_source);
        const fragment_shader = this.loadShader(gl.FRAGMENT_SHADER, fs_source);
        if (!vertex_shader || !fragment_shader)
            return;
        const shader_program = gl.createProgram();
        gl.attachShader(shader_program, vertex_shader);
        gl.attachShader(shader_program, fragment_shader);
        gl.linkProgram(shader_program);

        if (!gl.getProgramParameter(shader_program, gl.LINK_STATUS)) {
            log(LogLevel.WARN, "Unable to initialize the shader program: " + gl.getProgramInfoLog(shader_program));
            return;
        }
        this.vertex_attr = gl.getAttribLocation(shader_program, "aVertex");
        this.time_attr = gl.getUniformLocation(shader_program, "uTime");
        this.vertex_buffer = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, this.vertex_buffer);
        gl.vertexAttribPointer(this.vertex_attr, 3, gl.FLOAT, false, 0, 0);
        gl.enableVertexAttribArray(this.vertex_attr);
        gl.useProgram(shader_program);
        this.initialized = true;
        this.animFrameId = requestAnimationFrame(() => this.render());
    }

    render() {
        // only do work if necessary
        // Only render drawing lines when video is disabled — otherwise the video covers the canvas.
        if (!check_video.checked && (this.lines_active.size > 0 || this.lines_old.length > 0)) {
            while (this.lines_old.length > 0 &&
                   (performance.now() - this.lines_old[0][this.lines_old[0].length - 1] > fade_time ||
                    this.lines_old.length > 1000)) {
                this.lines_old.shift();
            }
            let gl = this.gl;
            gl.viewport(0, 0, this.canvas.width, this.canvas.height);
            gl.clear(gl.COLOR_BUFFER_BIT);
            gl.uniform1f(this.time_attr, performance.now());
            gl.bindBuffer(gl.ARRAY_BUFFER, this.vertex_buffer);
            for (let vertices of this.lines_old) {
                gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(vertices), gl.DYNAMIC_DRAW);
                gl.drawArrays(gl.TRIANGLE_STRIP, 0, vertices.length / 3)
            }
            for (let [_, vertices] of this.lines_active.values()) {
                // sometimes there are no linesegments because there has been only a single
                // PointerEvent
                if (vertices.length == 0)
                    continue;
                gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(vertices), gl.DYNAMIC_DRAW);
                gl.drawArrays(gl.TRIANGLE_STRIP, 0, vertices.length / 3)
            }
        }
        this.animFrameId = requestAnimationFrame(() => this.render());
    }

    destroy() {
        if (this.animFrameId) {
            cancelAnimationFrame(this.animFrameId);
            this.animFrameId = 0;
        }
    }

    /// Discard every in-flight stroke and render state. Used on disconnect so
    /// the user doesn't see the local canvas keep ghost-drawing after the
    /// server stopped receiving events.
    cancel_all() {
        if (this.lines_active) this.lines_active.clear();
        if (this.lines_old) this.lines_old.length = 0;
        if (this.gl && this.initialized) {
            this.gl.clear(this.gl.COLOR_BUFFER_BIT);
        }
    }

    appendEventToLine(event: PointerEvent) {
        let line = this.lines_active.get(event.pointerId);
        if (!line) {
            line = [null, []];
            this.lines_active.set(event.pointerId, line)
        }
        let max_pixels = Math.max(this.canvas.width, this.canvas.height);
        let x = event.clientX * window.devicePixelRatio / this.canvas.width * 2 - 1;
        let y = 1 - event.clientY * window.devicePixelRatio / this.canvas.height * 2;
        let delta = event.pressure + 0.4;
        let t = performance.now();
        // to draw a line segment, there has to be some previous position
        if (line[0]) {
            let [x0, y0, delta0, t0] = line[0];
            // get vector perpendicular to the linesegment to calculate quadrangel around the
            // segment with appropriate thickness
            let dx = (y - y0);
            let dy = -(x - x0);
            let dd = Math.sqrt(dx ** 2 + dy ** 2);
            if (dd == 0) {
                return;
            }
            dx = dx / dd * max_pixels / this.canvas.width * 0.004;
            dy = dy / dd * max_pixels / this.canvas.height * 0.004;

            if (line[1].length == 0)
                line[1].push(
                    x0 + delta0 * dx, y0 + delta0 * dy, t0, x0 - delta0 * dx, y0 - delta0 * dy, t0,
                );
            line[1].push(
                x + delta * dx, y + delta * dy, t, x - delta * dx, y - delta * dy, t
            )
        }
        line[0] = [x, y, delta, t];
    }

    onstart(event: PointerEvent) {
        this.appendEventToLine(event);
    }

    onmove(event: PointerEvent) {
        if (this.lines_active.has(event.pointerId)) {
            const events = event.getCoalescedEvents?.() ?? [event];
            for (const e of events) {
                this.appendEventToLine(e);
            }
        }
    }

    onstop(event: PointerEvent) {
        let lines = this.lines_active.get(event.pointerId);
        if (lines) {
            if (lines[1].length > 0)
                this.lines_old.push(lines[1]);
            this.lines_active.delete(event.pointerId);
        }
    }
}

class PointerHandler {
    webSocket: WebSocket;
    pointerTypes: string[];

    constructor(webSocket: WebSocket) {
        let video = document.getElementById("video");
        let canvas = document.getElementById("canvas");
        this.webSocket = webSocket;
        this.pointerTypes = settings.pointer_types();

        video.onpointerdown = (e) => this.onEvent(e, "pointerdown");
        video.onpointerup = (e) => this.onEvent(e, "pointerup");
        video.onpointercancel = (e) => this.onEvent(e, "pointercancel");
        video.onpointermove = (e) => this.onEvent(e, "pointermove");
        video.onpointerout = (e) => this.onEvent(e, "pointerout");
        video.onpointerleave = (e) => this.onEvent(e, "pointerleave");
        video.onpointerenter = (e) => this.onEvent(e, "pointerenter");
        video.onpointerover = (e) => this.onEvent(e, "pointerover");

        let painter: Painter;
        if (!settings.checks.get("energysaving").checked) {
            painter = new Painter(canvas as HTMLCanvasElement);
            active_painter = painter;
        } else {
            active_painter = null;
        }

        if (painter && painter.initialized) {
            canvas.onpointerdown = (e) => { this.onEvent(e, "pointerdown"); painter.onstart(e); };
            canvas.onpointerup = (e) => { this.onEvent(e, "pointerup"); painter.onstop(e); };
            canvas.onpointercancel = (e) => { this.onEvent(e, "pointercancel"); painter.onstop(e); };
            canvas.onpointermove = (e) => { this.onEvent(e, "pointermove"); painter.onmove(e); };
            canvas.onpointerout = (e) => { this.onEvent(e, "pointerout"); painter.onstop(e); };
            canvas.onpointerleave = (e) => { this.onEvent(e, "pointerleave"); painter.onstop(e); };
            canvas.onpointerenter = (e) => { this.onEvent(e, "pointerenter"); painter.onmove(e); };
            canvas.onpointerover = (e) => { this.onEvent(e, "pointerover"); painter.onmove(e); };
        } else {
            canvas.onpointerdown = (e) => this.onEvent(e, "pointerdown");
            canvas.onpointerup = (e) => this.onEvent(e, "pointerup");
            canvas.onpointercancel = (e) => this.onEvent(e, "pointercancel");
            canvas.onpointermove = (e) => this.onEvent(e, "pointermove");
            canvas.onpointerout = (e) => this.onEvent(e, "pointerout");
            canvas.onpointerleave = (e) => this.onEvent(e, "pointerleave");
            canvas.onpointerenter = (e) => this.onEvent(e, "pointerenter");
            canvas.onpointerover = (e) => this.onEvent(e, "pointerover");
        }

        // This is a workaround for the following Safari/WebKit bug:
        // https://bugs.webkit.org/show_bug.cgi?id=217430
        // I have no idea why this works but it does.
        video.ontouchmove = (e) => e.preventDefault();
        canvas.ontouchmove = (e) => e.preventDefault();

        for (let elem of [video, canvas]) {
            elem.onwheel = (e) => {
                wsSend(JSON.stringify({ "WheelEvent": new WEvent(e) }));
            }
        }
    }

    onEvent(event: PointerEvent, event_type: string) {
        if (settings.checks.get("enable_debug_overlay").checked) {
            let props = [
                "altKey",
                "altitudeAngle",
                "azimuthAngle",
                "button",
                "buttons",
                "clientX",
                "clientY",
                "ctrlKey",
                "height",
                "isPrimary",
                "metaKey",
                "movementX",
                "movementY",
                "offsetX",
                "offsetY",
                "pageX",
                "pageY",
                "pointerId",
                "pointerType",
                "pressure",
                "screenX",
                "screenY",
                "shiftKey",
                "tangentialPressure",
                "tiltX",
                "tiltY",
                "timeStamp",
                "twist",
                "type",
                "width",
                "x",
                "y",
            ];
            if (!last_pointer_data) {
                last_pointer_data = {};
                for (let prop of props) {
                    let span_id = `prop_${prop}_span`;
                    let span = document.getElementById(span_id);
                    if (!span) {
                        span = document.createElement("span");
                        span.id = span_id;
                        debug_overlay.appendChild(span);
                        debug_overlay.appendChild(document.createElement("br"));
                    }
                }
            }
            for (let prop of props) {
                let span_id = `prop_${prop}_span`;
                let span = document.getElementById(span_id);
                let v = event[prop];
                span.textContent = `${prop}: ${v}`;
                if (last_pointer_data[prop] == v) {
                    span.classList.remove("updated");
                } else {
                    span.classList.add("updated");
                    last_pointer_data[prop] = v;
                }
            }
        }
        if (this.pointerTypes.includes(event.pointerType)) {
            // Palm rejection: any pen contact "locks out" touch for 100ms. Only
            // active when the user has stylus input enabled — if they've turned
            // pen off and touch on, they probably want raw touch.
            const palm_reject_active =
                settings.checks.get("enable_palm_rejection")?.checked !== false
                && settings.checks.get("enable_stylus")?.checked === true;
            if (palm_reject_active) {
                if (event.pointerType === "pen") {
                    palm_rejector.onPenEvent();
                } else if (event.pointerType === "touch" && palm_rejector.shouldRejectTouch()) {
                    return;
                }
            }
            let rect = (event.target as HTMLElement).getBoundingClientRect();
            const events = event_type === "pointermove" ? (event.getCoalescedEvents?.() ?? [event]) : [event];
            for (let event of events) {
                wsSend(
                    JSON.stringify(
                        {
                            "PointerEvent": new PEvent(
                                event_type,
                                event,
                                rect
                            )
                        }
                    )
                );
            }
            if (settings.visible) {
                settings.toggle();
            }
        }
    }
}

class KEvent {
    event_type: string;
    code: string;
    key: string;
    location: number;
    alt: boolean;
    ctrl: boolean;
    shift: boolean;
    meta: boolean;

    constructor(event_type: string, event: KeyboardEvent) {
        this.event_type = event_type;
        this.code = event.code;
        this.key = event.key;
        this.location = event.location;
        this.alt = event.altKey;
        this.ctrl = event.ctrlKey;
        this.shift = event.shiftKey;
        this.meta = event.metaKey;
    }
}

class KeyboardHandler {
    webSocket: WebSocket;

    constructor(webSocket: WebSocket) {
        this.webSocket = webSocket;

        let d = document;
        let s = document.getElementById("settings")

        // Consume all KeyboardEvents, except the settings menu is open.
        // this avoids making the main/video/canvas element focusable by using
        // a tabindex which interferes with PointerEvent than can be considered
        // hovering.

        function settings_hidden() {
            return s.classList.contains("hide") || s.classList.contains("vanish");
        }

        d.onkeydown = (e) => {
            if (!settings_hidden())
                return true;
            if (e.repeat)
                return this.onEvent(e, "repeat");
            return this.onEvent(e, "down");
        };
        d.onkeyup = (e) => {
            if (!settings_hidden())
                return true;
            return this.onEvent(e, "up");
        };
        d.onkeypress = (e) => {
            if (!settings_hidden())
                return true;
            e.preventDefault();
            e.stopPropagation();
            return false;
        };
    }

    onEvent(event: KeyboardEvent, event_type: string) {
        wsSend(JSON.stringify({ "KeyboardEvent": new KEvent(event_type, event) }));
        event.preventDefault();
        event.stopPropagation();
        return false;
    }
}

function frame_rate_stats() {
    let t = performance.now();
    let fps = Math.round(frame_count / (t - last_fps_calc) * 10000) / 10;
    fps_out.value = fps.toString();
    frame_count = 0;
    last_fps_calc = t;
    setTimeout(() => frame_rate_stats(), 1500);
}

function handle_messages(
    webSocket: WebSocket,
    video: HTMLVideoElement,
    onConfigOk: Function,
    onConfigError: Function,
    onCapturableList: Function,
) {
    let mediaSource: MediaSource = null;
    let sourceBuffer: SourceBuffer = null;
    let queue = [];
    let prevObjectURL: string = null;
    let health_frame_count = 0;
    let last_onerror_restart = 0;
    // Codec string from server's VideoInit message (e.g. "avc1.4D0028").
    // Falls back to a widely-compatible default if the server doesn't send one.
    let serverCodecString: string = "avc1.4D0028";
    const ONERROR_DEBOUNCE_MS = 5000;
    const MAX_BUFFER_LENGTH = 20;  // In seconds
    function upd_buf() {
        if (sourceBuffer == null)
            return;
        if (!sourceBuffer.updating && queue.length > 0 && mediaSource.readyState == "open") {
            let buffer_length = 0;
            if (sourceBuffer.buffered.length) {
                // Assume only one time range...
                buffer_length = sourceBuffer.buffered.end(0) - sourceBuffer.buffered.start(0);
            }
            if (buffer_length > MAX_BUFFER_LENGTH) {
                sourceBuffer.remove(0, sourceBuffer.buffered.end(0) - MAX_BUFFER_LENGTH / 2);
                // This will trigger updateend when finished
            } else {
                try {
                    sourceBuffer.appendBuffer(queue.shift());
                } catch (err) {
                    log(LogLevel.DEBUG, "Error appending to sourceBuffer:" + err);
                    // Drop everything, and try to pick up the stream again
                    try {
                        if (sourceBuffer.updating)
                            sourceBuffer.abort();
                        sourceBuffer.remove(0, Infinity);
                    } catch (e) {
                        // MediaSource may have closed; recovery will happen on NewVideo
                    }
                }
            }
        }
    }
    webSocket.onmessage = (event: MessageEvent) => {
        if (typeof event.data == "string") {
            let msg;
            try {
                msg = JSON.parse(event.data);
            } catch (e) {
                log(LogLevel.ERROR, "Failed to parse message from server: " + e);
                return;
            }
            if (typeof msg == "string") {
                if (msg == "NewVideo") {
                    let MS = window.ManagedMediaSource ? window.ManagedMediaSource : window.MediaSource;
                    mediaSource = new MS();
                    sourceBuffer = null;
                    queue.length = 0;  // Clear stale frames from previous video
                    health_frame_count = 0;
                    if (prevObjectURL) {
                        URL.revokeObjectURL(prevObjectURL);
                    }
                    prevObjectURL = URL.createObjectURL(mediaSource);
                    video.src = prevObjectURL;
                    mediaSource.addEventListener("sourceopen", (_) => {
                        let mimeType = 'video/mp4; codecs="' + serverCodecString + '"';
                        if (!MS.isTypeSupported(mimeType))
                            mimeType = "video/mp4";
                        sourceBuffer = mediaSource.addSourceBuffer(mimeType);
                        sourceBuffer.addEventListener("updateend", upd_buf);
                        // try to recover from errors by restarting the video (debounced)
                        sourceBuffer.onerror = () => {
                            let now = performance.now();
                            if (now - last_onerror_restart > ONERROR_DEBOUNCE_MS) {
                                last_onerror_restart = now;
                                settings.send_server_config();
                            }
                        };
                    })
                } else if (msg == "ConfigOk") {
                    onConfigOk();
                }
            } else if (typeof msg == "object") {
                if ("Hello" in msg) {
                    log(LogLevel.INFO, "Server protocol version: " + msg["Hello"]["protocol_version"]);
                } else if ("HelloNack" in msg) {
                    const nack = msg["HelloNack"];
                    log(LogLevel.ERROR, "Server rejected client protocol: " + JSON.stringify(nack));
                    showToast(
                        (nack && nack.reason) ||
                            "This page is out of date — reloading to pick up the new client.",
                        "error",
                        4500,
                    );
                    setTimeout(() => location.reload(), 2000);
                } else if ("HeartbeatAck" in msg) {
                    on_heartbeat_ack();
                } else if ("VideoInit" in msg) {
                    let vi = msg["VideoInit"];
                    if (vi && vi.codec_string) {
                        serverCodecString = vi.codec_string;
                        log(LogLevel.INFO, "Server codec: " + serverCodecString);
                    }
                } else if ("CapturableList" in msg)
                    onCapturableList(msg["CapturableList"]);
                else if ("Error" in msg)
                    showToast(String(msg["Error"]), "error", 4000);
                else if ("ConfigError" in msg) {
                    onConfigError(msg["ConfigError"]);
                } else if ("CustomInputAreas" in msg) {
                    settings.custom_input_areas = msg["CustomInputAreas"];
                    settings.checks.get("enable_custom_input_areas").checked = true;
                    settings.save_settings();
                }
            }

            return;
        }

        // not a string -> got a video frame
        queue.push(event.data);
        upd_buf();
        frame_count += 1;

        // Report buffer health to server for adaptive quality
        health_frame_count++;
        if (health_frame_count >= 30) {
            health_frame_count = 0;
            if (sourceBuffer && mediaSource && mediaSource.readyState === "open") {
                try {
                    if (sourceBuffer.buffered.length > 0) {
                        let buffer_seconds = sourceBuffer.buffered.end(0) - video.currentTime;
                        wsSend(JSON.stringify({"BufferHealth": {"buffer_seconds": buffer_seconds}}));
                    }
                } catch (e) {
                    // sourceBuffer may be in an invalid state during teardown
                }
            }
        }

        // only seek if there is data available, some browsers choke otherwise
        if (video.seekable.length > 0) {
            let seek_time = video.seekable.end(video.seekable.length - 1);
            if (video.readyState >= (settings.check_aggressive_seek.checked ? 3 : 4)
                // but make sure to catch up if the video is more than 3 seconds behind
                || seek_time - video.currentTime > 3) {
                if (isFinite(seek_time))
                    video.currentTime = seek_time;
                else
                    log(LogLevel.WARN, "Failed to seek to end of video.")
            }

        }
    }
}

function check_apis() {
    let apis = [
        {
            attrs: ["MediaSource", "ManagedMediaSource"],
            msg: "This browser doesn't support MSE/MMS required to playback video stream, try upgrading!"
        },
        {
            attrs: ["PointerEvent"],
            msg: "This browser doesn't support PointerEvents, input will not work, try upgrading!"
        },
    ];

    outer:
    for (let d of apis) {
        for (let attr of d.attrs) {
            if (attr in window) {
                continue outer;
            }
        }
        log(LogLevel.ERROR, d.msg);
    }
}

function register_service_worker() {
    if (!("serviceWorker" in navigator)) return;
    // Only register over secure contexts; registering over plain HTTP is a no-op on iOS
    // and prints a scary warning on Chrome/Android. Rylus ships with TLS on by default,
    // so on a vanilla install this always runs; a user who explicitly disabled TLS
    // doesn't need offline PWA support anyway.
    if (location.protocol !== "https:" && location.hostname !== "localhost") return;
    navigator.serviceWorker.register("/sw.js").catch((e) => {
        log(LogLevel.WARN, "Service worker registration failed: " + e);
    });
}

function check_browser_quirks() {
    const ua = navigator.userAgent;
    // Chrome on iPadOS is known-broken for MSE video playback. Safari and Firefox work.
    // The crPr token is unique to Chrome iOS. Show one toast, don't hard-fail — the
    // user might be on a desktop Chrome connecting to Rylus via ipad.local and just
    // happen to match this sniff; worst case they see an unnecessary message.
    if (/\bCriOS\b/.test(ua)) {
        showToast(
            "Chrome on iPadOS can't play the video stream. Please reopen this page in Safari or Firefox.",
            "error",
            6000,
        );
    }
}

function init() {
    check_apis();
    check_browser_quirks();
    register_service_worker();

    let protocol = document.location.protocol == "https:" ? "wss://" : "ws://";
    let webSocket = new WebSocket(
        protocol + window.location.hostname + ":" +
        window.location.port + "/ws"
    );
    webSocket.binaryType = "arraybuffer";
    activeWebSocket = webSocket;

    debug_overlay = document.getElementById("debug_overlay");
    settings = new Settings(webSocket);

    let video = document.getElementById("video") as HTMLVideoElement;
    let canvas = document.getElementById("canvas") as HTMLCanvasElement;

    video.oncontextmenu = function(event) {
        event.preventDefault();
        event.stopPropagation();
        return false;
    };
    canvas.oncontextmenu = function(event) {
        event.preventDefault();
        event.stopPropagation();
        return false;
    };

    let toggle_fullscreen_btn = document.getElementById("fullscreen") as HTMLButtonElement;

    if (document.exitFullscreen) {
        toggle_fullscreen_btn.onclick = () => {
            if (!document.fullscreenElement)
                document.body.requestFullscreen({ navigationUI: "hide" });
            else
                document.exitFullscreen();
        }
    } else {
        // No Fullscreen API — typically iOS/iPadOS Safari. Leave the button
        // visible but disabled with a tooltip that tells the user to use
        // "Add to Home Screen" instead. Removing it silently left iPad
        // users staring at a missing control with no explanation.
        toggle_fullscreen_btn.disabled = true;
        toggle_fullscreen_btn.title =
            "Fullscreen isn't supported in this browser. On iPadOS, use Safari's Share → Add to Home Screen.";
        toggle_fullscreen_btn.setAttribute("aria-disabled", "true");
        toggle_fullscreen_btn.onclick = () => {
            showToast(
                "Fullscreen isn't supported here. On iPad, use Safari's Share → Add to Home Screen.",
                "info",
                5000,
            );
        };
    }

    // Reconnection state machine
    const RECONNECT_MAX_ATTEMPTS = 10
    const RECONNECT_BASE_DELAY = 1000
    const RECONNECT_MAX_DELAY = 30000
    let reconnect_attempt = 0
    let reconnect_timer: number | null = null
    let reconnect_countdown_timer: number | null = null
    let reconnect_banner: HTMLDivElement | null = null
    let is_reconnecting = false
    let is_connected = false
    let heartbeat_timer: number | null = null
    const HEARTBEAT_INTERVAL = 5000  // 5s — well within 60s idle timeout, fast enough to
                                     // keep the connection-quality pip responsive.
    function start_heartbeat() {
        stop_heartbeat()
        // Send the first beat immediately so the pip is populated without a 5s delay.
        send_heartbeat()
        heartbeat_timer = window.setInterval(() => send_heartbeat(), HEARTBEAT_INTERVAL)
    }
    function stop_heartbeat() {
        if (heartbeat_timer !== null) {
            clearInterval(heartbeat_timer)
            heartbeat_timer = null
        }
    }

    function remove_reconnect_banner() {
        if (reconnect_banner) {
            reconnect_banner.remove()
            reconnect_banner = null
        }
        if (reconnect_countdown_timer !== null) {
            clearInterval(reconnect_countdown_timer)
            reconnect_countdown_timer = null
        }
        if (reconnect_timer !== null) {
            clearTimeout(reconnect_timer)
            reconnect_timer = null
        }
    }

    function show_reconnect_banner(delay_ms: number, attempt: number) {
        remove_reconnect_banner()
        let banner = document.createElement("div")
        banner.className = "rylus-reconnect-banner"
        banner.setAttribute("role", "status")
        banner.setAttribute("aria-live", "polite")
        let msg = document.createElement("span")
        let remaining = Math.ceil(delay_ms / 1000)
        let render = () => {
            msg.textContent =
                "Connection lost — input paused. Reconnecting in " +
                remaining +
                "s (attempt " +
                attempt +
                " of " +
                RECONNECT_MAX_ATTEMPTS +
                ")."
        }
        render()
        banner.appendChild(msg)
        let btn = document.createElement("button")
        btn.type = "button"
        btn.className = "rylus-reconnect-btn"
        btn.textContent = "Retry now"
        btn.setAttribute("aria-label", "Retry connecting now")
        btn.onclick = () => {
            remove_reconnect_banner()
            attempt_reconnect()
        }
        banner.appendChild(btn)
        document.body.appendChild(banner)
        reconnect_banner = banner

        reconnect_countdown_timer = window.setInterval(() => {
            remaining--
            if (remaining < 1) remaining = 1
            render()
        }, 1000)
    }

    function show_failed_banner() {
        remove_reconnect_banner()
        let banner = document.createElement("div")
        banner.className = "rylus-reconnect-banner failed"
        banner.setAttribute("role", "alert")
        let msg = document.createElement("span")
        msg.textContent = "Could not reconnect after several attempts."
        banner.appendChild(msg)
        let reload_btn = document.createElement("button")
        reload_btn.type = "button"
        reload_btn.className = "rylus-reconnect-btn"
        reload_btn.textContent = "Reload"
        reload_btn.onclick = () => location.reload()
        banner.appendChild(reload_btn)
        let retry_btn = document.createElement("button")
        retry_btn.type = "button"
        retry_btn.className = "rylus-reconnect-btn"
        retry_btn.textContent = "Try again"
        retry_btn.onclick = () => {
            remove_reconnect_banner()
            is_reconnecting = true
            reconnect_attempt = 0
            schedule_reconnect()
        }
        banner.appendChild(retry_btn)
        let dismiss = document.createElement("button")
        dismiss.type = "button"
        dismiss.className = "rylus-reconnect-dismiss"
        dismiss.textContent = "✕"
        dismiss.setAttribute("aria-label", "Dismiss reconnect banner")
        dismiss.onclick = () => remove_reconnect_banner()
        banner.appendChild(dismiss)
        document.body.appendChild(banner)
        reconnect_banner = banner
    }

    function calc_reconnect_delay(attempt: number): number {
        let delay = RECONNECT_BASE_DELAY * Math.pow(2, attempt - 1)
        return Math.min(delay, RECONNECT_MAX_DELAY)
    }

    function schedule_reconnect() {
        reconnect_attempt++
        if (reconnect_attempt > RECONNECT_MAX_ATTEMPTS) {
            is_reconnecting = false
            show_failed_banner()
            return
        }
        let delay = calc_reconnect_delay(reconnect_attempt)
        show_reconnect_banner(delay, reconnect_attempt)
        reconnect_timer = window.setTimeout(() => {
            reconnect_timer = null
            attempt_reconnect()
        }, delay)
    }

    function attempt_reconnect() {
        remove_reconnect_banner()
        let banner = document.createElement("div")
        banner.className = "rylus-reconnect-banner"
        banner.setAttribute("role", "status")
        banner.setAttribute("aria-live", "polite")
        banner.textContent =
            "Reconnecting… (attempt " + reconnect_attempt + " of " + RECONNECT_MAX_ATTEMPTS + ")"
        document.body.appendChild(banner)
        reconnect_banner = banner

        let protocol = document.location.protocol == "https:" ? "wss://" : "ws://"
        let newSocket = new WebSocket(
            protocol + window.location.hostname + ":" +
            window.location.port + "/ws"
        )
        newSocket.binaryType = "arraybuffer"

        newSocket.onerror = () => {
            // onclose will fire after onerror, so reconnect is handled there
        }

        newSocket.onclose = () => {
            if (newSocket !== activeWebSocket) return
            if (!is_reconnecting) {
                is_reconnecting = true
                reconnect_attempt = 0
            }
            schedule_reconnect()
        }

        newSocket.onopen = () => {
            // Reconnect succeeded
            activeWebSocket = newSocket
            webSocket = newSocket
            settings.webSocket = newSocket
            reconnect_attempt = 0
            is_reconnecting = false
            remove_reconnect_banner()
            start_heartbeat()

            // Re-send handshake and config
            wsSend(JSON.stringify({"Hello": {"protocol_version": 3}}))
            wsSend('"GetCapturableList"')
            if (!settings.video_enabled())
                wsSend('"PauseVideo"')
            settings.send_server_config()

            // Re-wire message handling and input handlers
            handle_messages(newSocket, video, () => {
                new KeyboardHandler(newSocket)
                new PointerHandler(newSocket)
            },
                (err) => showToast(String(err), "error", 4000),
                (window_names) => settings.onCapturableList(window_names)
            )

            document.onvisibilitychange = () => {
                if (document.hidden) {
                    wsSend('"PauseVideo"')
                } else if (settings.video_enabled()) {
                    wsSend('"ResumeVideo"')
                }
            }
        }

        activeWebSocket = newSocket
        webSocket = newSocket
    }

    let handle_disconnect = () => {
        stop_heartbeat()
        // Drop any locally-drawn partial strokes — the server never saw the
        // tail of them, so keeping them on the canvas would create a false
        // sense that the stroke had been delivered.
        if (active_painter) {
            try { active_painter.cancel_all() } catch (_e) {}
        }
        if (is_reconnecting) return
        is_reconnecting = true
        reconnect_attempt = 0
        schedule_reconnect()
    }

    webSocket.onerror = () => handle_disconnect();
    webSocket.onclose = () => handle_disconnect();
    window.onresize = () => {
        stretch_video();
        canvas.width = window.innerWidth * window.devicePixelRatio;
        canvas.height = window.innerHeight * window.devicePixelRatio;
        let [w, h] = calc_max_video_resolution(settings.scale_video_input.valueAsNumber);
        settings.scale_video_output.value = w + "x" + h;
        settings.send_server_config();
    }
    video.controls = false;
    video.disableRemotePlayback = true;
    video.onloadeddata = () => stretch_video();
    handle_messages(webSocket, video, () => {
        if (!is_connected) {
            new KeyboardHandler(webSocket);
            new PointerHandler(webSocket);
            is_connected = true;
        }
    },
        (err) => showToast(String(err), "error", 4000),
        (window_names) => settings.onCapturableList(window_names)
    );
    window.onunload = () => { webSocket.close(); }
    webSocket.onopen = function(event) {
        start_heartbeat()
        wsSend(JSON.stringify({"Hello": {"protocol_version": 3}}));
        wsSend('"GetCapturableList"');
        if (!settings.video_enabled())
            wsSend('"PauseVideo"');

        settings.send_server_config();

        document.onvisibilitychange = () => {
            if (document.hidden) {
                wsSend('"PauseVideo"');
            } else if (settings.video_enabled()) {
                wsSend('"ResumeVideo"');
            }
        };
    }
    frame_rate_stats();
}

// `run` is invoked by an inline <script> tag in index.html; bundling wraps this
// file in an IIFE, so we have to re-expose it on the global object.
(globalThis as any).run = run;

// object-fit: fill; <-- this is unfortunately not supported on iOS, so we use the following
// workaround
function stretch_video() {
    let video = document.getElementById("video") as HTMLVideoElement;
    let vw = video.clientWidth || 1;
    let vh = video.clientHeight || 1;
    if (settings.stretched_video()) {
        video.style.transform = "scaleX(" + document.body.clientWidth / vw + ") scaleY(" + document.body.clientHeight / vh + ")";
    } else {
        let scale = Math.min(document.body.clientWidth / vw, document.body.clientHeight / vh);
        video.style.transform = "scale(" + scale + ")";
    }
}
