import { initVirtualKey, setVirtualKeysProfiles } from "./virtualKey";

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
let webSocket: WebSocket;

export const getWebSocket = () => webSocket;
export const sendWsMessage = (msg: object) => webSocket.send(JSON.stringify(msg));

function run(access_code: string, websocket_port: number, level: string) {
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
        init(access_code, websocket_port)
    };
}

function log(level: LogLevel, msg: string) {
    if (level > log_level)
        return;
    if (no_log_messages) {
        no_log_messages = false;
        document.getElementById("log_section").classList.remove("hide");
    }
    log_pre.textContent += LogLevel[level] + ": " + msg + "\n";
}

function frame_update_scale(x: number) {
    return Math.pow(x / 100, 3);
}

function frame_update_scale_inv(x: number) {
    return 100 * Math.pow(x, 1 / 3);
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

    if (painter) painter = undefined; // dispose of old painter
    return canvas;
}

function toggle_energysaving(energysaving: boolean) {
    const canvas = fresh_canvas();

    if (!energysaving) {
        painter = new Painter(canvas);
    }

    if (settings) {
        if (energysaving) {
            settings.checks.get("enable_video").checked = false;
            settings.checks.get("enable_video").disabled = true;
            settings.checks.get("enable_video").dispatchEvent(new Event("change"));
        } else
            settings.checks.get("enable_video").disabled = false;
    }
}

class Settings {
    checks: Map<string, HTMLInputElement>;
    capturable_select: HTMLSelectElement;
    frame_update_limit_input: HTMLInputElement;
    frame_update_limit_output: HTMLOutputElement;
    scale_video_input: HTMLInputElement;
    scale_video_output: HTMLOutputElement;
    range_min_pressure: HTMLInputElement;
    check_aggressive_seek: HTMLInputElement;
    client_name_input: HTMLInputElement;
    visible: boolean;
    settings: HTMLElement;

