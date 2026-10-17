// The Samsung Product API (`webapis.*`): productinfo, tvinfo and network, plus avinfo and adinfo, answered
// from the Version Profile's device values with the Session's overrides applied; widgetdata, kept in the
// App's Sandbox; and
// avplay (see avplay.ts).
//
// A TV gives a page `webapis` only once it loads `$WEBAPIS/webapis/webapis.js`
// (samsung-product-api-references.html), so the bundle defines it here and the
// dev server answers that virtual script with a call to `installWebapis`.
//
// Sources: developer.samsung.com/smarttv/develop/api-references/
// samsung-product-api-references/
// {webapi,productinfo,tvinfo,network,avinfo,adinfo,widgetdata}-api.html.
// "Since" versions there are Tizen versions; members newer than the Version
// Profile are absent (see availability.ts).
//
// Not shimmed: methods for B2B products only (tvinfo.setMenuValue, network's
// setters and proxy methods), the TvInfoMenuValue constants, and network's
// DHCP option 60 (partner), SoftAP and EAP methods.

// webapi-api.html#WebAPIException adds four codes to tizen's. That
// InvalidValuesError, IOError, ServiceNotAvailableError and UnknownError carry
// them is unverified: the reference lists the constants, not the mapping.
// So is DOMStringSizeError (widgetdata.write) carrying DOMSTRING_SIZE_ERR.
var WEBAPIS_ERROR_CONSTANTS = extend(ERROR_CONSTANTS, {
  INVALID_VALUES_ERR: 26,
  IO_ERR: 27,
  SERVICE_NOT_AVAILABLE_ERR: 28,
  UNKNOWN_ERR: 9999,
});
var WEBAPIS_ERROR_CODES = extend(ERROR_CODES, {
  DOMStringSizeError: 2,
  InvalidValuesError: 26,
  IOError: 27,
  ServiceNotAvailableError: 28,
  UnknownError: 9999,
});
var WebapisException = webApiErrorType(true, WEBAPIS_ERROR_CONSTANTS, WEBAPIS_ERROR_CODES);
var WebapisError = webApiErrorType(false, WEBAPIS_ERROR_CONSTANTS, WEBAPIS_ERROR_CODES);

var PRODUCTINFO_PRIVILEGE = "http://developer.samsung.com/privilege/productinfo";
var NETWORK_PRIVILEGE = "http://developer.samsung.com/privilege/network.public";
var ADINFO_PRIVILEGE = "http://developer.samsung.com/privilege/adinfo";
var WIDGETDATA_PRIVILEGE = "http://developer.samsung.com/privilege/widgetdata";

function extend<T>(base: { [key: string]: T }, extra: { [key: string]: T }): { [key: string]: T } {
  var out: { [key: string]: T } = {};
  for (var key in base) {
    out[key] = base[key];
  }
  for (key in extra) {
    out[key] = extra[key];
  }
  return out;
}

function webapisFail(name: string, message: string): never {
  throw new WebapisException(name, message);
}

function requireWebapisPrivilege(privilege: string, api: string) {
  requirePrivilege(privilege, api + "()", WebapisException);
}

/** Calls a listener given as a function or as an object with `onchange`. */
function callListener(listener: any, value: any) {
  if (typeof listener === "function") {
    listener(value);
  } else {
    listener.onchange(value);
  }
}

function requireListener(listener: any) {
  if (typeof listener !== "function" && !(listener && typeof listener.onchange === "function")) {
    webapisFail("TypeMismatchError", "listener must be a function or have an onchange method");
  }
}

function requireType(value: any, type: string, argument: string) {
  if (typeof value !== type) {
    webapisFail("TypeMismatchError", argument + " must be a " + type);
  }
}

/** For a value of one of the references' string enums. */
function requireOneOf(value: any, values: string[], argument: string) {
  requireType(value, "string", argument);
  if (values.indexOf(value) < 0) {
    webapisFail("InvalidValuesError", argument + " must be one of " + values.join(", "));
  }
}

/** Listener ids shared by every module, as numbers the App can't confuse. */
var nextWebapisListenerId = 1;

function addListener(listeners: { [id: string]: any }, entry: any): number {
  var id = nextWebapisListenerId++;
  listeners[id] = entry;
  return id;
}

