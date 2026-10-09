// Installs window.tizen, which a TV provides without any script tag.

(window as any).tizen = {
  WebAPIException: WebAPIException,
  WebAPIError: WebAPIError,
  application: application,
  systeminfo: systeminfo,
};
