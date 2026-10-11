// webapis.avplay: a simulated player that goes through AVPlay's states,
// events and timing without a TV, drawing into the Video Plane (ADR-0004).
//
// Sources: developer.samsung.com/smarttv/develop/api-references/
// samsung-product-api-references/avplay-api.html (methods, the states each
// may be called in, exceptions) and guides/multimedia/media-playback/
// using-avplay.html (life cycle, callbacks).
//
// - States go NONE -> IDLE (open) -> READY (prepare) -> PLAYING/PAUSED;
//   stop() returns to IDLE and close() to NONE. A method called in a state
//   the reference doesn't list for it throws InvalidStateError.
// - prepare() and prepareAsync() call onbufferingstart, then
//   onbufferingprogress up to 100, then onbufferingcomplete. The guide names
//   onbufferingstart on prepare; the rest of the order is unverified.
// - play() calls oncurrentplaytime, then again while playing. How often a TV
//   calls it is unverified.
// - At the end of the stream, onstreamcompleted is called and the player
//   stays PLAYING at the end until the App stops it (the guide warns that
//   the last frame stays otherwise). That the state stays PLAYING is
//   unverified.
// - A stream the Engine decodes plays for real, with its own duration;
//   otherwise a clock stands in for it, over SIMULATED_DURATION_MS. The
//   synchronous prepare() can't wait for the Engine to learn which, so until
//   it does (onbufferingcomplete with prepareAsync) the duration is the
//   simulated one. A TV's prepare() returns once it knows the real one.
//
// Not shimmed: B2B-only and seamless methods (getVideoSeamlessInfo,
// setSeamlessLooping, setVideoRoi), sound analysis (deprecated since 2.4),
// suspend/restore/restoreAsync and setVideoStillMode/setDisplayRotation,
// whose arguments and effect the reference doesn't describe well enough to
// imitate, and getCodecInfo. Subtitles and DRM aren't played: their setters
// validate and do nothing (setDrm raises a Diagnostic).

var DRMPLAY_PRIVILEGE = "http://developer.samsung.com/privilege/drmplay";

/**
 * The length of a stream the Engine can't decode, which the simulator can't
 * learn: ten minutes. A TV reports the stream's own.
 */
var SIMULATED_DURATION_MS = 600000;

/** How often oncurrentplaytime is called while playing (unverified). */
var PLAYTIME_INTERVAL_MS = 500;

/** Simulated buffering: progress steps, and the time between them. */
var BUFFERING_STEPS = [25, 50, 75, 100];
var BUFFERING_STEP_MS = 100;

/** How long a seek on the simulated clock takes. */
var SIMULATED_SEEK_MS = 100;

// avplay-api.html#AVPlayManager::setSpeed.
var AVPLAY_SPEEDS = [-16, -8, -4, -2, 1, 2, 4, 8, 16];

var DISPLAY_MODES: { [mode: string]: string } = {
  PLAYER_DISPLAY_MODE_LETTER_BOX: "contain",
  // The guide: "By default, video fills the display area."
  PLAYER_DISPLAY_MODE_FULL_SCREEN: "fill",
  PLAYER_DISPLAY_MODE_AUTO_ASPECT_RATIO: "contain",
};

var STREAMING_PROPERTIES = [
  "COOKIE",
  "USER_AGENT",
  "PREBUFFER_MODE",
  "ADAPTIVE_INFO",
  "SET_MODE_4K",
  "LISTEN_SPARSE_TRACK",
  "IS_LIVE",
  "AVAILABLE_BITRATE",
  "GET_LIVE_DURATION",
  "CURRENT_BANDWIDTH",
  "USE_VIDEOMIXER",
  "SET_MIXEDFRAME",
  "PORTRAIT_MODE",
  "IN_APP_MULTIVIEW",
];

var STREAM_TYPES = ["VIDEO", "AUDIO", "TEXT"];
var DRM_TYPES = ["PLAYREADY", "VERIMATRIX", "WIDEVINE_CDM"];

// The states each group of methods may be called in (avplay-api.html).
var ANY_STATE = ["NONE", "IDLE", "READY", "PLAYING", "PAUSED"];
var OPENED = ["IDLE", "READY", "PLAYING", "PAUSED"];
var PREPARED = ["READY", "PLAYING", "PAUSED"];