/**
 * Removes a listener, or throws InvalidValuesError for an unknown id. That a
 * non-number id is a TypeMismatchError is unverified.
 */
function removeListener(listeners: { [id: string]: any }, listenerId: any) {
  requireType(listenerId, "number", "listenerId");
  if (!listeners.hasOwnProperty(listenerId)) {
    webapisFail("InvalidValuesError", "No listener with id " + listenerId);
  }
  delete listeners[listenerId];
}

/**
 * The plugin version every module's getVersion() reports. The references
 * give no format, so the Tizen version standing in for it is unverified.
 */
function pluginVersion(): string {
  return config.profile.version;
}

// productinfo-api.html.
function createProductInfo(device: { [key: string]: any }): any {
  var CONFIG_KEYS = { CONFIG_KEY_DATA_SERVICE: 0, CONFIG_KEY_SERVICE_COUNTRY: 1 };
  var systemConfig: { [key: number]: string } = {};
  systemConfig[CONFIG_KEYS.CONFIG_KEY_DATA_SERVICE] = device.dataService;
  systemConfig[CONFIG_KEYS.CONFIG_KEY_SERVICE_COUNTRY] = device.serviceCountry;
  var configListeners: { [id: string]: { key: number; listener: any } } = {};

  // A non-number is a TypeMismatchError; that a number outside the enum is an
  // InvalidValuesError (listed "since 3.0" without a reason) is unverified.
  function requireConfigKey(key: any) {
    requireType(key, "number", "key");
    if (!systemConfig.hasOwnProperty(key)) {
      webapisFail("InvalidValuesError", "Not a ProductInfoConfigKey: " + key);
    }
  }

  function value(method: string, name: string, privileged?: boolean) {
    return function () {
      if (privileged !== false) {
        requireWebapisPrivilege(PRODUCTINFO_PRIVILEGE, "webapis.productinfo." + method);
      }
      return device[name];
    };
  }

  var productinfo: any = {
    ProductInfoConfigKey: CONFIG_KEYS,
    ProductInfoNoGlass3dSupport: { NO_GLASS_3D_NOT_SUPPORTED: 0, NO_GLASS_3D_SUPPORTED: 1 },
    // The misspelt OPERATIING is the documented name.
    ProductInfoSiServerType: {
      SI_TYPE_OPERATIING_SERVER: 0,
      SI_TYPE_DEVELOPMENT_SERVER: 1,
      SI_TYPE_DEVELOPING_SERVER: 2,
    },

    getVersion: function () {
      requireWebapisPrivilege(PRODUCTINFO_PRIVILEGE, "webapis.productinfo.getVersion");
      return pluginVersion();
    },
    getFirmware: value("getFirmware", "firmware"),
    getDuid: value("getDuid", "duid"),
    getModelCode: value("getModelCode", "modelCode"),
    getModel: value("getModel", "productModel"),
    getRealModel: value("getRealModel", "realModel"),
    getLocalSet: value("getLocalSet", "localSet"),
    getSmartTVServerType: value("getSmartTVServerType", "smartTVServerType"),
    getSmartTVServerVersion: value("getSmartTVServerVersion", "smartTVServerVersion"),
    isUdPanelSupported: value("isUdPanelSupported", "udPanel"),

    getSystemConfig: function (key: any) {
      requireWebapisPrivilege(PRODUCTINFO_PRIVILEGE, "webapis.productinfo.getSystemConfig");
      requireConfigKey(key);
      return systemConfig[key];
    },

    // The reference lists no SecurityError among the exceptions but describes
    // one for onerror, so a missing privilege goes there.
    setSystemConfig: function (key: any, newValue: any, onsuccess?: () => void, onerror?: (e: any) => void) {
      requireConfigKey(key);
      requireType(newValue, "string", "value");
      // The reference marks this error deprecated since 3.0 without saying
      // what replaced it, so refusing the read-only key is unverified.
      if (key === CONFIG_KEYS.CONFIG_KEY_SERVICE_COUNTRY) {
        webapisFail("NotSupportedError", "CONFIG_KEY_SERVICE_COUNTRY is read-only");
      }
      try {
        requireWebapisPrivilege(PRODUCTINFO_PRIVILEGE, "webapis.productinfo.setSystemConfig");
      } catch (e: any) {
        failLater(onerror, e.name, e.message, WebapisError);
        return;
      }
      systemConfig[key] = newValue;
      setTimeout(function () {
        if (typeof onsuccess === "function") {
          onsuccess();
        }
        for (var id in configListeners) {
          if (configListeners[id].key === key) {
            callListener(configListeners[id].listener, key);
          }
        }
      }, 0);
    },

    addSystemConfigChangeListener: function (key: any, listener: any) {
      requireWebapisPrivilege(PRODUCTINFO_PRIVILEGE, "webapis.productinfo.addSystemConfigChangeListener");
      requireConfigKey(key);
      requireListener(listener);
      return addListener(configListeners, { key: key, listener: listener });
    },

    removeSystemConfigChangeListener: function (listenerId: any) {
      requireWebapisPrivilege(PRODUCTINFO_PRIVILEGE, "webapis.productinfo.removeSystemConfigChangeListener");
      removeListener(configListeners, listenerId);
    },

    isUHDAModel: value("isUHDAModel", "uhdaModel"),
  };
  if (available("webapis.productinfo.is8KPanelSupported")) {
    productinfo.is8KPanelSupported = value("is8KPanelSupported", "panel8K");
  }
  if (available("webapis.productinfo.isWallModel")) {
    productinfo.isWallModel = value("isWallModel", "wallModel");
  }
  if (available("webapis.productinfo.isDisplayRotatorSupported")) {
    // The one productinfo method that needs no privilege.
    productinfo.isDisplayRotatorSupported = value("isDisplayRotatorSupported", "displayRotator", false);
  }
  if (available("webapis.productinfo.getLicensedVendor")) {
    productinfo.getLicensedVendor = value("getLicensedVendor", "licensedVendor");
  }
  if (available("webapis.productinfo.getLicensedBrand")) {
    // NotSupportedError on Samsung products, which have no licensed brand.
    productinfo.getLicensedBrand = function () {
      requireWebapisPrivilege(PRODUCTINFO_PRIVILEGE, "webapis.productinfo.getLicensedBrand");
      if (!device.licensedBrand) {
        webapisFail("NotSupportedError", "Not a licensed product");
      }
      return device.licensedBrand;
    };
  }
  if (available("webapis.productinfo.isOledPanelSupported")) {
    productinfo.isOledPanelSupported = value("isOledPanelSupported", "oledPanel");
  }
  return productinfo;
}

