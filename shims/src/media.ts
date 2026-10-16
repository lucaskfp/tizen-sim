// Media formats a TV plays and the Engine doesn't: the
// `media-format-unsupported` Diagnostic.
//
// What each Version Profile's TVs play comes from Samsung's TV video
// specifications for that year, one table per model group, and the Music
// table of the media specifications index (config.profile.media, from
// crates/tizen-sim-core/data/media.toml). The Engines up to Tizen 8.0 are
// Chromium snapshots, built without H.264, AAC, HEVC or Dolby audio, and no
// Engine plays MPEG-TS, AC-3 or E-AC-3 (measured on every Windows Engine,
// #105). The Engine's own answers are left as they are, since the stream
// can't play anyway; the Diagnostic tells the developer a TV would play it.
//
// - canPlayType, MediaSource.isTypeSupported and addSourceBuffer: the type
//   names the container and codecs. Which formats a TV's MSE takes is
//   documented only for H.264 (up to FHD) and WebM VP9, so for others the
//   Diagnostic says that is unverified.
// - A <video>, <audio> or <source> that fails with MEDIA_ERR_SRC_NOT_SUPPORTED:
//   the format comes from its <source type> or its URL's extension; a stream
//   served under another name goes unreported.
// - AVPlay: its streams too, and the adaptive streaming formats AVPlay plays
//   on a TV (TV guides/multimedia/adaptive-streaming.html): MPEG-DASH (.mpd),
//   Smooth Streaming (.ism/Manifest) and HLS (.m3u8).
// - Only formats the year lists count, and only those a type can name: the
//   containers and codecs below. A format only some model groups list says so.

/** Containers by MIME type; the first type of each is how the Engine is asked about it. */
var CONTAINER_TYPES: { [container: string]: string[] } = {
  MP4: ["video/mp4", "audio/mp4", "video/x-m4v", "audio/x-m4a"],
  MOV: ["video/quicktime"],
  "3GP": ["video/3gpp", "audio/3gpp", "video/3gpp2", "audio/3gpp2"],
  MKV: ["video/x-matroska", "audio/x-matroska", "video/matroska", "audio/matroska"],
  "MPEG-TS": ["video/mp2t"],
  AVI: ["video/x-msvideo", "video/avi", "video/msvideo"],
  ASF: ["video/x-ms-asf", "video/x-ms-wmv", "application/vnd.ms-asf"],
  FLV: ["video/x-flv"],
  "MPEG-PS": ["video/mpeg"],
  WebM: ["video/webm", "audio/webm"],
  MP3: ["audio/mpeg", "audio/mp3"],
  AAC: ["audio/aac", "audio/aacp", "audio/x-aac"],
  FLAC: ["audio/flac", "audio/x-flac"],
  Ogg: ["audio/ogg"],
  WAV: ["audio/wav", "audio/wave", "audio/x-wav"],
  WMA: ["audio/x-ms-wma"],
};

/** Containers by file extension, for a failed stream with no type. */
var CONTAINER_EXTENSIONS: { [extension: string]: string } = {
  mp4: "MP4", m4v: "MP4", m4a: "MP4", mov: "MOV", "3gp": "3GP", "3g2": "3GP", mkv: "MKV",
  ts: "MPEG-TS", tp: "MPEG-TS", trp: "MPEG-TS", m2ts: "MPEG-TS", mts: "MPEG-TS", avi: "AVI",
  asf: "ASF", wmv: "ASF", flv: "FLV", mpg: "MPEG-PS", mpeg: "MPEG-PS", webm: "WebM",
  mp3: "MP3", aac: "AAC", flac: "FLAC", ogg: "Ogg", oga: "Ogg", wav: "WAV", wma: "WMA",
};

/**
 * Codecs: their RFC 6381 identifiers (one ending in "." matches as a prefix;
 * the others alone or followed by "."), and a type that asks the Engine about
 * the codec alone. Order matters: HE-AAC and MP3 before AAC.
 */
