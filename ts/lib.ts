function run(password: string, websocket_pointer_port: number, websocket_video_port: number) {
    window.onload = () => { init(password, websocket_pointer_port, websocket_video_port) };
}

class ClientConfig {
    width: number;
    height: number;

    constructor(width: number, height: number) {
        this.width = width;
        this.height = height;
    }
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

function process_stream(videoWebSocket: WebSocket, video: HTMLVideoElement) {
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
    videoWebSocket.onmessage = (event: MessageEvent) => {
        if (typeof event.data == "string") {
            if (event.data[0] == "@") {
                let interval_millis: number = parseInt(event.data.slice(1, -1));
                setTimeout(() => videoWebSocket.send(""), interval_millis);
            } else if (event.data == "new") {
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

            }
            requestAnimationFrame(() => videoWebSocket.send(""));
            return;
        }
        queue.push(event.data);
        upd_buf();
        if (video.seekable.length > 0 && video.seekable.end(0) - video.currentTime > 0.01)
            video.currentTime = video.seekable.end(0)
        requestAnimationFrame(() => videoWebSocket.send(""));
    }
}

function init(password: string, websocket_pointer_port: number, websocket_video_port: number) {

    // pointer
    let webSocket = new WebSocket("ws://" + window.location.hostname + ":" + websocket_pointer_port);
    webSocket.onopen = function(event) {
        if (password)
            webSocket.send(password);
        let pointerHandler = new PointerHandler(video, webSocket);
    }


    // videostreaming
    let video = document.getElementById("video") as HTMLVideoElement;

    window.onresize = () => stretch_video(video);
    video.autoplay = true;
    video.controls = false;
    video.onloadeddata = () => stretch_video(video);
    let videoWebSocket = new WebSocket("ws://" + window.location.hostname + ":" + websocket_video_port);
    videoWebSocket.binaryType = "arraybuffer";
    videoWebSocket.onopen = () => {
        if (password)
            videoWebSocket.send(password);
        videoWebSocket.send("");
    }
    process_stream(videoWebSocket, video);
}


// object-fit: fill; <-- this is unfortunately not supported on iOS, so we use the following
// workaround
function stretch_video(video: HTMLVideoElement) {
    video.style.transform = "scaleX(" + document.body.clientWidth / video.clientWidth + ") scaleY(" + document.body.clientHeight / video.clientHeight + ")";
}