// tvinfo-api.html. Only setMenuValue, B2B only, needs a privilege.
function createTvInfo(device: { [key: string]: any }): any {
  var MENU_KEYS: { [name: string]: number | string } = {
    CAPTION_ONOFF_KEY: 0,
    CAPTION_MODE_KEY: 1,
    CAPTION_FONT_SIZE_KEY: 2,
    CAPTION_FONT_STYLE_KEY: 3,
    CAPTION_FG_COLOR_KEY: 4,
    CAPTION_FG_OPACITY_KEY: 5,
    CAPTION_BG_COLOR_KEY: 6,
    CAPTION_BG_OPACITY_KEY: 7,
    CAPTION_EDGE_TYPE_KEY: 8,
    CAPTION_EDGE_COLOR_KEY: 9,
    CAPTION_WINDOW_COLOR_KEY: 10,
    CAPTION_WINDOW_OPACITY_KEY: 11,
    ACCESSIBILITY_FOCUS_ZOOM: 12,
    ACCESSIBILITY_HIGH_CONTRAST: 13,
    SMARTHUB_CHANNEL_BOUND_APPS_TICKER: 14,
    VOICE_GUIDE_KEY: 15,
    SUBTITLE_ONOFF_KEY: 16,
    SUBTITLE_MODE_KEY: 17,
    PRIMARY_AUDIO_LANGUAGE_KEY: 18,
    SECONDARY_AUDIO_LANGUAGE_KEY: 19,
    ACCESSIBILITY_VOICE_GUIDE_SPEED: 20,
    CAPTION_STYLE_KEY: 21,
    // The reference lists these without numbers; their values here are their
    // names, which is unverified.
    UI_SIZE: "UI_SIZE",
    UI_TRANSPARENCY: "UI_TRANSPARENCY",
    UI_TIMEOUT: "UI_TIMEOUT",
    MOVING_TEXT_AND_IMAGE: "MOVING_TEXT_AND_IMAGE",
  };
  if (available("TvInfoMenuKey.CAPTION_LANGUAGE_INDEX_KEY")) {
    MENU_KEYS.CAPTION_LANGUAGE_INDEX_KEY = 22;
  }
  if (available("TvInfoMenuKey.CAPTION_LANGUAGE_KEY")) {
    MENU_KEYS.CAPTION_LANGUAGE_KEY = "CAPTION_LANGUAGE_KEY";
  }
  var INFO_KEYS: { [name: string]: string } = {};
  if (available("TvInfoKey.SMART_RC_BATTERY")) {
    INFO_KEYS.SMART_RC_BATTERY = "SMART_RC_BATTERY";
  }
  if (available("TvInfoKey.BOARD_TEMPERATURE")) {
    INFO_KEYS.BOARD_TEMPERATURE = "BOARD_TEMPERATURE";
  }
  var INFO_VALUES: { [key: string]: string } = {
    SMART_RC_BATTERY: "smartRcBattery",
    BOARD_TEMPERATURE: "boardTemperature",
  };
  var captionListeners: { [id: string]: any } = {};

  /**
   * The TvInfoMenuKey name for a key value. The reference lists
   * TypeMismatchError without saying when; that an unknown key is one is
   * unverified.
   */
  function menuKeyName(key: any): string {
    for (var name in MENU_KEYS) {
      if (MENU_KEYS[name] === key) {
        return name;
      }
    }
    return webapisFail("TypeMismatchError", "Not a TvInfoMenuKey: " + key);
  }

  return {
    TvInfoMenuKey: MENU_KEYS,
    TvInfoKey: INFO_KEYS,
    TvInfoValue: { TV_VIEWER_BG_NOT_EXECUTABLE: 0, TV_VIEWER_BG_EXECUTABLE: 1 },

    getVersion: pluginVersion,

    // Settings the profile doesn't list read as 0, the DEFAULT or OFF value of
    // the numeric keys. What a TV reports for them is unverified.
    getMenuValue: function (key: any) {
      var name = menuKeyName(key);
      var menu = device.tvMenu || {};
      return menu.hasOwnProperty(name) ? menu[name] : 0;
    },

    // Nothing changes the TV's caption settings, so listeners never fire.
    addCaptionChangeListener: function (key: any, listener: any) {
      menuKeyName(key);
      requireListener(listener);
      return addListener(captionListeners, listener);
    },

    removeCaptionChangeListener: function (listenerId: any) {
      removeListener(captionListeners, listenerId);
    },

    isTvsPicSizeResized: function () {
      return device.tvsPicSizeResized;
    },

    // That an unknown string key is an InvalidValuesError, rather than
    // NotSupportedError, is unverified.
    getTvInfoValue: function (key: any) {
      requireType(key, "string", "key");
      if (!INFO_KEYS.hasOwnProperty(key)) {
        webapisFail("InvalidValuesError", "Not a TvInfoKey: " + key);
      }
      return device[INFO_VALUES[key]];
    },

    // Captions are drawn by the TV, which the simulator doesn't render.
    showCaption: function (show: any) {
      requireType(show, "boolean", "show");
    },

    registerInAppCaptionControl: function (status: any) {
      requireType(status, "boolean", "status");
    },
  };
}

