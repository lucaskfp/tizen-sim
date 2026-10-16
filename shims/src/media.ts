// Media formats a TV plays and the Engine doesn't: the
// `media-format-unsupported` Diagnostic.
//
// Every year's TV video specifications list H.264 and AAC in MP4 (TV
// specifications/media-specifications/<year>-tv-video-specifications.html,
// 2017 to 2026). The Engines up to Tizen 8.0 are Chromium snapshots, built
// without those codecs: they answer "" to canPlayType, false to
// MediaSource.isTypeSupported, and fail such a stream with
// MEDIA_ERR_SRC_NOT_SUPPORTED (measured on every Windows Engine, #105). Their answers
// are left as they are, since the stream can't play anyway; the Diagnostic
// tells the developer a TV would play it.
//
// Only H.264, AAC and MP4 itself are recognised. Other formats a TV lists
// (HEVC, AC-3, MPEG-TS...) aren't checked: which of them each TV model
// decodes varies. A failed stream counts as MP4 by its <source type> or its
// URL's extension; one served under another name goes unreported.

/** The MP4 MIME types, and the file extensions that stand for them. */
var MP4_TYPES = ["video/mp4", "audio/mp4", "video/x-m4v", "audio/x-m4a"];
var MP4_EXTENSIONS: { [extension: string]: string } = { mp4: "video/mp4", m4v: "video/mp4", m4a: "audio/mp4" };

/**
 * A codec a TV decodes in MP4: its RFC 6381 prefixes, and a type the Engine
 * is asked about to learn whether it has the codec.
 */
var TV_CODECS = [
  { name: "H.264", prefixes: ["avc1.", "avc3."], probe: 'video/mp4; codecs="avc1.42E01E"' },
  { name: "AAC", prefixes: ["mp4a."], probe: 'audio/mp4; codecs="mp4a.40.2"' },
];

var nativeCanPlayType = HTMLMediaElement.prototype.canPlayType;
var reported: { [key: string]: boolean } = {};

/** An MP4 format, with the codecs it names (none when only the container is known). */
interface TvFormat {
  codecs: string[];
}

/** The format `mime` names, or null when it isn't MP4 or names a codec not recognised here. */
function tvFormat(mime: string): TvFormat | null {
  var parts = String(mime).split(";");
  if (MP4_TYPES.indexOf(parts[0].replace(/\s/g, "").toLowerCase()) < 0) {
    return null;
  }
  var codecs: string[] = [];
  for (var i = 1; i < parts.length; i++) {
    var parameter = /^\s*codecs\s*=\s*"?([^"]*)"?\s*$/i.exec(parts[i]);
    if (!parameter) {
      continue;
    }
    var listed = parameter[1].split(",");
    for (var j = 0; j < listed.length; j++) {
      var name = codecName(listed[j].replace(/\s/g, ""));
      if (!name) {
        return null;
      }
      if (codecs.indexOf(name) < 0) {
        codecs.push(name);
      }
    }
  }
  return { codecs: codecs };
}

function codecName(codec: string): string | null {
  for (var i = 0; i < TV_CODECS.length; i++) {
    for (var j = 0; j < TV_CODECS[i].prefixes.length; j++) {
      if (codec.toLowerCase().indexOf(TV_CODECS[i].prefixes[j]) === 0) {
        return TV_CODECS[i].name;
      }
    }
  }
  return null;
}

/**
 * The codecs of `format` the Engine lacks; for a bare container, those of
 * H.264 and AAC it lacks.
 */
function missingCodecs(format: TvFormat): string[] {
  var missing: string[] = [];
  for (var i = 0; i < TV_CODECS.length; i++) {
    var codec = TV_CODECS[i];
    var used = !format.codecs.length || format.codecs.indexOf(codec.name) >= 0;
    if (used && nativeCanPlayType.call(document.createElement("video"), codec.probe) === "") {
      missing.push(codec.name);
    }
  }
  return missing;
}

/**
 * Raises the Diagnostic if the Engine lacks a codec of `format`, once per
 * format and `source`: the API the App called, or the stream's URL.
 * `attempt` says what the App did.
 */
