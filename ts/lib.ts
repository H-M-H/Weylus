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

function run(access_code: string, websocket_port: number, level: string) {
    window.onload = () => {
        log_pre = document.getElementById("log") as HTMLPreElement;
        log_pre.textContent = "";
        log_level = LogLevel[level];
        window.addEventListener("error", (e: ErrorEvent) => {
            log(LogLevel.ERROR, e.filename + ":L" + e.lineno + ":" + e.colno + ": " + e.message);
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
    log_pre.textContent += msg + "\n";
}

class Settings {
    webSocket: WebSocket;
    checks: Map<string, HTMLInputElement>;
    capturable_select: HTMLSelectElement;
    frame_update_limit_input: HTMLInputElement;
    visible: boolean;
    settings: HTMLElement;

    constructor(webSocket: WebSocket) {
        this.webSocket = webSocket;
        this.checks = new Map<string, HTMLInputElement>();
        this.capturable_select = document.getElementById("window") as HTMLSelectElement;
        this.frame_update_limit_input = document.getElementById("frame_update_limit") as HTMLInputElement;
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

        let upd_pointer_filter = () => { this.save_settings(); new PointerHandler(this.webSocket); }
        this.checks.get("enable_mouse").onchange = upd_pointer_filter;
        this.checks.get("enable_stylus").onchange = upd_pointer_filter;
        this.checks.get("enable_touch").onchange = upd_pointer_filter;

        this.frame_update_limit_input.onchange = () => this.save_settings();

        // server
        let upd_server_config = () => { this.save_settings(); this.send_server_config() };
        this.checks.get("faster_capture").onchange = (e) => {
            this.capturable_select.disabled = !(e.target as HTMLInputElement).checked;
            this.checks.get("capture_cursor").disabled = !(e.target as HTMLInputElement).checked;
            upd_server_config()
        };
        this.checks.get("stylus_support").onchange = upd_server_config;
        this.checks.get("capture_cursor").onchange = upd_server_config;

        document.getElementById("refresh").onclick = () => this.webSocket.send('"GetCapturableList"');
        this.capturable_select.onchange = () => this.send_server_config();
    }

    send_server_config() {
        let config = new Object(null);
        config["capturable_id"] = Number(this.capturable_select.value);
        for (const key of [
            "stylus_support",
            "faster_capture",
            "capture_cursor"])
            config[key] = this.checks.get(key).checked
        config["max_width"] = Math.round(window.screen.availWidth * window.devicePixelRatio);
        config["max_height"] = Math.round(window.screen.availHeight * window.devicePixelRatio);
        this.webSocket.send(JSON.stringify({ "Config": config }));
    }

    save_settings() {
        let settings = Object(null);
        for (const [key, elem] of this.checks.entries())
            settings[key] = elem.checked;
        settings["frame_update_limit"] = this.frame_update_limit_input.value;
        localStorage.setItem("settings", JSON.stringify(settings));
    }

    load_settings() {
        let settings_string = localStorage.getItem("settings");
        if (settings_string === null)
            return;
        try {
            let settings = JSON.parse(settings_string);
            for (const [key, elem] of this.checks.entries()) {
                if (typeof settings[key] === "boolean")
                    elem.checked = settings[key];
            }
            this.capturable_select.disabled = !this.checks.get("faster_capture").checked;
            this.checks.get("capture_cursor").disabled = !this.checks.get("faster_capture").checked;
            this.frame_update_limit_input.value = settings["frame_update_limit"] ?? 0;
            if (this.checks.get("lefty").checked) {
                this.settings.classList.add("lefty");
            }

        } catch {
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

    frame_update_limit() {
        return this.frame_update_limit_input.valueAsNumber
    }

    toggle() {
        this.settings.classList.toggle("hide");
        this.visible = !this.visible;
    }

    onCapturableList(window_names: string[]) {
        let current_selection = this.capturable_select.selectedOptions[0]?.textContent;
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

    constructor(eventType: string, event: PointerEvent, video: HTMLVideoElement) {
        let videoRect = video.getBoundingClientRect();
        let diag_len = Math.sqrt(videoRect.width * videoRect.width + videoRect.height * videoRect.height)
        this.event_type = eventType.toString();
        this.pointer_id = event.pointerId;
        this.timestamp = Math.round(event.timeStamp * 1000);
        this.is_primary = event.isPrimary;
        this.pointer_type = event.pointerType;
        this.button = event.button < 0 ? 0 : 1 << event.button;
        this.buttons = event.buttons;
        this.x = (event.clientX - videoRect.left) / videoRect.width;
        this.y = (event.clientY - videoRect.top) / videoRect.height;
        this.movement_x = event.movementX ? event.movementX : 0;
        this.movement_y = event.movementY ? event.movementY : 0;
        this.pressure = event.pressure;
        this.tilt_x = event.tiltX;
        this.tilt_y = event.tiltY;
        this.width = event.width / diag_len;
        this.height = event.height / diag_len;
        this.twist = event.twist;
    }
}

class PointerHandler {
    video: HTMLVideoElement;
    webSocket: WebSocket;
    pointerTypes: string[];

    constructor(webSocket: WebSocket) {
        this.video = document.getElementById("video") as HTMLVideoElement;
        this.webSocket = webSocket;
        this.pointerTypes = settings.pointer_types();
        this.video.onpointerdown = (e) => { this.onEvent(e, "pointerdown") };
        this.video.onpointerup = (e) => { this.onEvent(e, "pointerup") };
        this.video.onpointercancel = (e) => { this.onEvent(e, "pointercancel") };
        this.video.onpointermove = (e) => { this.onEvent(e, "pointermove") };
    }

    onEvent(event: PointerEvent, event_type: string) {
        if (this.pointerTypes.includes(event.pointerType)) {
            this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent(event_type, event, this.video) }));
            if (settings.visible) {
                settings.toggle();
            }
        }
    }
}

function frame_timer(webSocket: WebSocket) {
    if (webSocket.readyState > webSocket.OPEN)  // Closing or closed, so no more frames
        return;

    if (webSocket.readyState === webSocket.OPEN)
        webSocket.send('"TryGetFrame"');
    let upd_limit = settings.frame_update_limit();
    if (upd_limit > 0)
        setTimeout(() => frame_timer(webSocket), upd_limit);
    else
        requestAnimationFrame(() => frame_timer(webSocket));
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
            }

            return;
        }

        // not a string -> got a video frame
        queue.push(event.data);
        upd_buf();
        if (video.seekable.length > 0 &&
            // only seek if there is data available, some browsers choke otherwise
            (video.readyState >= 3 ||
                // seek to end if we are more than half a second off, this may happen if a tab is
                // moved to the background
                video.seekable.end(video.seekable.length - 1) - video.currentTime > 0.5)) {
            video.currentTime = video.seekable.end(video.seekable.length - 1);
        }
    }
}

