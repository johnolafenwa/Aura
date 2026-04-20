"use strict";

function uriToPath(uri, platform = process.platform) {
  if (typeof uri !== "string") {
    return null;
  }

  let parsed;
  try {
    parsed = new URL(uri);
  } catch (_error) {
    return null;
  }

  if (parsed.protocol !== "file:") {
    return null;
  }

  const pathname = decodeURIComponent(parsed.pathname);
  if (platform === "win32") {
    if (parsed.host) {
      const segments = pathname.split("/").filter(Boolean).join("\\");
      return `\\\\${parsed.host}${segments ? `\\${segments}` : ""}`;
    }
    if (/^\/[A-Za-z]:/.test(pathname)) {
      return pathname.slice(1).replace(/\//g, "\\");
    }
    return pathname.replace(/\//g, "\\");
  }

  if (parsed.host) {
    return `//${parsed.host}${pathname}`;
  }
  return pathname;
}

module.exports = {
  uriToPath
};
