// A test App for tizen-sim's media checks: what the Engine plays of the
// formats Tizen TVs list, and the `media-format-unsupported` Diagnostics.
// ES5, so it runs on every Version Profile down to Tizen 3.0.
(function () {
  var TYPES = [
    'video/mp4; codecs="avc1.42E01E, mp4a.40.2"',
    'video/mp4; codecs="hev1.1.6.L93.B0"',
    'video/mp4; codecs="av01.0.05M.08"',
    'video/webm; codecs="vp9"',
    'video/webm; codecs="vp8, vorbis"',
    'audio/mp4; codecs="mp4a.40.2"',
    'audio/mp4; codecs="ac-3"',
    'audio/mp4; codecs="ec-3"',
    'video/mp2t; codecs="avc1.42E01E, mp4a.40.2"',
    'video/x-matroska',
    'video/quicktime',
    'audio/mpeg',
    'audio/flac',
    'audio/x-ms-wma',
    'application/vnd.apple.mpegurl'
  ];

  function mediaUrl(name) {
    return location.href.replace(/[^\/]*$/, '') + 'media/' + name;
  }

  // The table: the Engine's own answers. Asking is enough for a Diagnostic
  // when a TV plays the format and the Engine says no.
  var rows = document.getElementById('typeRows');
  var probe = document.createElement('video');
  for (var i = 0; i < TYPES.length; i++) {
    var answer = probe.canPlayType(TYPES[i]);
    var mse = window.MediaSource ? MediaSource.isTypeSupported(TYPES[i]) : null;
    var row = document.createElement('tr');
    row.innerHTML = '<td></td><td class="' + (answer ? '' : 'no') + '">' + (answer || '""') + '</td>' +
      '<td class="' + (mse ? '' : 'no') + '">' + (mse === null ? 'no MSE' : mse) + '</td>';
    row.firstChild.textContent = TYPES[i];
    rows.appendChild(row);
  }

  var video = document.getElementById('video');

  function playInVideo(name, done) {
    video.onerror = function () { done('failed: error ' + (video.error && video.error.code), true); };
    video.oncanplay = function () {
      done('plays (' + video.videoWidth + 'x' + video.videoHeight + ')');
      var playing = video.play();
      if (playing && playing.catch) playing.catch(function () {});
    };
    video.src = mediaUrl(name);
  }

  function playInAVPlay(name, done) {
    var player = webapis.avplay;
    try {
      if (player.getState() !== 'NONE') player.close();
      player.open(name.indexOf('://') > 0 ? name : mediaUrl(name));
      player.setListener({
        onerror: function (e) { done('onerror ' + e, true); }
      });
      // Beside the <video> panel, in 1920x1080 coordinates.
      player.setDisplayRect(1440, 620, 440, 248);
      player.prepareAsync(function () {
        player.play();
        done('state ' + player.getState() + ', duration ' + player.getDuration() + ' ms' +
          (player.getDuration() === 600000 ? ' (simulated: the Engine can\'t decode it)' : ''));
      }, function (e) { done('prepareAsync failed: ' + e.name, true); });
    } catch (e) {
      done('threw ' + e.name + ': ' + e.message, true);
    }
  }

  function addSourceBuffer(type, done) {
    var source = new MediaSource();
    source.addEventListener('sourceopen', function () {
      try {
        source.addSourceBuffer(type);
        done('added');
      } catch (e) {
        done('threw ' + e.name, true);
      }
    });
    var element = document.createElement('video');
    element.src = URL.createObjectURL(source);
  }

  var ACTIONS = [
    ['<video>: H.264/AAC MP4', function (done) { playInVideo('clip.mp4', done); }],
    ['<video>: VP8 WebM (plays on every Engine)', function (done) { playInVideo('clip.webm', done); }],
    ['<video>: MPEG-TS file name (missing file)', function (done) { playInVideo('live/segment.ts', done); }],
    ['AVPlay: H.264/AAC MP4', function (done) { playInAVPlay('clip.mp4', done); }],
    ['AVPlay: VP8 WebM', function (done) { playInAVPlay('clip.webm', done); }],
    ['AVPlay: HLS (.m3u8)', function (done) { playInAVPlay('live/stream.m3u8', done); }],
    ['AVPlay: MPEG-DASH (.mpd)', function (done) { playInAVPlay('live/stream.mpd', done); }],
    ['MSE addSourceBuffer: H.264/AAC MP4', function (done) { addSourceBuffer('video/mp4; codecs="avc1.42E01E, mp4a.40.2"', done); }],
    ['MSE addSourceBuffer: AC-3 in MP4', function (done) { addSourceBuffer('audio/mp4; codecs="ac-3"', done); }],
    ['canPlayType: Dolby Vision (no TV lists it: no Diagnostic)', function (done) {
      done('answered "' + probe.canPlayType('video/mp4; codecs="dvh1.05.06"') + '"');
    }]
  ];

  var buttons = [];
  var list = document.getElementById('buttons');
  for (var j = 0; j < ACTIONS.length; j++) {
    (function (label, run) {
      var button = document.createElement('button');
      var title = document.createElement('span');
      var result = document.createElement('span');
      title.textContent = label;
      result.className = 'result';
      button.appendChild(title);
      button.appendChild(result);
      button.onclick = function () {
        result.className = 'result';
        result.textContent = '...';
        run(function (text, failed) {
          result.textContent = text;
          result.className = failed ? 'result failed' : 'result';
          if (window.console) console.log('[media-formats] ' + label + ': ' + text);
        });
      };
      list.appendChild(button);
      buttons.push(button);
    })(ACTIONS[j][0], ACTIONS[j][1]);
  }

  // Arrows and Enter reach every App without registering them; Enter runs
  // the focused action from keydown, as a remote delivers it.
  var focused = 0;
  buttons[0].focus();
  document.addEventListener('keydown', function (event) {
    if (event.keyCode === 38 || event.keyCode === 40) {
      focused = (focused + (event.keyCode === 40 ? 1 : buttons.length - 1)) % buttons.length;
      buttons[focused].focus();
      event.preventDefault();
    } else if (event.keyCode === 13) {
      buttons[focused].onclick();
      event.preventDefault();
    }
  });
})();