    constructor() {
        this.checks = new Map<string, HTMLInputElement>();
        this.capturable_select = document.getElementById("window") as HTMLSelectElement;
        this.frame_update_limit_input = document.getElementById("frame_update_limit") as HTMLInputElement;
        this.frame_update_limit_input.min = frame_update_scale_inv(1).toString();
        this.frame_update_limit_input.max = frame_update_scale_inv(1000).toString();
        this.frame_update_limit_output = this.frame_update_limit_input.nextElementSibling as HTMLOutputElement;
        this.scale_video_input = document.getElementById("scale_video") as HTMLInputElement;
        this.scale_video_output = this.scale_video_input.nextElementSibling as HTMLOutputElement;
        this.range_min_pressure = document.getElementById("min_pressure") as HTMLInputElement;
        this.client_name_input = document.getElementById("client_name") as HTMLInputElement;
        this.frame_update_limit_input.oninput = (e) => {
            this.frame_update_limit_output.value = Math.round(frame_update_scale(this.frame_update_limit_input.valueAsNumber)).toString();
        }
        this.scale_video_input.oninput = (e) => {
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

        this.checks.get("stretch").onchange = (e) => {
            stretch_video();
            this.save_settings();
        };

        this.check_aggressive_seek = this.checks.get("aggressive_seeking");
        this.check_aggressive_seek.onchange = (e) => {
            this.save_settings();
        };

        this.checks.get("enable_video").onchange = (e) => {
            document.getElementById("video").classList.toggle("vanish", !(e.target as HTMLInputElement).checked);
            document.getElementById("canvas").classList.toggle("vanish", (e.target as HTMLInputElement).checked);
            this.save_settings();
        }

        let upd_pointer = () => {
            this.save_settings();
        }
        this.checks.get("enable_mouse").onchange = upd_pointer;
        this.checks.get("enable_stylus").onchange = upd_pointer;
        this.checks.get("enable_touch").onchange = upd_pointer;

        this.checks.get("energysaving").onchange = (e) => {
            this.save_settings();
            toggle_energysaving((e.target as HTMLInputElement).checked);
        };

        this.frame_update_limit_input.onchange = () => this.save_settings();
        this.range_min_pressure.onchange = () => this.save_settings();

        // server
        let upd_server_config = () => { this.save_settings(); this.send_server_config() };
        this.checks.get("uinput_support").onchange = upd_server_config;
        this.checks.get("capture_cursor").onchange = upd_server_config;
        this.scale_video_input.onchange = upd_server_config;
        this.client_name_input.onchange = upd_server_config;

        document.getElementById("refresh").onclick = () => webSocket.send('"GetCapturableList"');
        this.capturable_select.onchange = () => this.send_server_config();
    }

    send_server_config() {
        let config = new Object(null);
        config["capturable_id"] = Number(this.capturable_select.value);
        for (const key of [
            "uinput_support",
            "capture_cursor"])
            config[key] = this.checks.get(key).checked;
        let [w, h] = calc_max_video_resolution(this.scale_video_input.valueAsNumber);
        config["max_width"] = w;
        config["max_height"] = h;
        if (this.client_name_input.value)
            config["client_name"] = this.client_name_input.value;
        webSocket.send(JSON.stringify({ "Config": config }));
    }

    save_settings() {
        let settings = Object(null);
        for (const [key, elem] of this.checks.entries())
            settings[key] = elem.checked;
        settings["frame_update_limit"] = frame_update_scale(this.frame_update_limit_input.valueAsNumber).toString();
        settings["scale_video"] = this.scale_video_input.value;
        settings["min_pressure"] = this.range_min_pressure.value;
        settings["client_name"] = this.client_name_input.value;
        localStorage.setItem("settings", JSON.stringify(settings));
    }

    load_settings() {
        let settings_string = localStorage.getItem("settings");
        if (settings_string === null) {
            this.frame_update_limit_input.value = frame_update_scale_inv(33).toString();
            this.frame_update_limit_output.value = (33).toString();
            let [w, h] = calc_max_video_resolution(this.scale_video_input.valueAsNumber)
            this.scale_video_output.value = w + "x" + h;
            return;
        }
        try {
            let settings = JSON.parse(settings_string);
            for (const [key, elem] of this.checks.entries()) {
                if (typeof settings[key] === "boolean")
                    elem.checked = settings[key];
            }
            let upd_limit = settings["frame_update_limit"];
            if (upd_limit)
                this.frame_update_limit_input.value = frame_update_scale_inv(upd_limit).toString();
            else
                this.frame_update_limit_input.value = frame_update_scale_inv(33).toString();
            this.frame_update_limit_output.value = Math.round(frame_update_scale(this.frame_update_limit_input.valueAsNumber)).toString();

            let scale_video = settings["scale_video"];
            if (scale_video)
                this.scale_video_input.value = scale_video;
            let [w, h] = calc_max_video_resolution(this.scale_video_input.valueAsNumber);
            this.scale_video_output.value = w + "x" + h;

            let min_pressure = settings["min_pressure"];
            if (min_pressure)
                this.range_min_pressure.value = min_pressure;

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

            toggle_energysaving(!!this.checks.get("energysaving").checked);

            if (!this.checks.get("enable_virtual_keys").checked) {
                document.getElementById("vk-container").classList.add("hidden");
            }

            let client_name = settings["client_name"];
            if (client_name)
                this.client_name_input.value = client_name;

        } catch {
            log(LogLevel.DEBUG, "Failed to load settings.")
            return;
        }
    }

    stretched_video() {
        return this.checks.get("stretch").checked
    }

    is_pointer_type_enabled(ptr_type: string) {
        if (ptr_type === 'mouse') return !!this.checks.get("enable_mouse").checked;
        if (ptr_type === 'pen') return !!this.checks.get("enable_stylus").checked;
        if (ptr_type === 'touch') return !!this.checks.get("enable_touch").checked;
        return false;
    }

    frame_update_limit() {
        return frame_update_scale(this.frame_update_limit_input.valueAsNumber)
    }

    toggle() {
        this.settings.classList.toggle("hide");
        this.visible = !this.visible;
    }

    onCapturableList(window_names: string[]) {
        let current_selection = undefined;
        if (this.capturable_select.selectedOptions[0])
            current_selection = this.capturable_select.selectedOptions[0].textContent;
        let new_index;
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
    }


}

let settings: Settings;

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
    movement_x: number;
    movement_y: number;
    pressure: number;
    tilt_x: number;
    tilt_y: number;
    twist: number;
    width: number;
    height: number;

    constructor(eventType: string, event: PointerEvent, targetRect: DOMRect) {
        let diag_len = Math.sqrt(targetRect.width * targetRect.width + targetRect.height * targetRect.height)
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
        this.x = (event.clientX - targetRect.left) / targetRect.width;
        this.y = (event.clientY - targetRect.top) / targetRect.height;
        this.movement_x = event.movementX ? event.movementX : 0;
        this.movement_y = event.movementY ? event.movementY : 0;
        this.pressure = Math.max(event.pressure, settings.range_min_pressure.valueAsNumber);
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
        requestAnimationFrame(() => this.render());
    }

