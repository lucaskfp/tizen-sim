// tizen.systeminfo, answered from the Version Profile.
//
// Source: systeminfo.html (9.0 TV reference); the properties a TV has are in
// TD guides/device/system-information.md.

// Every SystemInfoPropertyId (systeminfo.html#SystemInfoPropertyId). Any other
// string throws TypeMismatchError; one of these the Shims don't answer goes to
// the error callback as NotSupportedError.
var SYSTEM_INFO_PROPERTY_IDS = [
  "BATTERY", "CPU", "STORAGE", "DISPLAY", "DEVICE_ORIENTATION", "BUILD", "LOCALE", "NETWORK",
  "WIFI_NETWORK", "ETHERNET_NETWORK", "CELLULAR_NETWORK", "NET_PROXY_NETWORK", "SIM", "PERIPHERAL",
  "MEMORY", "VIDEOSOURCE", "CAMERA_FLASH", "ADS", "SERVICE_COUNTRY", "SOURCE_INFO", "PANEL",
];

function requirePropertyId(property: any) {
  for (var i = 0; i < SYSTEM_INFO_PROPERTY_IDS.length; i++) {
    if (SYSTEM_INFO_PROPERTY_IDS[i] === property) {
      return;
    }
  }
  throw new WebAPIException("TypeMismatchError", "Not a SystemInfoPropertyId: " + property);
}

function systemInfoProperty(property: string): any {
  var device = config.profile.device;
  switch (property) {
    case "BUILD":
      return {
        model: device.model,
        manufacturer: device.manufacturer,
        buildVersion: device.buildVersion,
      };
    case "CPU":
      return { load: device.cpuLoad };
    case "DISPLAY":
      return {
        resolutionWidth: device.resolutionWidth,
        resolutionHeight: device.resolutionHeight,
        dotsPerInchWidth: device.dotsPerInch,
        dotsPerInchHeight: device.dotsPerInch,
        physicalWidth: device.physicalWidth,
        physicalHeight: device.physicalHeight,
        brightness: 1,
      };
    case "LOCALE":
      // Both in LANGUAGE_REGION form (#SystemInfoLocale).
      return { language: device.language, country: device.country };
    case "MEMORY":
      return { status: "NORMAL" };
    case "NETWORK":
      return { networkType: device.networkType };
    case "PANEL":
      return { panelWidth: device.panelWidth, panelHeight: device.panelHeight };
    default:
      return null;
  }
}

/** Fetches a property for a success callback, or reports NotSupportedError. */
function getProperty(
  property: string,
  successCallback: (value: any) => void,
  errorCallback: ((e: WebAPIException) => void) | undefined,
  wrap: (value: any) => any,
) {
  requirePropertyId(property);
  requireFunction(successCallback, "successCallback");
  var value = systemInfoProperty(property);
  if (!value) {
    failLater(errorCallback, "NotSupportedError", "Unsupported system info property: " + property);
    return;
  }
  setTimeout(function () {
    successCallback(wrap(value));
  }, 0);
}

var nextPropertyListenerId = 1;
var propertyListeners: { [id: string]: boolean } = {};

/**
 * Registers a change listener. Nothing the Shims report ever changes, so it
 * never fires, which the doc allows for properties that don't change
 * (#SystemInfo::addPropertyValueChangeListener).
 */
function addPropertyListener(
  property: string,
  successCallback: (value: any) => void,
  errorCallback: ((e: WebAPIException) => void) | undefined,
): number {
  requirePropertyId(property);
  requireFunction(successCallback, "successCallback");
  // PANEL can't be watched (#SystemInfoPanel).
  if (property === "PANEL" || !systemInfoProperty(property)) {
    failLater(errorCallback, "NotSupportedError", "Can't watch system info property: " + property);
  }
  var id = nextPropertyListenerId++;
  propertyListeners[id] = true;
  return id;
}

var systeminfo: any = {
  getCapability: function (key: string) {
    var capabilities = config.profile.capabilities;
    if (!capabilities.hasOwnProperty(key)) {
      throw new WebAPIException("NotSupportedError", "Unsupported capability: " + key);
    }
    return capabilities[key];
  },

  getCount: function (property: string) {
    requirePropertyId(property);
    return systemInfoProperty(property) ? 1 : 0;
  },

  getPropertyValue: function (
    property: string,
    successCallback: (value: any) => void,
    errorCallback?: (e: WebAPIException) => void,
  ) {
    getProperty(property, successCallback, errorCallback, function (value) {
      return value;
    });
  },

  getPropertyValueArray: function (
    property: string,
    successCallback: (values: any[]) => void,
    errorCallback?: (e: WebAPIException) => void,
  ) {
    getProperty(property, successCallback, errorCallback, function (value) {
      return [value];
    });
  },

  addPropertyValueChangeListener: function (
    property: string,
    successCallback: (value: any) => void,
    options?: any,
    errorCallback?: (e: WebAPIException) => void,
  ) {
    return addPropertyListener(property, successCallback, errorCallback);
  },

  addPropertyValueArrayChangeListener: function (
    property: string,
    successCallback: (values: any[]) => void,
    options?: any,
    errorCallback?: (e: WebAPIException) => void,
  ) {
    return addPropertyListener(property, successCallback, errorCallback);
  },

  // Removing an id that isn't registered does nothing.
  removePropertyValueChangeListener: function (listenerId: number) {
    delete propertyListeners[listenerId];
  },

  getTotalMemory: function () {
    return config.profile.device.totalMemory;
  },

  getAvailableMemory: function () {
    return config.profile.device.availableMemory;
  },
};

// Since Tizen 8.0 (#SystemInfo::getDeviceUptime). The TV booted when the
// Session started.
if (parseFloat(config.profile.version) >= 8) {
  systeminfo.getDeviceUptime = function () {
    return Math.floor((new Date().getTime() - config.app.installedAt) / 1000);
  };
}