var CODECS = [
  { name: "H.264", ids: ["avc1.", "avc3."], probe: 'video/mp4; codecs="avc1.42E01E"' },
  { name: "HEVC", ids: ["hev1.", "hvc1."], probe: 'video/mp4; codecs="hev1.1.6.L93.B0"' },
  { name: "AV1", ids: ["av01."], probe: 'video/mp4; codecs="av01.0.05M.08"' },
  { name: "VP8", ids: ["vp8"], probe: 'video/webm; codecs="vp8"' },
  { name: "VP9", ids: ["vp9", "vp09."], probe: 'video/webm; codecs="vp9"' },
  { name: "MPEG-4 Part 2", ids: ["mp4v.20"], probe: 'video/mp4; codecs="mp4v.20.8"' },
  { name: "HE-AAC", ids: ["mp4a.40.5", "mp4a.40.29"], probe: 'audio/mp4; codecs="mp4a.40.5"' },
  { name: "MP3", ids: ["mp3", "mp4a.69", "mp4a.6b", "mp4a.40.34"], probe: "audio/mpeg" },
  { name: "AAC", ids: ["mp4a.40.", "mp4a.66", "mp4a.67", "mp4a.68"], probe: 'audio/mp4; codecs="mp4a.40.2"' },
  { name: "AC-3", ids: ["ac-3", "mp4a.a5"], probe: 'audio/mp4; codecs="ac-3"' },
  { name: "E-AC-3", ids: ["ec-3", "mp4a.a6"], probe: 'audio/mp4; codecs="ec-3"' },
  { name: "AC-4", ids: ["ac-4"], probe: 'audio/mp4; codecs="ac-4.02.01.01"' },
  { name: "MPEG-H", ids: ["mhm1", "mha1"], probe: 'audio/mp4; codecs="mhm1.0x0D"' },
  { name: "Opus", ids: ["opus"], probe: 'audio/webm; codecs="opus"' },
  { name: "Vorbis", ids: ["vorbis"], probe: 'audio/webm; codecs="vorbis"' },
  { name: "DTS", ids: ["dtsc", "dtse", "dtsh", "dtsl", "dtsx"], probe: 'audio/mp4; codecs="dtsc"' },
  { name: "FLAC", ids: ["flac"], probe: "audio/flac" },
  { name: "LPCM", ids: ["1"], probe: 'audio/wav; codecs="1"' },
  { name: "WMA", ids: [], probe: "audio/x-ms-wma" },
];

/** AVPlay's adaptive streaming formats, by how their URLs end. */
var ADAPTIVE_FORMATS = [
  { name: "MPEG-DASH", pattern: /\.mpd$/i },
  { name: "Smooth Streaming", pattern: /\.ism\/manifest$/i },
  { name: "HLS", pattern: /\.m3u8$/i },
];

var nativeCanPlayType = HTMLMediaElement.prototype.canPlayType;
var reported: { [key: string]: boolean } = {};

/** A container and the codecs a type names (none when it names only the container). */
interface NamedFormat {
  container: string;
  codecs: string[];
}

function engineCanPlay(mime: string): boolean {
  return nativeCanPlayType.call(document.createElement("video"), mime) !== "";
}

function containerOf(mime: string): string | null {
  var type = mime.replace(/\s/g, "").toLowerCase();
  for (var container in CONTAINER_TYPES) {
    if (CONTAINER_TYPES.hasOwnProperty(container) && CONTAINER_TYPES[container].indexOf(type) >= 0) {
      return container;
    }
  }
  return null;
}

function codecNamed(id: string): string | null {
  var lowered = id.toLowerCase();
  for (var i = 0; i < CODECS.length; i++) {
    for (var j = 0; j < CODECS[i].ids.length; j++) {
      var known = CODECS[i].ids[j];
      var matches = known.charAt(known.length - 1) === "."
        ? lowered.indexOf(known) === 0
        : lowered === known || lowered.indexOf(known + ".") === 0;
      if (matches) {
        return CODECS[i].name;
      }
    }
  }
  return null;
}

/** The format a MIME type names, or null when it names something not recognised here. */
function namedFormat(mime: string): NamedFormat | null {
  var parts = String(mime).split(";");
  var container = containerOf(parts[0]);
  if (!container) {
    return null;
  }
  var codecs: string[] = [];
  for (var i = 1; i < parts.length; i++) {
    var parameter = /^\s*codecs\s*=\s*"?([^"]*)"?\s*$/i.exec(parts[i]);
    if (!parameter) {
      continue;
    }
    var ids = parameter[1].split(",");
    for (var j = 0; j < ids.length; j++) {
      var name = codecNamed(ids[j].replace(/\s/g, ""));
      if (!name) {
        return null;
      }
      if (codecs.indexOf(name) < 0) {
        codecs.push(name);
      }
    }
  }
  return { container: container, codecs: codecs };
}