function init(access_code: string, websocket_port: number) {

    let authed = false;
    let webSocket = new WebSocket("ws://" + window.location.hostname + ":" + websocket_port);
    webSocket.binaryType = "arraybuffer";

    settings = new Settings(webSocket);

    let video = document.getElementById("video") as HTMLVideoElement;

    let handle_disconnect = (msg: string) => {
        document.body.onclick = video.onclick = (e) => {
            e.stopPropagation();
            if (window.confirm(msg + " Reload page?"))
                location.reload();
        }
    }
    webSocket.onerror = () => handle_disconnect("Lost connection.");
    webSocket.onclose = () => handle_disconnect("Connection closed.");
    window.onresize = () => {
        stretch_video();
        if (authed)
            settings.send_server_config();
    }
    video.controls = false;
    video.onloadeddata = () => stretch_video();
    handle_messages(webSocket, video, () => {
        new PointerHandler(webSocket);
        frame_timer(webSocket);
    },
        (err) => alert(err),
        (window_names) => settings.onCapturableList(window_names)
    );
    window.onunload = () => { webSocket.close(); }
    webSocket.onopen = function(event) {
        if (access_code)
            webSocket.send(access_code);
        authed = true;
        webSocket.send('"GetCapturableList"');
        settings.send_server_config();
    }
}

// object-fit: fill; <-- this is unfortunately not supported on iOS, so we use the following
// workaround
function stretch_video() {
    let video = document.getElementById("video") as HTMLVideoElement;
    if (settings.stretched_video()) {
        video.style.transform = "scaleX(" + document.body.clientWidth / video.clientWidth + ") scaleY(" + document.body.clientHeight / video.clientHeight + ")";
    } else {
        video.style.transform = "none"
    }
}
