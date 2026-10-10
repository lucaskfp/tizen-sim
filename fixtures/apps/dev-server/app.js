// Stands in for a bundler's HMR client: it applies each update pushed over
// the HMR WebSocket in place, without reloading the page.
window.hmrVersion = 1;
var socket = new WebSocket('ws://' + location.host + '/hmr', 'fixture-hmr');
socket.onopen = function () {
  window.hmrConnected = true;
};
socket.onmessage = function (event) {
  var update = JSON.parse(event.data);
  window.hmrVersion = update.version;
  socket.send('applied ' + update.version);
};
