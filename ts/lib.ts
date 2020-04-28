async function delay(milliseconds: number) {
    return new Promise(resolve => setTimeout(resolve, milliseconds));
}

function run(websocket_pointer_addr: string, websocket_video_addr: string) {
    window.onload = () => { init(websocket_pointer_addr, websocket_video_addr) };
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

    constructor(eventType: String, event: PointerEvent, canvas: HTMLCanvasElement) {
        let canvasRect = canvas.getBoundingClientRect();
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
        this.width = event.width;
        this.twist = event.twist;
        this.height = event.height;
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

function init(websocket_pointer_addr: string, websocket_video_addr: string) {
    let webSocket = new WebSocket("ws://" + websocket_pointer_addr, "pointer");
    let canvas = document.getElementById("canvas") as HTMLCanvasElement;
    webSocket.onopen = function(event) {
        let pointerHandler = new PointerHandler(canvas, webSocket);
    }
    webSocket.onmessage = function(event) {
    }


    let videoWebSocket = new WebSocket("ws://" + websocket_video_addr, "video");
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
        videoWebSocket.send("gimme gimme !");
    }
    videoWebSocket.onopen = () => {
        videoWebSocket.send("gimme gimme !");
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