// network-api.html. Every method needs network.public.
function createNetwork(device: { [key: string]: any }): any {
  var CONNECTION_TYPES = { DISCONNECTED: 0, WIFI: 1, CELLULAR: 2, ETHERNET: 3 };
  var IP_MODES: { [name: string]: number } = { NONE: 0, STATIC: 1, DYNAMIC: 2, AUTO: 3, FIXED: 4, UNKNOWN: 5 };
  var SECURITY_MODES: { [name: string]: number } = { WEP: 1, WPA_PSK: 2, WPA2_PSK: 3, EAP: 4, NONE: 5, UNKNOWN: 6 };
  var ENCRYPTION_TYPES: { [name: string]: number } = { WEP: 1, TKIP: 2, AES: 3, TKIP_AES_MIXED: 4, NONE: 5, UNKNOWN: 6 };
  var stateListeners: { [id: string]: any } = {};

  /**
   * The NetworkActiveConnectionType of tizen.systeminfo's networkType. That
   * the cellular generations map to CELLULAR, and that any connection reaches
   * the gateway, is unverified.
   */
  function connectionType(): number {
    switch (device.networkType) {
      case "WIFI":
        return CONNECTION_TYPES.WIFI;
      case "ETHERNET":
        return CONNECTION_TYPES.ETHERNET;
      case "2G":
      case "2.5G":
      case "3G":
      case "4G":
        return CONNECTION_TYPES.CELLULAR;
      default:
        return CONNECTION_TYPES.DISCONNECTED;
    }
  }

  function enumValue(values: { [name: string]: number }, name: string): number {
    return values.hasOwnProperty(name) ? values[name] : values.UNKNOWN;
  }

  /** Checks the privilege, then that the TV has the connection `needs`. */
  function access(method: string, needs?: "connection" | "wifi") {
    requireWebapisPrivilege(NETWORK_PRIVILEGE, "webapis.network." + method);
    requireConnection(needs);
  }

  function requireConnection(needs?: "connection" | "wifi") {
    var type = connectionType();
    if (needs === "connection" && type === CONNECTION_TYPES.DISCONNECTED) {
      webapisFail("InvalidStateError", "The TV has no network connection");
    }
    if (needs === "wifi" && type !== CONNECTION_TYPES.WIFI) {
      webapisFail("InvalidStateError", "The TV is not connected over Wi-Fi");
    }
  }

  function getter(method: string, needs: "connection" | "wifi" | undefined, read: () => any) {
    return function () {
      access(method, needs);
      return read();
    };
  }

  function setting(name: string) {
    return function () {
      return device[name];
    };
  }

  var network: any = {
    NetworkActiveConnectionType: CONNECTION_TYPES,
    NetworkIpMode: IP_MODES,
    NetworkState: {
      INTERNET_CONNECTED: 0,
      LAN_CABLE_ATTACHED: 1,
      LAN_CABLE_DETACHED: 2,
      LAN_CABLE_STATE_UNKNOWN: 3,
      GATEWAY_CONNECTED: 4,
      GATEWAY_DISCONNECTED: 5,
      WIFI_MODULE_STATE_ATTACHED: 6,
      WIFI_MODULE_STATE_DETACHED: 7,
      WIFI_MODULE_STATE_UNKNOWN: 8,
      INTERNET_DISCONNECTED: 9,
    },
    NetworkWiFiSecurityMode: SECURITY_MODES,
    NetworkWiFiEncryptionType: ENCRYPTION_TYPES,

    getVersion: getter("getVersion", undefined, pluginVersion),
    isConnectedToGateway: getter("isConnectedToGateway", undefined, function () {
      return connectionType() !== CONNECTION_TYPES.DISCONNECTED;
    }),
    getActiveConnectionType: getter("getActiveConnectionType", undefined, connectionType),
    getIpMode: getter("getIpMode", undefined, function () {
      return enumValue(IP_MODES, device.ipMode);
    }),
    getIp: getter("getIp", "connection", setting("ip")),
    getSubnetMask: getter("getSubnetMask", "connection", setting("subnetMask")),
    getGateway: getter("getGateway", "connection", setting("gateway")),
    getDns: getter("getDns", "connection", setting("dns")),
    getSecondaryDns: getter("getSecondaryDns", "connection", setting("secondaryDns")),
    getMac: getter("getMac", "connection", setting("mac")),
    getWiFiSsid: getter("getWiFiSsid", "wifi", setting("wifiSsid")),
    getWiFiSignalStrengthLevel: getter("getWiFiSignalStrengthLevel", "wifi", setting("wifiSignalStrength")),
    getWiFiSecurityMode: getter("getWiFiSecurityMode", "wifi", function () {
      return enumValue(SECURITY_MODES, device.wifiSecurityMode);
    }),
    getWiFiEncryptionType: getter("getWiFiEncryptionType", "wifi", function () {
      return enumValue(ENCRYPTION_TYPES, device.wifiEncryptionType);
    }),

    // The connection never changes during a Session, so listeners never fire.
    addNetworkStateChangeListener: function (listener: any) {
      access("addNetworkStateChangeListener");
      requireListener(listener);
      return addListener(stateListeners, listener);
    },

    removeNetworkStateChangeListener: function (listenerId: any) {
      access("removeNetworkStateChangeListener");
      removeListener(stateListeners, listenerId);
    },
  };
  if (available("webapis.network.getTVName")) {
    network.getTVName = getter("getTVName", "connection", setting("tvName"));
  }
  if (available("webapis.network.getWiFiFrequency")) {
    network.getWiFiFrequency = getter("getWiFiFrequency", "wifi", setting("wifiFrequency"));
  }
  return network;
}

