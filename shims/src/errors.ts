// tizen.WebAPIException (thrown synchronously) and tizen.WebAPIError (passed
// to error callbacks), with the legacy numeric codes Tizen APIs expose.

var ERROR_CODES: { [name: string]: number } = {
  NotFoundError: 8,
  NotSupportedError: 9,
  InvalidValuesError: 0,
  TypeMismatchError: 17,
  SecurityError: 18,
  UnknownError: 0,
};

interface WebAPIException extends Error {
  code: number;
}

type WebAPIErrorType = { new (name: string, message: string): WebAPIException };

// Both types carry the same fields; only the thrown one is an Error.
function webApiErrorType(isException: boolean): WebAPIErrorType {
  var type = function (this: WebAPIException, name: string, message: string) {
    this.name = name;
    this.message = message;
    this.code = ERROR_CODES[name] || 0;
  } as any as WebAPIErrorType;
  if (isException) {
    type.prototype = Object.create(Error.prototype);
    type.prototype.constructor = type;
  }
  return type;
}

var WebAPIException = webApiErrorType(true);
var WebAPIError = webApiErrorType(false);

/** Calls an optional error callback asynchronously, as Tizen APIs do. */
function failLater(errorCallback: ((e: WebAPIException) => void) | undefined, name: string, message: string) {
  if (typeof errorCallback === "function") {
    setTimeout(function () {
      errorCallback(new WebAPIError(name, message));
    }, 0);
  }
}

function requireFunction(value: any, argument: string) {
  if (typeof value !== "function") {
    throw new WebAPIException("TypeMismatchError", argument + " must be a function");
  }
}
