function run(password: string, websocket_port: number) {
    window.onload = () => { init(password, websocket_port) };
}

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

    constructor(video: HTMLVideoElement, webSocket: WebSocket) {
        this.video = video;
        this.webSocket = webSocket;
        this.video.addEventListener("pointerdown", (e) => { this.onDown(e) }, false);
        this.video.addEventListener("pointerup", (e) => { this.onUp(e) }, false);
        this.video.addEventListener("pointercancel", (e) => { this.onCancel(e) }, false);
        this.video.addEventListener("pointermove", (e) => { this.onMove(e) }, false);
    }

    onDown(event: PointerEvent) {
        this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent("pointerdown", event, this.video) }));
    }

    onUp(event: PointerEvent) {
        this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent("pointerup", event, this.video) }));
    }

    onCancel(event: PointerEvent) {
        this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent("pointercancel", event, this.video) }));
    }

    onMove(event: PointerEvent) {
        this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent("pointermove", event, this.video) }));
    }
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
    function upd_buf() {
        if (sourceBuffer == null)
            return;
        if (!sourceBuffer.updating && queue.length > 0 && mediaSource.readyState == "open") {
            sourceBuffer.appendBuffer(queue.shift());
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
                    })
                    requestAnimationFrame(() => webSocket.send('"GetFrame"'));
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
        if (video.seekable.length > 0 && video.seekable.end(0) - video.currentTime > 0.01)
            video.currentTime = video.seekable.end(0)
        requestAnimationFrame(() => webSocket.send('"GetFrame"'));
    }
}

function init(password: string, websocket_port: number) {

    // pointer
    let webSocket = new WebSocket("ws://" + window.location.hostname + ":" + websocket_port);
    webSocket.binaryType = "arraybuffer";
    webSocket.onerror = () => handle_disconnect("Lost connection.");
    webSocket.onclose = () => handle_disconnect("Connection closed.");

    // videostreaming
    let video = document.getElementById("video") as HTMLVideoElement;

    window.onresize = () => stretch_video(video);
    video.controls = false;
    video.onloadeddata = () => stretch_video(video);
    handle_messages(webSocket, video, () => {
        new PointerHandler(video, webSocket);
        webSocket.send('"GetFrame"');
    },
        (err) => alert(err),
        (capturables) => console.log(capturables)
    );
    window.onunload = () => { webSocket.close(); }
    webSocket.onopen = function(event) {
        if (password)
            webSocket.send(password);
        webSocket.send('"GetCapturableList"');
        let config =
        {
            "stylus_support": true,
            "enable_mouse": true,
            "enable_stylus": true,
            "enable_touch": true,
            "faster_capture": true,
            "capturable_id": 0,
            "capture_cursor": true,
        }
        webSocket.send(JSON.stringify({ "Config": config }));
    }

}


// object-fit: fill; <-- this is unfortunately not supported on iOS, so we use the following
// workaround
function stretch_video(video: HTMLVideoElement) {
    video.style.transform = "scaleX(" + document.body.clientWidth / video.clientWidth + ") scaleY(" + document.body.clientHeight / video.clientHeight + ")";
}


function handle_disconnect(msg: string) {
    let video = document.getElementById("video") as HTMLVideoElement;
    video.onclick = () => {
        if (window.confirm(msg + " Reload the page?"))
            location.reload();
    }
}