// avinfo-api.html. No method names a privilege. The reference leaves the
// order of its checks open; checking the argument before refusing an
// unsupported call is unverified.
function createAvInfo(device: { [key: string]: any }): any {
  var SUB_AMP_MODES = ["NO_EXT_AUDIO", "EXT_AUDIO_TV", "EXT_AUDIO_SWITCH", "EXT_HEALTHCARE"];
  var ENERGY_SAVING_TYPES = ["OFF", "LOW", "MEDIUM", "HIGH"];
  var ECO_SENSOR_TYPES = ["OFF", "ON"];
  // The App can change these; the change lasts as long as the page.
  var energySaving: string = device.energySaving;
  var ecoSensor: string = device.ecoSensor;

  /**
   * NotSupportedError "for non Samsung products". A licensed brand marks
   * one, as for productinfo.getLicensedBrand; that a TV links the two is
   * unverified.
   */
  function requireSamsungProduct(method: string) {
    if (device.licensedBrand) {
      webapisFail("NotSupportedError", "webapis.avinfo." + method + "() is available on Samsung products only");
    }
  }

  var avinfo: any = {
    AvInfoDigitalCompMode: { DOLBY_DIGITAL_COMP_MODE_LINE: 0, DOLBY_DIGITAL_COMP_MODE_RF: 1 },

    getVersion: function () {
      requireSamsungProduct("getVersion");
      return pluginVersion();
    },

    // "This feature doesn't be supported since 2016", before every Version
    // Profile.
    getDolbyDigitalCompMode: function () {
      webapisFail("NotSupportedError", "Dolby Digital compression mode is not supported since 2016");
    },

    isHdrTvSupport: function () {
      requireSamsungProduct("isHdrTvSupport");
      return device.hdrTvSupport;
    },
  };
  if (available("webapis.avinfo.setSubAmpMode")) {
    // For Hotel TVs only: NotSupportedError on any other product.
    avinfo.setSubAmpMode = function (mode: any) {
      requireOneOf(mode, SUB_AMP_MODES, "mode");
      webapisFail("NotSupportedError", "setSubAmpMode() is available on Hotel TVs only");
    };
  }
  if (available("webapis.avinfo.getEnergySaving")) {
    avinfo.getEnergySaving = function () {
      requireSamsungProduct("getEnergySaving");
      return energySaving;
    };
  }
  if (available("webapis.avinfo.setEnergySaving")) {
    avinfo.setEnergySaving = function (value: any) {
      requireOneOf(value, ENERGY_SAVING_TYPES, "value");
      requireSamsungProduct("setEnergySaving");
      energySaving = value;
    };
  }
  if (available("webapis.avinfo.getEcoSensor")) {
    avinfo.getEcoSensor = function () {
      requireSamsungProduct("getEcoSensor");
      return ecoSensor;
    };
  }
  if (available("webapis.avinfo.setEcoSensor")) {
    avinfo.setEcoSensor = function (value: any) {
      requireOneOf(value, ECO_SENSOR_TYPES, "value");
      requireSamsungProduct("setEcoSensor");
      ecoSensor = value;
    };
  }
  return avinfo;
}

