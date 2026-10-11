// The Bridge: JSON-RPC 2.0 notifications from the page to the Host over the
// dev server's WebSocket. Messages sent before the socket opens are queued.
// Calls the page must wait on, such as tizen.filesystem's synchronous
// methods, are blocking POSTs to the same path instead (ADR-0008).

var bridge = (function () {
  var queue: string[] = [];
  var socket: WebSocket | null = null;
  var callUrl = config.bridgeUrl.replace(/^ws:/, "http:");
  var lastCallId = 0;

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
    /**
     * Calls the Host and waits for its result. A failure throws the
     * WebAPIException the Host names.
     */
    call: function (method: string, params: any): any {
      var request = new XMLHttpRequest();
      request.open("POST", callUrl, false);
      request.setRequestHeader("Content-Type", "text/plain");
      try {
        request.send(JSON.stringify({ jsonrpc: "2.0", id: ++lastCallId, method: method, params: params }));
      } catch (e) {
        throw new WebAPIException("UnknownError", "The simulator did not answer " + method + ": " + e);
      }
      if (request.status !== 200) {
        throw new WebAPIException("UnknownError", "The simulator refused " + method + " (HTTP " + request.status + ")");
      }
      var reply = JSON.parse(request.responseText);
      if (reply.error) {
        throw new WebAPIException((reply.error.data && reply.error.data.name) || "UnknownError", reply.error.message);
      }
      return reply.result;
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