function createAVPlay(): any {
  var plane = createVideoPlane();
  var state = "NONE";
  var url = "";
  var listener: any = {};
  // The App's display area; the plane shows only once there is one. That a
  // TV shows no video before setDisplayRect is unverified.
  var rect: DisplayRect | null = null;
  // The <video> playing the stream, once the Engine decodes it.
  var media: HTMLVideoElement | null = null;
  // The simulated clock: the position, and when it last started running.
  var position = 0;
  var runningSince = 0;
  var speed = 1;
  var looping = false;
  var completed = false;
  var seeking = false;
  // While prepareAsync buffers, in IDLE.
  var preparing = false;
  var ticker = 0;
  var streamingProperties: { [type: string]: string } = {};
  // Bumped by stop() and close(), so callbacks of an older stream are dropped.
  var generation = 0;

  function fail(name: string, message: string): never {
    return webapisFail(name, message);
  }

  function requireState(method: string, allowed: string[]) {
    if (allowed.indexOf(state) < 0) {
      fail("InvalidStateError", "webapis.avplay." + method + "() can't be called in the " + state + " state");
    }
  }

  function requireOneOf(value: any, values: string[], argument: string) {
    requireType(value, "string", argument);
    if (values.indexOf(value) < 0) {
      fail("InvalidValuesError", argument + " must be one of " + values.join(", "));
    }
  }

  function requireCallback(value: any, argument: string) {
    if (value !== undefined && value !== null && typeof value !== "function") {
      fail("TypeMismatchError", argument + " must be a function");
    }
  }

  /** Calls the App's listener later, as the player does, never inside a method call. */
  function emit(name: string, args?: any[]) {
    var current = generation;
    setTimeout(function () {
      if (current === generation && listener && typeof listener[name] === "function") {
        listener[name].apply(listener, args || []);
      }
    }, 0);
  }

  function duration(): number {
    if (state === "NONE" || state === "IDLE") {
      return 0;
    }
    if (media && isFinite(media.duration)) {
      return Math.round(media.duration * 1000);
    }
    return SIMULATED_DURATION_MS;
  }

  function currentTime(): number {
    if (media) {
      return Math.round(media.currentTime * 1000);
    }
    var now = position;
    if (runningSince) {
      now += (Date.now() - runningSince) * speed;
    }
    return Math.max(0, Math.min(now, state === "IDLE" ? now : duration()));
  }

  function setClock(ms: number) {
    position = ms;
    runningSince = state === "PLAYING" && !completed ? Date.now() : 0;
  }

  function tick() {
    var now = currentTime();
    emit("oncurrentplaytime", [now]);
    if (!media) {
      plane.caption("AVPlay test pattern: the Engine can't decode " + url + " | " + formatTime(now));
      if (now >= duration() && speed > 0) {
        complete();
      }
    }
  }

  function startTicking() {
    if (!ticker) {
      ticker = setInterval(tick, PLAYTIME_INTERVAL_MS);
    }
  }

  function stopTicking() {
    if (ticker) {
      clearInterval(ticker);
      ticker = 0;
    }
  }

  function complete() {
    if (looping) {
      setClock(0);
      return;
    }
    setClock(duration());
    completed = true;
    runningSince = 0;
    stopTicking();
    emit("onstreamcompleted");
  }

  /** Hands the stream from the simulated clock to the <video> that decodes it. */
  function adopt(video: HTMLVideoElement) {
    var at = currentTime();
    media = video;
    runningSince = 0;
    video.loop = looping;
    video.playbackRate = speed > 0 ? speed : 1;
    try {
      video.currentTime = at / 1000;
    } catch (e) {
      // Not seekable yet: it starts from the beginning.
    }
    video.addEventListener("ended", function () {
      if (media === video) {
        complete();
      }
    });
    if (state === "PLAYING") {
      playMedia();
    }
  }

  function playMedia() {
    if (media) {
      var playing: any = media.play();
      if (playing && typeof playing.catch === "function") {
        playing.catch(function () {});
      }
    }
  }

  /** Loads the stream and simulates its buffering; `ready` follows onbufferingcomplete. */
  function buffer(ready?: () => void) {
    var current = generation;
    var steps = BUFFERING_STEPS.length;
    var probed = false;
    emit("onbufferingstart");
    plane.caption("AVPlay test pattern: the Engine can't decode " + url);
    plane.load(
      url,
      function (video) {
        if (current === generation) {
          adopt(video);
        }
      },
      function () {
        probed = true;
        finish();
      },
    );
    for (var i = 0; i < BUFFERING_STEPS.length; i++) {
      setTimeout(
        (function (percent: number) {
          return function () {
            emit("onbufferingprogress", [percent]);
            steps--;
            finish();
          };
        })(BUFFERING_STEPS[i]),
        (i + 1) * BUFFERING_STEP_MS,
      );
    }
    // The async prepare completes once the Engine knows whether it decodes the stream.
    function finish() {
      if (steps === 0 && (probed || !ready) && current === generation) {
        steps = -1;
        emit("onbufferingcomplete");
        if (ready) {
          setTimeout(function () {
            if (current === generation) {
              ready();
            }
          }, 0);
        }
      }
    }
  }

  /** Forgets the stream, keeping the App's settings (stop) or not (close). */
  function reset() {
    generation++;
    stopTicking();
    plane.unload();
    media = null;
    position = 0;
    runningSince = 0;
    completed = false;
    seeking = false;
    preparing = false;
  }

  /** Moves to `ms`, then calls `done` (the App must wait for it, avplay-api.html). */
  function seek(ms: number, done: () => void) {
    var current = generation;
    seeking = true;
    completed = false;
    var finished = function () {
      if (current === generation) {
        seeking = false;
        done();
      }
    };
    if (media) {
      var video = media;
      var onSeeked = function () {
        video.removeEventListener("seeked", onSeeked);
        finished();
      };
      video.addEventListener("seeked", onSeeked);
      video.currentTime = ms / 1000;
      if (state === "PLAYING") {
        playMedia();
      }
    } else {
      setClock(Math.max(0, state === "IDLE" ? ms : Math.min(ms, duration())));
      setTimeout(finished, SIMULATED_SEEK_MS);
    }
    // A seek after the end of the stream plays on from the new position.
    if (state === "PLAYING") {
      startTicking();
    }
  }

  /** seekTo, jumpForward and jumpBackward: callbacks, not exceptions, after the argument checks. */
  function seekMethod(method: string, allowed: string[], target: (ms: number) => number) {
    return function (milliseconds: any, onsuccess?: () => void, onerror?: (e: any) => void) {
      requireType(milliseconds, "number", "milliseconds");
      requireCallback(onsuccess, "successCallback");
      requireCallback(onerror, "errorCallback");
      if (allowed.indexOf(state) < 0) {
        failLater(onerror, "InvalidStateError", "webapis.avplay." + method + "() can't be called in the " + state + " state", WebapisError);
        return;
      }
      seek(target(milliseconds), function () {
        if (typeof onsuccess === "function") {
          onsuccess();
        }
      });
    };
  }

  /** A setter with nothing to simulate: it validates its argument and state. */
  function setting(method: string, allowed: string[], type: string) {
    return function (value: any) {
      requireType(value, type, "argument");
      requireState(method, allowed);
    };
  }

  function streamInfo(): any[] {
    var width = media ? media.videoWidth : 1920;
    var height = media ? media.videoHeight : 1080;
    return [
      {
        index: 0,
        type: "VIDEO",
        // The reference's format: {fourCC:"H264","Width":"1920","Height":"1080","Bit_rate":" 477000"}.
        extra_info: JSON.stringify({ fourCC: "", Width: String(width), Height: String(height), Bit_rate: "0" }),
      },
    ];
  }

  var avplay: any = {
    open: function (newUrl: any) {
      requireType(newUrl, "string", "url");
      requireState("open", ["NONE", "IDLE"]);
      // That an empty URL is an InvalidValuesError is unverified.
      if (!newUrl) {
        fail("InvalidValuesError", "url must not be empty");
      }
      reset();
      url = newUrl;
      speed = 1;
      streamingProperties = {};
      state = "IDLE";
    },

    close: function () {
      reset();
      rect = null;
      plane.show(null);
      url = "";
      state = "NONE";
    },

    prepare: function () {
      requireState("prepare", ["IDLE", "READY"]);
      if (preparing) {
        fail("InvalidStateError", "webapis.avplay.prepareAsync() is still preparing the stream");
      }
      if (state === "READY") {
        return;
      }
      // The TV blocks until buffered; a page can't, so READY comes at once.
      state = "READY";
      plane.show(rect);
      buffer();
    },

    prepareAsync: function (onsuccess?: () => void, onerror?: (e: any) => void) {
      requireCallback(onsuccess, "successCallback");
      requireCallback(onerror, "errorCallback");
      if (state !== "IDLE" && state !== "READY") {
        failLater(onerror, "InvalidStateError", "webapis.avplay.prepareAsync() can't be called in the " + state + " state", WebapisError);
        return;
      }
      // That a second prepare while one is under way is refused is unverified.
      if (preparing) {
        failLater(onerror, "InvalidStateError", "webapis.avplay.prepareAsync() is already preparing the stream", WebapisError);
        return;
      }
      if (state === "READY") {
        setTimeout(function () {
          if (typeof onsuccess === "function") {
            onsuccess();
          }
        }, 0);
        return;
      }
      preparing = true;
      buffer(function () {
        preparing = false;
        state = "READY";
        plane.show(rect);
        if (typeof onsuccess === "function") {
          onsuccess();
        }
      });
    },

    setDisplayRect: function (x: any, y: any, width: any, height: any) {
      var values = [x, y, width, height];
      for (var i = 0; i < values.length; i++) {
        requireType(values[i], "number", "rectangle");
        // That NaN and infinities are InvalidValuesErrors is unverified.
        if (values[i] < 0 || !isFinite(values[i])) {
          fail("InvalidValuesError", "the display rectangle can't have negative values");
        }
      }
      requireState("setDisplayRect", OPENED);
      rect = { x: x, y: y, width: width, height: height };
      if (state !== "IDLE") {
        plane.show(rect);
      }
    },

    setDisplayMethod: function (mode: any) {
      requireOneOf(mode, Object.keys(DISPLAY_MODES), "displayMode");
      requireState("setDisplayMethod", OPENED);
      plane.fit(DISPLAY_MODES[mode]);
    },

    play: function () {
      requireState("play", PREPARED);
      if (state === "PLAYING") {
        return;
      }
      setClock(currentTime());
      state = "PLAYING";
      if (!completed) {
        runningSince = media ? 0 : Date.now();
        playMedia();
        startTicking();
      }
      tick();
    },

    pause: function () {
      requireState("pause", ["PLAYING", "PAUSED"]);
      if (seeking) {
        fail("InvalidAccessError", "webapis.avplay.pause() can't be called while seeking");
      }
      position = currentTime();
      runningSince = 0;
      if (media) {
        media.pause();
      }
      stopTicking();
      state = "PAUSED";
    },

    stop: function () {
      if (state === "NONE") {
        return;
      }
      reset();
      plane.show(null);
      state = "IDLE";
    },

    seekTo: seekMethod("seekTo", OPENED, function (ms) {
      return ms;
    }),

    jumpForward: seekMethod("jumpForward", PREPARED, function (ms) {
      return currentTime() + ms;
    }),

    jumpBackward: seekMethod("jumpBackward", PREPARED, function (ms) {
      return Math.max(0, currentTime() - ms);
    }),

    getState: function () {
      return state;
    },

    getDuration: duration,

    getCurrentTime: function () {
      return state === "NONE" ? 0 : currentTime();
    },

    setListener: function (callbacks: any) {
      if (!callbacks || typeof callbacks !== "object") {
        fail("TypeMismatchError", "listener must be an AVPlayPlaybackCallback object");
      }
      listener = callbacks;
    },

    setSpeed: function (playbackSpeed: any) {
      requireType(playbackSpeed, "number", "playbackSpeed");
      if (AVPLAY_SPEEDS.indexOf(playbackSpeed) < 0) {
        fail("InvalidValuesError", "playbackSpeed must be one of " + AVPLAY_SPEEDS.join(", "));
      }
      requireState("setSpeed", PREPARED);
      setClock(currentTime());
      speed = playbackSpeed;
      // A <video> can't play backwards; it keeps its forward rate.
      if (media && playbackSpeed > 0) {
        media.playbackRate = playbackSpeed;
      }
    },

    setTimeoutForBuffering: setting("setTimeoutForBuffering", OPENED, "number"),

    setBufferingParam: function (option: any, unit: any, amount: any) {
      requireOneOf(option, ["PLAYER_BUFFER_FOR_PLAY", "PLAYER_BUFFER_FOR_RESUME"], "option");
      requireOneOf(unit, ["PLAYER_BUFFER_SIZE_IN_SECOND", "PLAYER_BUFFER_SIZE_IN_BYTE"], "unit");
      requireType(amount, "number", "amount");
      if (unit === "PLAYER_BUFFER_SIZE_IN_SECOND" && amount < 4) {
        fail("InvalidValuesError", "a time buffer must be at least 4 seconds");
      }
      requireState("setBufferingParam", ["IDLE"]);
    },

    setStreamingProperty: function (type: any, value: any) {
      requireOneOf(type, STREAMING_PROPERTIES, "propertyType");
      requireState("setStreamingProperty", ["IDLE"]);
      streamingProperties[type] = String(value);
    },

    // What a TV reports for the read-only properties (IS_LIVE, ...) is
    // unverified; they read as empty strings.
    getStreamingProperty: function (type: any) {
      requireOneOf(type, STREAMING_PROPERTIES, "propertyType");
      requireState("getStreamingProperty", PREPARED);
      return streamingProperties.hasOwnProperty(type) ? streamingProperties[type] : "";
    },

    getVersion: pluginVersion,

    getCurrentStreamInfo: function () {
      requireState("getCurrentStreamInfo", PREPARED);
      return streamInfo();
    },

    getTotalTrackInfo: function () {
      requireState("getTotalTrackInfo", PREPARED);
      return streamInfo();
    },

    setSelectTrack: function (type: any, index: any) {
      requireOneOf(type, STREAM_TYPES, "type");
      requireType(index, "number", "index");
      requireState("setSelectTrack", PREPARED);
    },

    setSilentSubtitle: setting("setSilentSubtitle", OPENED, "boolean"),
    setExternalSubtitlePath: setting("setExternalSubtitlePath", OPENED, "string"),
    setSubtitlePosition: setting("setSubtitlePosition", ["PLAYING", "PAUSED"], "number"),

    setDrm: function (drmType: any, operation: any, jsonParam: any) {
      requireWebapisPrivilege(DRMPLAY_PRIVILEGE, "webapis.avplay.setDrm");
      requireOneOf(drmType, DRM_TYPES, "drmType");
      requireType(operation, "string", "drmOperation");
      requireType(jsonParam, "string", "jsonParam");
      requireState("setDrm", ["IDLE"]);
      raiseDiagnostic(
        "avplay-drm-not-simulated",
        "webapis.avplay.setDrm(" + drmType + ") was accepted, but tizen-sim doesn't play DRM content: the stream shows the test pattern.",
      );
      return "";
    },

    getUID: function (drmType: any) {
      requireWebapisPrivilege(DRMPLAY_PRIVILEGE, "webapis.avplay.getUID");
      requireOneOf(drmType, DRM_TYPES, "drmType");
      return config.profile.device.duid;
    },
  };
  if (available("webapis.avplay.setLooping")) {
    avplay.setLooping = function (loop: any) {
      requireType(loop, "boolean", "looping");
      requireState("setLooping", OPENED);
      looping = loop;
      if (media) {
        media.loop = loop;
      }
    };
  }
  if (available("webapis.avplay.enableAudioStream")) {
    avplay.enableAudioStream = function () {
      requireState("enableAudioStream", OPENED);
      if (media) {
        media.muted = false;
      }
    };
    avplay.disableAudioStream = function () {
      requireState("disableAudioStream", OPENED);
      if (media) {
        media.muted = true;
      }
    };
  }
  if (available("webapis.avplay.getSubState")) {
    // PRELOAD belongs to a preloading mode the Shim doesn't have.
    avplay.getSubState = function () {
      return state;
    };
  }
  if (available("webapis.avplay.getVideoSize")) {
    avplay.getVideoSize = function () {
      requireState("getVideoSize", OPENED);
      return media ? { width: media.videoWidth, height: media.videoHeight } : { width: 1920, height: 1080 };
    };
  }
  return avplay;
}

/** `m:ss` for the test pattern's caption. */
function formatTime(ms: number): string {
  var seconds = Math.floor(ms / 1000);
  var rest = seconds % 60;
  return Math.floor(seconds / 60) + ":" + (rest < 10 ? "0" : "") + rest;
}
