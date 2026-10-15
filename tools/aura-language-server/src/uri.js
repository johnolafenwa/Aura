"use strict";

const fs = require("node:fs");
const { pathToFileURL } = require("node:url");

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

// The compiler reports canonical file paths. Resolve links so an editor URI
// and a compiler path for the same file produce the same key.
function canonicalFileUri(uri) {
  const filePath = uriToPath(uri);
  if (!filePath) {
    return uri;
  }
  try {
    return pathToFileURL(fs.realpathSync.native(filePath)).toString();
  } catch (_error) {
    return pathToFileURL(filePath).toString();
  }
}

module.exports = {
  canonicalFileUri,
  uriToPath
};
