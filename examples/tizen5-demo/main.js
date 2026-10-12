// A small Tizen 5.0 TV App to try tizen-sim with. ES5 on purpose, so it
// runs as written on the TV's Chromium M63.
(function () {
  'use strict';

  var events = document.getElementById('events');

  function pad(n) {
    return (n < 10 ? '0' : '') + n;
  }

  function log(text) {
    var now = new Date();
    var item = document.createElement('li');
    item.textContent = pad(now.getHours()) + ':' + pad(now.getMinutes()) + ':' + pad(now.getSeconds()) + '  ' + text;
    events.insertBefore(item, events.firstChild);
    while (events.children.length > 8) {
      events.removeChild(events.lastChild);
    }
    console.log(text);
  }

  function tick() {
    var now = new Date();
    document.getElementById('clock').textContent = pad(now.getHours()) + ':' + pad(now.getMinutes());
  }
  tick();
  setInterval(tick, 1000);

  // Device information, from tizen.systeminfo and the Samsung Product API.
  var info = document.getElementById('info');
  function row(label, read) {
    var value;
    try {
      value = read();
    } catch (e) {
      value = e.name + ': ' + e.message;
    }
    var dt = document.createElement('dt');
    var dd = document.createElement('dd');
    dt.textContent = label;
    dd.textContent = value;
    info.appendChild(dt);
    info.appendChild(dd);
  }
  row('Platform', function () {
    return tizen.systeminfo.getCapability('http://tizen.org/feature/platform.version');
  });
  row('Model', function () { return webapis.productinfo.getRealModel(); });
  row('Firmware', function () { return webapis.productinfo.getFirmware(); });
  row('UHD panel', function () { return webapis.productinfo.isUdPanelSupported() ? 'yes' : 'no'; });
  row('TV name', function () { return webapis.network.getTVName(); });
  row('Engine', function () {
    var match = navigator.userAgent.match(/Chrome\/([\d.]+)/);
    return match ? 'Chromium ' + match[1] : navigator.userAgent.replace(/^.*?\(/, '(').split(')')[0] + ')';
  });

  // Remote Keys: the color keys only arrive once registered.
  var tiles = Array.prototype.slice.call(document.querySelectorAll('.tile'));
  var tileByCode = {};
  tiles.forEach(function (tile) {
    var name = tile.getAttribute('data-key');
    try {
      tizen.tvinputdevice.registerKey(name);
      tileByCode[tizen.tvinputdevice.getKey(name).code] = tile;
    } catch (e) {
      log('registerKey(' + name + ') failed: ' + e.name);
    }
  });
  tiles[0].focus();

  function light(tile) {
    tile.classList.add('lit');
    setTimeout(function () { tile.classList.remove('lit'); }, 600);
  }

  document.addEventListener('keydown', function (event) {
    var index = tiles.indexOf(document.activeElement);
    document.getElementById('lastKey').textContent = 'Last key code: ' + event.keyCode;
    switch (event.keyCode) {
      case 37:
        tiles[Math.max(0, index - 1)].focus();
        break;
      case 39:
        tiles[Math.min(tiles.length - 1, index + 1)].focus();
        break;
      case 13:
        if (index >= 0) {
          light(tiles[index]);
          log('Enter on ' + tiles[index].getAttribute('data-key'));
        }
        break;
      case 10009:
        log('Back pressed');
        break;
      default:
        if (tileByCode[event.keyCode]) {
          light(tileByCode[event.keyCode]);
          log('Key ' + tileByCode[event.keyCode].getAttribute('data-key'));
        } else {
          log('Key code ' + event.keyCode);
        }
    }
  });

  // Leaving the App (Home, another App) hides it.
  document.addEventListener('visibilitychange', function () {
    log(document.hidden ? 'App hidden' : 'App visible again');
  });

  // A launch counter in the App's private storage, with the 5.0 file API.
  var note = document.getElementById('fsNote');
  try {
    var path = 'wgt-private/launches.txt';
    var count = 0;
    if (tizen.filesystem.pathExists(path)) {
      var reader = tizen.filesystem.openFile(path, 'r');
      count = parseInt(reader.readString(), 10) || 0;
      reader.close();
    }
    count += 1;
    var writer = tizen.filesystem.openFile(path, 'w');
    writer.writeString(String(count));
    writer.close();
    document.getElementById('launches').textContent = count;
    note.textContent = 'Stored in ' + path + ' with tizen.filesystem.openFile (since 5.0).';
  } catch (e) {
    note.textContent = e.name + ': ' + e.message;
  }

  log('App started on Tizen ' + tizen.systeminfo.getCapability('http://tizen.org/feature/platform.version'));
})();
