// tizen.systeminfo, answered from the Version Profile.

function systemInfoProperty(property: string): any {
  var device = config.profile.device;
  switch (property) {
    case "BUILD":
      return {
        model: device.model,
        manufacturer: device.manufacturer,
        buildVersion: device.buildVersion,
      };
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
      return { language: device.language, country: device.country };
    default:
      return null;
  }
}

var systeminfo = {
  getCapability: function (key: string) {
    var capabilities = config.profile.capabilities;
    if (!capabilities.hasOwnProperty(key)) {
      throw new WebAPIException("NotSupportedError", "Unsupported capability: " + key);
    }
    return capabilities[key];
  },

  getCount: function (property: string) {
    return systemInfoProperty(property) ? 1 : 0;
  },

  getPropertyValue: function (
    property: string,
    successCallback: (value: any) => void,
    errorCallback?: (e: WebAPIException) => void,
  ) {
    requireFunction(successCallback, "successCallback");
    var value = systemInfoProperty(property);
    if (!value) {
      failLater(errorCallback, "NotSupportedError", "Unsupported system info property: " + property);
      return;
    }
    setTimeout(function () {
      successCallback(value);
    }, 0);
  },

  getTotalMemory: function () {
    return config.profile.device.totalMemory;
  },

  getAvailableMemory: function () {
    return config.profile.device.availableMemory;
  },
};
