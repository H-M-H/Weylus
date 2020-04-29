async function delay(milliseconds: number) {
    return new Promise(resolve => setTimeout(resolve, milliseconds));
}

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

    constructor(eventType: string, event: PointerEvent, canvas: HTMLCanvasElement) {
        let canvasRect = canvas.getBoundingClientRect();
        let diag_len = Math.sqrt(canvas.width * canvas.width + canvas.height * canvas.height)
        this.event_type = eventType.toString();
        this.pointer_id = event.pointerId;
        this.is_primary = event.isPrimary;
        this.pointer_type = event.pointerType;
        this.button = event.button < 0 ? 0 : 1 << event.button;
        this.buttons = event.buttons;
        this.x = (event.clientX - canvasRect.left) / canvasRect.width;
        this.y = (event.clientY - canvasRect.top) / canvasRect.height;
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
    canvas: HTMLCanvasElement;
    webSocket: WebSocket;

    constructor(canvas: HTMLCanvasElement, webSocket: WebSocket) {
        this.canvas = canvas;
        this.webSocket = webSocket;
        this.canvas.addEventListener("pointerdown", (e) => { this.onDown(e) }, false);
        this.canvas.addEventListener("pointerup", (e) => { this.onUp(e) }, false);
        this.canvas.addEventListener("pointercancel", (e) => { this.onCancel(e) }, false);
        this.canvas.addEventListener("pointermove", (e) => { this.onMove(e) }, false);
    }

    onDown(event: PointerEvent) {
        this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent("pointerdown", event, this.canvas) }));
    }

    onUp(event: PointerEvent) {
        this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent("pointerup", event, this.canvas) }));
    }

    onCancel(event: PointerEvent) {
        this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent("pointercancel", event, this.canvas) }));
    }

    onMove(event: PointerEvent) {
        this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent("pointermove", event, this.canvas) }));
    }
}

function init(password: string, websocket_pointer_port: number, websocket_video_port: number) {
    let webSocket = new WebSocket("ws://" + window.location.hostname + ":" + websocket_pointer_port, "pointer");
    let canvas = document.getElementById("canvas") as HTMLCanvasElement;
    webSocket.onopen = function(event) {
        if (password)
            webSocket.send(password);
        let pointerHandler = new PointerHandler(canvas, webSocket);
    }
    webSocket.onmessage = function(event) {
    }


    let videoWebSocket = new WebSocket("ws://" + window.location.hostname + ":" + websocket_video_port, "video");
    videoWebSocket.onmessage = (event: MessageEvent) => {
        let img = new Image();
        img.src = "data:image/png;base64," + event.data;
        let ctx = canvas.getContext("2d");
        img.onload = () => {
            if (canvas.height != img.height)
                canvas.height = img.height;
            if (canvas.width != img.width)
                canvas.width = img.width;
            ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
        };
        videoWebSocket.send("");
    }
    videoWebSocket.onopen = () => {
        if (password)
            videoWebSocket.send(password);
        videoWebSocket.send("");
    }
}

function fullscreen() {
    let canvas = document.getElementById("canvas") as any;
    if (canvas.requestFullscreen) {
        canvas.requestFullscreen();
    } else if (canvas.webkitRequestFullScreen) {
        canvas.webkitRequestFullScreen();
    } else if (canvas.mozRequestFullScreen) {
        canvas.mozRequestFullScreen();
    }
}