    render() {
        // only do work if necessary
        if (!check_video.checked && (this.lines_active.size > 0 || this.lines_old.length > 0)) {
            if (this.lines_old.length > 0) {
                if (performance.now() - this.lines_old[0][this.lines_old[0].length - 1] > fade_time)
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
        requestAnimationFrame(() => this.render());
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
        if (this.lines_active.has(event.pointerId))
            for (const e of event.getCoalescedEvents()) {
                this.appendEventToLine(e);
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

let painter: Painter | undefined   // be undefined in power-save mode

class PointerHandler {
    constructor() {
        let el = document.getElementById("pointer-event-receiver");

        el.onpointerenter = (e) => this.onEvent(e, "pointerenter");
        el.onpointerleave = (e) => this.onEvent(e, "pointerleave");
        el.onpointerdown = (e) => { this.onEvent(e, "pointerdown"); painter?.onstart(e); };
        el.onpointerup = (e) => { this.onEvent(e, "pointerup"); painter?.onstop(e); };
        el.onpointercancel = (e) => { this.onEvent(e, "pointercancel"); painter?.onstop(e); };
        el.addEventListener('pointermove', (e) => { this.onEvent(e, "pointermove"); painter?.onmove(e); }, { passive: true });
        el.onwheel = (e) => {
            webSocket.send(JSON.stringify({ "WheelEvent": new WEvent(e) }));
            e.preventDefault();
        }
    }

    /** since getBoundingClientRect() is slow, we only compute once when pointer is down */
    targetRect: DOMRect;

    onEvent(event: PointerEvent, event_type: string) {
        if (!settings.is_pointer_type_enabled(event.pointerType))
            return;

        if (event_type === "pointerdown") {
            const el = event.currentTarget as HTMLElement;
            el.setPointerCapture(event.pointerId);
            event.preventDefault();
            this.targetRect = el.getBoundingClientRect();
        }

        const events = event_type === "pointermove" ? event.getCoalescedEvents() : [event];

        for (let event of events) {
            webSocket.send(
                JSON.stringify(
                    {
                        "PointerEvent": new PEvent(
                            event_type,
                            event,
                            this.targetRect
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

export class KEvent {
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
    constructor() {
        let m = document.getElementById("main");

        m.onkeydown = (e) => {
            if (e.repeat)
                return this.onEvent(e, "repeat");
            return this.onEvent(e, "down");
        };
        m.onkeyup = (e) => { return this.onEvent(e, "up") };
        m.onkeypress = (e) => {
            e.preventDefault();
            e.stopPropagation();
            return false;
        }
    }

    onEvent(event: KeyboardEvent, event_type: string) {
        webSocket.send(JSON.stringify({ "KeyboardEvent": new KEvent(event_type, event) }));
        event.preventDefault();
        event.stopPropagation();
        return false;
    }
}

function frame_timer() {
    // Closing or closed, so no more frames
    if (webSocket.readyState > webSocket.OPEN)
        return;

    let t = performance.now();
    if (t - last_fps_calc > 1500) {
        let fps = Math.round(frame_count / (t - last_fps_calc) * 10000) / 10;
        fps_out.value = fps.toString();
        frame_count = 0;
        last_fps_calc = t;
    }

    if (document.hidden) {
        requestAnimationFrame(() => frame_timer());
        return;
    }

    if (webSocket.readyState === webSocket.OPEN && check_video.checked)
        webSocket.send('"TryGetFrame"');
    setTimeout(() => frame_timer(), settings.frame_update_limit());
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
                    if (sourceBuffer.updating)
                        sourceBuffer.abort();
                    sourceBuffer.remove(0, Infinity);
                }
            }
        }
    }
    webSocket.onmessage = (event: MessageEvent) => {
        if (typeof event.data == "string") {
            let msg = JSON.parse(event.data);
            if (typeof msg == "string") {
                if (msg == "NewVideo") {
                    mediaSource = new MediaSource();
                    sourceBuffer = null;
                    video.src = URL.createObjectURL(mediaSource);
                    mediaSource.addEventListener("sourceopen", (_) => {
                        let mimeType = 'video/mp4; codecs="avc1.4D403D"';
                        if (!MediaSource.isTypeSupported(mimeType))
                            mimeType = "video/mp4";
                        sourceBuffer = mediaSource.addSourceBuffer(mimeType);
                        sourceBuffer.addEventListener("updateend", upd_buf);
                        // try to recover from errors by restarting the video
                        if (sourceBuffer.onerror)
                            sourceBuffer.onerror = () => settings.send_server_config();
                    })
                } else if (msg == "ConfigOk") {
                    onConfigOk();
                }
            } else if (typeof msg == "object") {
                if ("CapturableList" in msg)
                    onCapturableList(msg["CapturableList"]);
                else if ("Error" in msg)
                    alert(msg["Error"]);
                else if ("ConfigError" in msg) {
                    onConfigError(msg["ConfigError"]);
                }
                else if ("VirtualKeysProfiles" in msg) {
                    const profiles = JSON.parse(msg["VirtualKeysProfiles"] || '[]');
                    setVirtualKeysProfiles(profiles);
                }
            }

            return;
        }

        // not a string -> got a video frame
        queue.push(event.data);
        upd_buf();
        frame_count += 1;

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
    let apis = {
        "MediaSource": "This browser doesn't support MSE required to playback video stream, try upgrading!",
        "PointerEvent": "This browser doesn't support PointerEvents, input will not work, try upgrading!",
    };
    for (let n in apis) {
        if (!(n in window)) {
            log(LogLevel.ERROR, apis[n]);
        }
    }
}

function init(access_code: string, websocket_port: number) {
    check_apis();

    const disconnectedNotice = document.getElementById("disconnected-notice") as HTMLDivElement;
    const connectingNotice = document.getElementById("connecting-notice") as HTMLDivElement;

    const video = document.getElementById("video") as HTMLVideoElement;
    const getCanvas = () => document.getElementById("canvas") as HTMLCanvasElement; // canvas may rebuild

    new KeyboardHandler();
    new PointerHandler();

    let is_connected = false;

    /** update global `webSocket` and start connecting */
    const makeConnection = () => {
        is_connected = false;
        if (webSocket) webSocket.close();

        connectingNotice.classList.remove("hidden");
        disconnectedNotice.classList.add("hidden");

        const protocol = document.location.protocol == "https:" ? "wss://" : "ws://";
        const ws = webSocket = new WebSocket(protocol + window.location.hostname + ":" + websocket_port);

        ws.binaryType = "arraybuffer";
        ws.onopen = function () {
            if (webSocket !== ws) return;
            if (access_code) ws.send(access_code);
            is_connected = true;

            ws.send('"GetCapturableList"');
            ws.send('"RequestVirtualKeysProfiles"');
            settings.send_server_config();

            disconnectedNotice.classList.add("hidden");
            connectingNotice.classList.add("hidden");
        }

        let disconnected = false;
        let handle_disconnect = (msg: string) => {
            if (webSocket !== ws) return;  // a outdated connection
            if (disconnected) return;

            is_connected = false;
            disconnected = true;
            disconnectedNotice.classList.remove("hidden");
            connectingNotice.classList.add("hidden");
            disconnectedNotice.querySelector("h2").textContent = msg;

            function handleGlobalClick(e: MouseEvent) {
                e.stopPropagation();
                e.preventDefault();
                document.body.removeEventListener("click", handleGlobalClick, true);
                makeConnection()
            }
            document.body.addEventListener("click", handleGlobalClick, true);
        }
        ws.onerror = () => handle_disconnect("Lost connection.");
        ws.onclose = () => handle_disconnect("Connection closed.");

        let config_ok_received = false;
        handle_messages(webSocket, video, () => {
            if (!config_ok_received) {
                frame_timer();
                config_ok_received = true;
            }
        },
            (err) => alert(err),
            (window_names) => settings.onCapturableList(window_names)
        );
    }
    makeConnection();

    settings = new Settings();
    initVirtualKey();

    document.body.addEventListener("contextmenu", (event) => {
        event.preventDefault();
        event.stopPropagation();
        return false;
    }, true);

    let toggle_fullscreen_btn = document.getElementById("fullscreen") as HTMLButtonElement;

    if (document.exitFullscreen) {
        toggle_fullscreen_btn.onclick = () => {
            if (!document.fullscreenElement)
                document.body.requestFullscreen({ navigationUI: "hide" });
            else
                document.exitFullscreen();
        }
    } else {
        // if document.exitFullscreen is not present we are probably running on iOS/iPadOS.
        // As input is broken in fullscreen mode on these, do not offer fullscreen in the first
        // place.
        toggle_fullscreen_btn.parentElement.removeChild(toggle_fullscreen_btn);
    }

    window.onresize = () => {
        stretch_video();
        const canvas = getCanvas();
        canvas.width = window.innerWidth * window.devicePixelRatio;
        canvas.height = window.innerHeight * window.devicePixelRatio;
        let [w, h] = calc_max_video_resolution(settings.scale_video_input.valueAsNumber);
        settings.scale_video_output.value = w + "x" + h;
        if (is_connected)
            settings.send_server_config();
    }
    video.controls = false;
    video.onloadeddata = () => stretch_video();
}

// object-fit: fill; <-- this is unfortunately not supported on iOS, so we use the following
// workaround
function stretch_video() {
    const video = document.getElementById("video") as HTMLVideoElement;
    const is_stretched = settings.stretched_video();

    if (navigator.userAgent.includes("iPad") || navigator.userAgent.includes("iPhone")) {
        const windowRatio = window.innerWidth / window.innerHeight;
        const videoRatio = video.videoWidth / video.videoHeight;
        if (windowRatio > videoRatio) {
            video.style.transform = "scaleX(" + (windowRatio / videoRatio) + ")";
        } else {
            video.style.transform = "scaleY(" + (videoRatio / windowRatio) + ")";
        }
    } else {
        video.style.objectFit = is_stretched ? "fill" : "contain";
    }
}

// @ts-ignore
window.run = run;
