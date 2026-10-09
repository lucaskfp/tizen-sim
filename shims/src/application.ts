// tizen.application for the running App.

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

var currentApplication = {
  appInfo: currentAppInfo,
  contextId: "1",
  exit: function () {
    bridge.notify("application.exit");
  },
  hide: function () {},
  getRequestedAppControl: function () {
    return null;
  },
};

var application = {
  getCurrentApplication: function () {
    return currentApplication;
  },

  getAppInfo: function (id?: string) {
    if (id === undefined || id === null || id === currentAppInfo.id) {
      return currentAppInfo;
    }
    throw new WebAPIException("NotFoundError", "No application with id " + id);
  },

  getAppsInfo: function (
    successCallback: (apps: any[]) => void,
    errorCallback?: (e: WebAPIException) => void,
  ) {
    requirePrivilege("http://tizen.org/privilege/application.info", "tizen.application.getAppsInfo");
    requireFunction(successCallback, "successCallback");
    setTimeout(function () {
      successCallback([currentAppInfo]);
    }, 0);
  },
};
