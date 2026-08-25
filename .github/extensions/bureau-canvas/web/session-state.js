const PREFIX = "bureau.";

export function sessionValue(key, fallback = null) {
  return sessionStorage.getItem(`${PREFIX}${key}`) ?? fallback;
}

export function storeSessionValue(key, value) {
  if (value === null || value === undefined || value === "") {
    sessionStorage.removeItem(`${PREFIX}${key}`);
  } else {
    sessionStorage.setItem(`${PREFIX}${key}`, String(value));
  }
}
