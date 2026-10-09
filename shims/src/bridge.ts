// The Bridge: JSON-RPC 2.0 notifications from the page to the Host over the
// dev server's WebSocket. Messages sent before the socket opens are queued.

var bridge = (function () {
  var queue: string[] = [];
  var socket: WebSocket | null = null;

  function connect() {
    socket = new WebSocket(config.bridgeUrl);
    socket.onopen = function () {
      while (queue.length) {
        socket!.send(queue.shift()!);
      }
    };
    socket.onclose = function () {
      if (queue.length && typeof console !== "undefined") {
        console.error("[tizen-sim] the Bridge to the simulator closed; " + queue.length + " message(s) were not delivered");
      }
    };
  }

  function send(text: string) {
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send(text);
    } else {
      queue.push(text);
    }
  }

  connect();

  return {
    notify: function (method: string, params?: any) {
      send(JSON.stringify({ jsonrpc: "2.0", method: method, params: params || {} }));
    },
  };
})();

/** Tells the developer something would behave differently on a real TV. */
function raiseDiagnostic(code: string, message: string) {
  if (typeof console !== "undefined" && console.warn) {
    console.warn("[tizen-sim] " + message);
  }
  bridge.notify("diagnostic", { code: code, message: message });
}