// adinfo-api.html. Every method needs adinfo.
function createAdInfo(device: { [key: string]: any }): any {
  function access(method: string) {
    requireWebapisPrivilege(ADINFO_PRIVILEGE, "webapis.adinfo." + method);
  }

  return {
    getVersion: function () {
      access("getVersion");
      return pluginVersion();
    },

    // Thrown "if the PSID value is null"; the profile's empty TIFA stands for that.
    getTIFA: function () {
      access("getTIFA");
      if (!device.tifa) {
        webapisFail("InvalidStateError", "The TV has no PSID");
      }
      return device.tifa;
    },

    isLATEnabled: function () {
      access("isLATEnabled");
      return device.limitAdTracking;
    },
  };
}

// widgetdata-api.html: the App's secure storage, which the Host keeps in its
// Sandbox. read, write and remove exist since 4.0; they throw only
// TypeMismatchError and pass every other error, SecurityError included, to
// onerror. That a non-string `data` is a TypeMismatchError, rather than
// converted to a string, is unverified.
function createWidgetData(): any {
  // write's limit: "Data, up to 20000 characters". Counting UTF-16 code
  // units, as String.length does, is unverified.
  var MAX_DATA_LENGTH = 20000;

  function requireOptionalCallback(callback: any, argument: string) {
    if (callback !== undefined && callback !== null) {
      requireType(callback, "function", argument);
    }
  }

  /**
   * Calls the Host after the method returns, as filesystem's runLater does,
   * and hands its result to `deliver`. A failure goes to `onerror` under its
   * own name if the method's reference lists it in `errors`, and as
   * UnknownError ("if any other error occurs") otherwise.
   */
  function callHostLater(method: string, params: any, errors: string[], onerror: any, deliver: (result: any) => void) {
    setTimeout(function () {
      var result: any;
      try {
        requireWebapisPrivilege(WIDGETDATA_PRIVILEGE, "webapis.widgetdata." + method);
        result = bridge.call("widgetdata." + method, params);
      } catch (e: any) {
        if (typeof onerror === "function") {
          var listed = e.name === "SecurityError" || errors.indexOf(e.name) >= 0;
          onerror(new WebapisError(listed ? e.name : "UnknownError", e.message));
        }
        return;
      }
      deliver(result);
    }, 0);
  }

  function succeed(onsuccess: any) {
    return function () {
      if (typeof onsuccess === "function") {
        onsuccess();
      }
    };
  }

  var widgetdata: any = {
    getVersion: function () {
      requireWebapisPrivilege(WIDGETDATA_PRIVILEGE, "webapis.widgetdata.getVersion");
      return pluginVersion();
    },
  };
  if (available("webapis.widgetdata.read")) {
    widgetdata.read = function (onsuccess: any, onerror?: any) {
      requireType(onsuccess, "function", "onsuccess");
      requireOptionalCallback(onerror, "onerror");
      callHostLater("read", {}, ["NotFoundError"], onerror, function (data: string) {
        onsuccess(data);
      });
    };
  }
  if (available("webapis.widgetdata.write")) {
    widgetdata.write = function (data: any, onsuccess?: any, onerror?: any) {
      requireType(data, "string", "data");
      requireOptionalCallback(onsuccess, "onsuccess");
      requireOptionalCallback(onerror, "onerror");
      if (data.length > MAX_DATA_LENGTH) {
        failLater(onerror, "DOMStringSizeError", "data is longer than " + MAX_DATA_LENGTH + " characters", WebapisError);
        return;
      }
      callHostLater("write", { data: data }, [], onerror, succeed(onsuccess));
    };
  }
  if (available("webapis.widgetdata.remove")) {
    widgetdata.remove = function (onsuccess?: any, onerror?: any) {
      requireOptionalCallback(onsuccess, "onsuccess");
      requireOptionalCallback(onerror, "onerror");
      callHostLater("remove", {}, ["NotFoundError"], onerror, succeed(onsuccess));
    };
  }
  return widgetdata;
}

/** Called by the virtual `$WEBAPIS/webapis/webapis.js`; loading it twice keeps the first. */
function installWebapis() {
  if ((window as any).webapis) {
    return;
  }
  var device = config.profile.device;
  (window as any).webapis = {
    productinfo: createProductInfo(device),
    tvinfo: createTvInfo(device),
    network: createNetwork(device),
    avinfo: createAvInfo(device),
    avplay: createAVPlay(),
    adinfo: createAdInfo(device),
    widgetdata: createWidgetData(),
  };
  warnOnDeprecated("webapis");
}
