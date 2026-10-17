// Installs window.tizen, which a TV provides without any script tag.

(window as any).tizen = {
  WebAPIException: WebAPIException,
  WebAPIError: WebAPIError,
  application: application,
  systeminfo: systeminfo,
  tvinputdevice: tvinputdevice,
  tvdisplaycontrol: tvdisplaycontrol,
  filesystem: createFilesystem(),
};
warnOnDeprecated("tizen");
installMediaChecks();

// The virtual $WEBAPIS script calls this (see webapis.ts).
config.installWebapis = installWebapis;

// Last, so the Shims have taken what they use first.
removeMissingFeatures();