function reportUnplayable(format: TvFormat, attempt: string, source: string) {
  var missing = missingCodecs(format);
  var key = source + " " + format.codecs.join("/");
  if (!missing.length || reported[key]) {
    return;
  }
  reported[key] = true;
  var engine = "the Engine of this Version Profile (Chromium M" + config.profile.milestone + ") has no " +
    missing.join(" and ") + ". A Tizen " + config.profile.version + " TV plays ";
  raiseDiagnostic(
    "media-format-unsupported",
    attempt + ": " + engine +
      (format.codecs.length
        ? format.codecs.join("/") + " in MP4. Check this stream on a TV, or on a Version Profile whose Engine plays it."
        : "H.264 and AAC in MP4, so if this stream uses them, check it on a TV or on a Version Profile whose Engine plays it."),
  );
}

/** A media URL's MP4 type, from its file extension. */
function mp4TypeOf(url: string): string | null {
  var path = String(url).split(/[?#]/)[0];
  var extension = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
  return MP4_EXTENSIONS.hasOwnProperty(extension) ? MP4_EXTENSIONS[extension] : null;
}

/** For a <video>, <audio> or <source> that failed (AVPlay's too): reports an MP4 stream the Engine can't decode. */
function reportMediaError(element: HTMLElement) {
  var url = (element as any).currentSrc || (element as any).src || "";
  var mime = (element instanceof HTMLSourceElement && element.type) || mp4TypeOf(url);
  var format = mime ? tvFormat(mime) : null;
  if (format) {
    reportUnplayable(format, "The Engine can't play " + url, url);
  }
}

function installMediaChecks() {
  // canPlayType and isTypeSupported: an App deciding what to play.
  HTMLMediaElement.prototype.canPlayType = function (this: HTMLMediaElement, mime: string) {
    var answer = nativeCanPlayType.call(this, mime);
    var format = answer === "" ? tvFormat(mime) : null;
    if (format) {
      reportUnplayable(format, "canPlayType('" + mime + "') answered \"\"", "canPlayType");
    }
    return answer;
  };
  var MediaSourceClass: any = (window as any).MediaSource;
  if (MediaSourceClass) {
    var nativeIsTypeSupported = MediaSourceClass.isTypeSupported;
    MediaSourceClass.isTypeSupported = function (mime: string) {
      var answer = nativeIsTypeSupported.call(MediaSourceClass, mime);
      var format = answer ? null : tvFormat(mime);
      if (format) {
        reportUnplayable(format, "MediaSource.isTypeSupported('" + mime + "') answered false", "isTypeSupported");
      }
      return answer;
    };
    var nativeAddSourceBuffer = MediaSourceClass.prototype.addSourceBuffer;
    MediaSourceClass.prototype.addSourceBuffer = function (mime: string) {
      try {
        return nativeAddSourceBuffer.call(this, mime);
      } catch (e) {
        var format = tvFormat(mime);
        if (format) {
          reportUnplayable(format, "MediaSource.addSourceBuffer('" + mime + "') failed", "addSourceBuffer");
        }
        throw e;
      }
    };
  }

  // A stream that fails to load. Media errors don't bubble, so this listens
  // in the capture phase; play() covers elements outside the document.
  var MEDIA_ERR_SRC_NOT_SUPPORTED = 4;
  function onError(event: Event) {
    var target = event.target as any;
    if (target instanceof HTMLSourceElement) {
      reportMediaError(target);
    } else if (target instanceof HTMLMediaElement && target.error && target.error.code === MEDIA_ERR_SRC_NOT_SUPPORTED) {
      reportMediaError(target);
    }
  }
  document.addEventListener("error", onError, true);
  var nativePlay = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function (this: any) {
    if (!this.__tizenSimWatched) {
      Object.defineProperty(this, "__tizenSimWatched", { value: true });
      this.addEventListener("error", function (event: Event) {
        if (!document.documentElement.contains(event.target as Node)) {
          onError(event);
        }
      });
    }
    return nativePlay.apply(this, arguments as any);
  };
}
