// tizen.WebAPIException (thrown synchronously) and tizen.WebAPIError (passed
// to error callbacks), with the legacy numeric codes Tizen APIs expose.

// tizen.html#WebAPIException; TD guides/concepts/error-handling.md.
var ERROR_CONSTANTS: { [constant: string]: number } = {
  INDEX_SIZE_ERR: 1,
  DOMSTRING_SIZE_ERR: 2,
  HIERARCHY_REQUEST_ERR: 3,
  WRONG_DOCUMENT_ERR: 4,
  INVALID_CHARACTER_ERR: 5,
  NO_DATA_ALLOWED_ERR: 6,
  NO_MODIFICATION_ALLOWED_ERR: 7,
  NOT_FOUND_ERR: 8,
  NOT_SUPPORTED_ERR: 9,
  INUSE_ATTRIBUTE_ERR: 10,
  INVALID_STATE_ERR: 11,
  SYNTAX_ERR: 12,
  INVALID_MODIFICATION_ERR: 13,
  NAMESPACE_ERR: 14,
  INVALID_ACCESS_ERR: 15,
  VALIDATION_ERR: 16,
  TYPE_MISMATCH_ERR: 17,
  SECURITY_ERR: 18,
  NETWORK_ERR: 19,
  ABORT_ERR: 20,
  URL_MISMATCH_ERR: 21,
  QUOTA_EXCEEDED_ERR: 22,
  TIMEOUT_ERR: 23,
  INVALID_NODE_TYPE_ERR: 24,
  DATA_CLONE_ERR: 25,
};

// DOM-named errors use their legacy DOMException code; Tizen-only names
// (InvalidValuesError, IOError, ServiceNotAvailableError, UnknownError,
// VerificationError) use 0.
var ERROR_CODES: { [name: string]: number } = {
  NotFoundError: 8,
  NotSupportedError: 9,
  InvalidStateError: 11,
  TypeMismatchError: 17,
  SecurityError: 18,
  NetworkError: 19,
  AbortError: 20,
  QuotaExceededError: 22,
  TimeoutError: 23,
};

interface WebAPIException extends Error {
  code: number;
}

type WebAPIErrorType = { new (name: string, message: string): WebAPIException };

// Both types carry the same fields; only the thrown one is an Error.
function webApiErrorType(
  isException: boolean,
  constants: { [constant: string]: number },
  codes: { [name: string]: number },
): WebAPIErrorType {
  var type = function (this: WebAPIException, name: string, message: string) {
    this.name = name;
    this.message = message;
    this.code = codes.hasOwnProperty(name) ? codes[name] : 0;
  } as any as WebAPIErrorType;
  if (isException) {
    type.prototype = Object.create(Error.prototype);
    type.prototype.constructor = type;
    for (var constant in constants) {
      (type as any)[constant] = type.prototype[constant] = constants[constant];
    }
  }
  return type;
}

var WebAPIException = webApiErrorType(true, ERROR_CONSTANTS, ERROR_CODES);
var WebAPIError = webApiErrorType(false, ERROR_CONSTANTS, ERROR_CODES);

/** Calls an optional error callback asynchronously, as Tizen APIs do. */
function failLater(
  errorCallback: ((e: WebAPIException) => void) | undefined,
  name: string,
  message: string,
  errorType?: WebAPIErrorType,
) {
  if (typeof errorCallback === "function") {
    setTimeout(function () {
      errorCallback(new (errorType || WebAPIError)(name, message));
    }, 0);
  }
}

function requireFunction(value: any, argument: string) {
  if (typeof value !== "function") {
    throw new WebAPIException("TypeMismatchError", argument + " must be a function");
  }
}
