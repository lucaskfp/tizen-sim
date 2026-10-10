// Installs window.tizen, which a TV provides without any script tag.

(window as any).tizen = {
  WebAPIException: WebAPIException,
  WebAPIError: WebAPIError,
  application: application,
  systeminfo: systeminfo,
  tvinputdevice: tvinputdevice,
};

// The virtual $WEBAPIS script calls this (see webapis.ts).
config.installWebapis = installWebapis;