/** The codecs the year lists in `container`. */
function codecsListedIn(container: string): string[] {
  var formats = config.profile.media!.formats;
  var codecs: string[] = [];
  for (var i = 0; i < formats.length; i++) {
    if (formats[i].containers.indexOf(container) >= 0) {
      codecs = codecs.concat(formats[i].codecs);
    }
  }
  return codecs;
}

/** The model groups that list `codec`, or null when every group does. */
function groupsListing(codec: string): string[] | null {
  var some = config.profile.media!.someModels;
  return some.hasOwnProperty(codec) ? some[codec] : null;
}

/**
 * Which of the year's model groups play `format`: "all", the groups, or null
 * when no model does. Every codec it names must be listed in its container; a
 * bare container plays on the models that play any codec in it.
 */
function modelsPlaying(format: NamedFormat): "all" | string[] | null {
  var listed = codecsListedIn(format.container);
  if (!listed.length) {
    return null;
  }
  var codecs = format.codecs.length ? format.codecs : listed;
  var models: "all" | string[] = format.codecs.length ? "all" : [];
  for (var i = 0; i < codecs.length; i++) {
    if (listed.indexOf(codecs[i]) < 0) {
      return null;
    }
    var groups = groupsListing(codecs[i]);
    if (format.codecs.length) {
      if (groups) {
        models = models === "all" ? groups : intersect(models as string[], groups);
      }
    } else if (!groups) {
      return "all";
    } else {
      models = union(models as string[], groups);
    }
  }
  return models;
}

function intersect(a: string[], b: string[]): string[] {
  var both: string[] = [];
  for (var i = 0; i < a.length; i++) {
    if (b.indexOf(a[i]) >= 0) {
      both.push(a[i]);
    }
  }
  return both;
}

function union(a: string[], b: string[]): string[] {
  var either = a.slice();
  for (var i = 0; i < b.length; i++) {
    if (either.indexOf(b[i]) < 0) {
      either.push(b[i]);
    }
  }
  return either;
}

/** What the Engine lacks of `codecs` in `container`: the container itself, or the codecs. */
function engineLacks(container: string, codecs: string[]): string[] {
  if (!engineCanPlay(CONTAINER_TYPES[container][0])) {
    return [container];
  }
  var missing: string[] = [];
  for (var i = 0; i < CODECS.length; i++) {
    if (codecs.indexOf(CODECS[i].name) >= 0 && !engineCanPlay(CODECS[i].probe)) {
      missing.push(CODECS[i].name);
    }
  }
  return missing;
}

function formatName(format: NamedFormat): string {
  return format.codecs.length ? format.codecs.join("/") + " in " + format.container : format.container;
}

/** "a, b and c" */
function inWords(names: string[]): string {
  return names.length > 1 ? names.slice(0, -1).join(", ") + " and " + names[names.length - 1] : names[0];
}

/** "the Engine of this Version Profile (Chromium M108) has no X" */
function engineHasNo(missing: string[]): string {
  return "the Engine of this Version Profile (Chromium M" + config.profile.milestone + ")" +
    (missing.length ? " has no " + inWords(missing) : " can't play it");
}

/** Where the year lists `container`: its video specifications, or the Music table for an audio-only format. */
function sourcePage(container: string): string {
  var formats = config.profile.media!.formats;
  for (var i = 0; i < formats.length; i++) {
    if (!formats[i].music && formats[i].containers.indexOf(container) >= 0) {
      return " (TV " + config.profile.media!.page + ")";
    }
  }
  return " (TV specifications/media-specifications.html, Music)";
}

var CHECK_ELSEWHERE = " Check it on a TV, or on a Version Profile whose Engine plays it.";

/** Raises the Diagnostic once per `key`. */
function report(key: string, message: string) {
  if (!reported[key]) {
    reported[key] = true;
    raiseDiagnostic("media-format-unsupported", message);
  }
}

/**
 * For a type the Engine refused: reports it if the year's TVs play it.
 * `attempt` says what the App did; `source` is the API or the stream's URL.
 */
