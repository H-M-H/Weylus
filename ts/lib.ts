async function delay(milliseconds: number) {
    return new Promise(resolve => setTimeout(resolve, milliseconds));
}

function run() {
    window.onload = init;
}

let span: HTMLSpanElement;

function log(msg: String) {
    //span.textContent += "\n" + msg;
    console.log(msg)
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
    event_type: String;
    pointer_id: number;
    is_primary: Boolean;
    pointer_type: String;
    button: number;
    buttons: number;
    screen_x: number;
    screen_y: number;
    movement_x: number;
    movement_y: number;
    pressure: number;
    tilt_x: number;
    tilt_y: number;
    width: number;
    height: number;

    constructor(eventType: String, event: PointerEvent) {
        this.event_type = eventType;
        this.pointer_id = event.pointerId;
        this.is_primary = event.isPrimary;
        this.pointer_type = event.pointerType;
        this.button = event.button < 0 ? 0 : 1 << event.button;
        this.buttons = event.buttons;
        this.screen_x = event.screenX;
        this.screen_y = event.screenY;
        this.movement_x = event.movementX ? event.movementX : 0;
        this.movement_y = event.movementY ? event.movementY : 0;
        this.pressure = event.pressure;
        this.tilt_x = event.tiltX;
        this.tilt_y = event.tiltY;
        this.width = event.width;
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
        this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent("pointerdown", event) }));
    }

    onUp(event: PointerEvent) {
        this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent("pointerup", event) }));
    }

    onCancel(event: PointerEvent) {
        this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent("pointercancel", event) }));
    }

    onMove(event: PointerEvent) {
        this.webSocket.send(JSON.stringify({ "PointerEvent": new PEvent("pointermove", event) }));
    }
}

function init() {
    span = document.getElementById("debug") as HTMLSpanElement;
    let webSocket = new WebSocket("ws://dirac.freedesk.lan:9001", "touch");
    let canvas = document.getElementById("canvas") as HTMLCanvasElement;
    canvas.width = 1920;
    canvas.height = 1080;
    webSocket.onopen = function(event) {
        webSocket.send(JSON.stringify({ "ClientConfig": new ClientConfig(canvas.clientWidth, canvas.clientHeight) }));
        window.onresize = () => {
            webSocket.send(JSON.stringify({ "ClientConfig": new ClientConfig(canvas.clientWidth, canvas.clientHeight) }));
        }
        let pointerHandler = new PointerHandler(canvas, webSocket);
    }
    webSocket.onmessage = function(event) {
    }


    let videoWebSocket = new WebSocket("ws://dirac.freedesk.lan:9002", "video");
    videoWebSocket.onmessage = (event: MessageEvent) => {
        let img = new Image();
        img.src = "data:image/png;base64," + event.data;
        let ctx = canvas.getContext("2d");
        img.onload = () => { ctx.drawImage(img, 0, 0, canvas.width, canvas.height); };
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
