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

  // `new URL` normalizes `file://localhost/...` to an empty host; any remaining
  // host denotes a remote or UNC authority, which local file resolution rejects.
  if (parsed.host) {
    return null;
  }

  if (/%(?:2f|5c)/i.test(parsed.pathname)) {
    return null;
  }

  let pathname;
  try {
    pathname = decodeURIComponent(parsed.pathname);
  } catch (_error) {
    return null;
  }

  if (pathname.includes("\0")) {
    return null;
  }

  if (platform === "win32") {
    if (pathname.startsWith("//")) {
      return null;
    }
    if (/^\/[A-Za-z]:/.test(pathname)) {
      return pathname.slice(1).replace(/\//g, "\\");
    }
    return pathname.replace(/\//g, "\\");
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
