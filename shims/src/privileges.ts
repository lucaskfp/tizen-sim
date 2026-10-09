// Privilege enforcement: an API whose privilege the App's config.xml doesn't
// declare throws SecurityError on a TV. Lenient Mode allows the call, but the
// Diagnostic is raised either way.

function requirePrivilege(privilege: string, api: string) {
  var declared = config.app.privileges;
  for (var i = 0; i < declared.length; i++) {
    if (declared[i] === privilege) {
      return;
    }
  }
  var message =
    api + " requires the privilege " + privilege +
    ", which config.xml does not declare. On a TV this call throws SecurityError.";
  raiseDiagnostic("privilege-not-declared", message);
  if (!config.lenient) {
    throw new WebAPIException("SecurityError", message);
  }
}