function reportRefusedType(mime: string, attempt: string, source: string) {
  if (!config.profile.media) {
    return;
  }
  var format = namedFormat(mime);
  var models = format ? modelsPlaying(format) : null;
  if (!format || !models) {
    return;
  }
  var name = formatName(format);
  var version = config.profile.version;
  var tvs = models === "all"
    ? "Tizen " + version + " TVs play " + name
    : "Only some Tizen " + version + " TV models (" + models.length + " of " +
      config.profile.media!.modelGroups.length + " model groups) play " + name;
  // The MSE section documents H.264 and WebM VP9 only.
  var documentedForMse = format.codecs.length > 0 && !(format.codecs.join(",") + ",").replace(/(H\.264|AAC|VP9),/g, "");
  var mse = source === "isTypeSupported" || source === "addSourceBuffer";
  report(
    source + " " + name,
    attempt + ": " + engineHasNo(engineLacks(format.container, format.codecs)) + ". " + tvs + sourcePage(format.container) + "." +
      (mse && !documentedForMse ? " Whether a TV's MSE takes it is unverified." : "") + CHECK_ELSEWHERE,
  );
}

/** For a <video>, <audio> or <source> that failed (AVPlay's too): reports a format the Engine can't play. */
function reportMediaError(element: HTMLElement) {
  if (!config.profile.media) {
    return;
  }
  var url = (element as any).currentSrc || (element as any).src || "";
  var mime = element instanceof HTMLSourceElement ? element.type : "";
  if (mime) {
    reportRefusedType(mime, "The Engine can't play " + url, url);
    return;
  }
  var path = String(url).split(/[?#]/)[0];
  var extension = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
  var container = CONTAINER_EXTENSIONS.hasOwnProperty(extension) ? CONTAINER_EXTENSIONS[extension] : null;
  if (!container || !modelsPlaying({ container: container, codecs: [] })) {
    return;
  }
  // The stream's codecs are unknown: name those of the year's the Engine lacks.
  var missing = engineLacks(container, codecsListedIn(container));
  if (!missing.length) {
    return;
  }
  var lacksContainer = missing[0] === container;
  report(
    url,
    "The Engine can't play " + url + ": " + engineHasNo(missing) + ". Tizen " + config.profile.version + " TVs play " +
      (lacksContainer ? container : inWords(missing) + " in " + container) + sourcePage(container) +
      (lacksContainer ? "." + CHECK_ELSEWHERE : ", so if this stream uses them, check it on a TV, or on a Version Profile whose Engine plays it."),
  );
}

/** For an AVPlay stream the Engine didn't play: reports an adaptive streaming format AVPlay plays on a TV. */
function reportAdaptiveStream(url: string) {
  var path = String(url).split(/[?#]/)[0];
  for (var i = 0; i < ADAPTIVE_FORMATS.length; i++) {
    var name = ADAPTIVE_FORMATS[i].name;
    if (ADAPTIVE_FORMATS[i].pattern.test(path)) {
      report(
        url,
        "AVPlay can't play " + url + " here: " + engineHasNo([name]) + ", so the Video Plane shows a test pattern. " +
          "On a TV AVPlay plays " + name + " (TV guides/multimedia/adaptive-streaming.html). Check it on a TV.",
      );
      return;
    }
  }
}

function installMediaChecks() {
  if (!config.profile.media) {
    return;
  }
  // canPlayType and isTypeSupported: an App deciding what to play.
  HTMLMediaElement.prototype.canPlayType = function (this: HTMLMediaElement, mime: string) {
    var answer = nativeCanPlayType.call(this, mime);
    if (answer === "") {
      reportRefusedType(mime, "canPlayType('" + mime + "') answered \"\"", "canPlayType");
    }
    return answer;
  };
  var MediaSourceClass: any = (window as any).MediaSource;
  if (MediaSourceClass) {
    var nativeIsTypeSupported = MediaSourceClass.isTypeSupported;
    MediaSourceClass.isTypeSupported = function (mime: string) {
      var answer = nativeIsTypeSupported.call(MediaSourceClass, mime);
      if (!answer) {
        reportRefusedType(mime, "MediaSource.isTypeSupported('" + mime + "') answered false", "isTypeSupported");
      }
      return answer;
    };
    var nativeAddSourceBuffer = MediaSourceClass.prototype.addSourceBuffer;
    MediaSourceClass.prototype.addSourceBuffer = function (mime: string) {
      try {
        return nativeAddSourceBuffer.call(this, mime);
      } catch (e) {
        reportRefusedType(mime, "MediaSource.addSourceBuffer('" + mime + "') failed", "addSourceBuffer");
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
