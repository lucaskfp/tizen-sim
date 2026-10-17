// tizen.tvdisplaycontrol: the TV's stereoscopic 3D support, answered from the
// Version Profile's device values with the Session's overrides applied.
//
// Source: tvdisplaycontrol.html in the 4.0 to 10.0 TV references. Every
// member is there since 2.3. The order of the checks within a method is
// unverified.

var TV_DISPLAY_PRIVILEGE = "http://tizen.org/privilege/tv.display";
var TV_DISPLAY_FEATURE = "http://tizen.org/feature/tv.display";

/**
 * Up to 5.5 every method needs `tv.display`; the 5.0 and 5.5 references mark
 * the privilege and its SecurityError deprecated, and from 6.0 they name no
 * privilege. There is no 3.0 reference; it gets the 4.0 one, where the
 * pre-5.0 shapes of the APIs are (docs/agents/tizen-docs.md).
 */
function requireTvDisplay(method: string) {
  if (parseFloat(config.profile.version) < 6.0) {
    requirePrivilege(TV_DISPLAY_PRIVILEGE, "tizen.tvdisplaycontrol." + method + "()");
  }
  // "NotSupportedError, if this feature is not supported" (Related Feature).
  if (config.profile.capabilities[TV_DISPLAY_FEATURE] !== true) {
    throw new WebAPIException("NotSupportedError", "These TVs don't support the TV Display Control API");
  }
}

var tvdisplaycontrol = {
  get3DEffectMode: function (): string {
    requireTvDisplay("get3DEffectMode");
    return config.profile.device.display3DEffectMode;
  },

  is3DModeEnabled: function (): string {
    requireTvDisplay("is3DModeEnabled");
    return config.profile.device.display3DModeState;
  },

  getSupported3DEffectModeList: function (
    successCallback: (modes: string[]) => void,
    errorCallback?: ((e: WebAPIException) => void) | null,
  ) {
    requireFunction(successCallback, "successCallback");
    requireOptionalFunction(errorCallback, "errorCallback");
    requireTvDisplay("getSupported3DEffectModeList");
    var modes: string[] = config.profile.device.supported3DEffectModes.slice();
    setTimeout(function () {
      successCallback(modes);
    }, 0);
  },
};
