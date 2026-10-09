// tizen.application for the running App. Source: application.html (9.0 TV
// reference).

var APP_INFO_PRIVILEGE = "http://tizen.org/privilege/application.info";

// The running App's only context.
var CONTEXT_ID = "1";

// Who launched the App, as getRequestedAppControl() reports it. The docs
// don't say what a TV reports for a launch from Smart Hub. Unverified.
var LAUNCHER_APP_ID = "tizen-sim.launcher";

var currentAppInfo = {
  id: config.app.id,
  name: config.app.name,
  iconPath: "",
  version: config.app.version,
  show: true,
  categories: [] as string[],
  installDate: new Date(config.app.installedAt),
  size: 0,
  packageId: config.app.packageId,
};

// Reading `size` needs application.info; nothing else on
// ApplicationInformation does (#ApplicationInformation::size).
Object.defineProperty(currentAppInfo, "size", {
  enumerable: true,
  get: function () {
    requirePrivilege(APP_INFO_PRIVILEGE, "tizen.application ApplicationInformation.size");
    return 0;
  },
});

// A plain launch: the default operation and no data
// (#Application::getRequestedAppControl never returns null).
var requestedAppControl = {
  appControl: {
    operation: "http://tizen.org/appcontrol/operation/default",
    uri: null,
    mime: null,
    category: null,
    data: [] as any[],
    launchMode: "SINGLE",
  },
  callerAppId: LAUNCHER_APP_ID,
  // The launcher expects no reply (#RequestedApplicationControl::replyResult).
  replyResult: function () {
    throw new WebAPIException("NotFoundError", "The caller app does not wait for a reply");
  },
  replyFailure: function () {
    throw new WebAPIException("NotFoundError", "The caller app does not wait for a reply");
  },
};

var currentApplication = {
  appInfo: currentAppInfo,
  contextId: CONTEXT_ID,
  exit: function () {
    bridge.notify("application.exit");
  },
  // The Host backgrounds the App: `visibilitychange` fires and, without
  // background support, execution pauses (#Application::hide).
  hide: function () {
    bridge.notify("application.hide");
  },
  getRequestedAppControl: function () {
    return requestedAppControl;
  },
};

var application = {
  getCurrentApplication: function () {
    return currentApplication;
  },

  // Without an id, the calling App's context (#ApplicationManager::getAppContext).
  getAppContext: function (contextId?: string | null) {
    if (contextId === undefined || contextId === null || String(contextId) === CONTEXT_ID) {
      return { id: CONTEXT_ID, appId: currentAppInfo.id };
    }
    throw new WebAPIException("NotFoundError", "No application context with id " + contextId);
  },

  getAppInfo: function (id?: string) {
    if (id === undefined || id === null || id === currentAppInfo.id) {
      return currentAppInfo;
    }
    throw new WebAPIException("NotFoundError", "No application with id " + id);
  },

  // No privilege needed (#ApplicationManager::getAppsInfo).
  getAppsInfo: function (
    successCallback: (apps: any[]) => void,
    errorCallback?: (e: WebAPIException) => void,
  ) {
    requireFunction(successCallback, "successCallback");
    setTimeout(function () {
      successCallback([currentAppInfo]);
    }, 0);
  },
};
