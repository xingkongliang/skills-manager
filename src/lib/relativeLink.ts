/** The skill-relative path a link points at, or null when it is not relative. */
export function relativeLinkPath(href: string | undefined): string | null {
  if (!href) return null;
  const trimmed = href.trim();
  if (/^[a-z][a-z0-9+.-]*:/i.test(trimmed) || trimmed.startsWith("/")) return null;
  // A bare `#fragment` or `?query` strips to nothing and is not a file link.
  let path = trimmed.replace(/[?#].*$/, "");
  while (path.startsWith("./")) path = path.slice(2);
  try {
    path = decodeURI(path);
  } catch {
    // Keep the raw path; it simply will not match a file.
  }
  return path || null;
}

/**
 * Resolves a link path against the directory of the skill file it appears in
 * (both skill-relative and `/`-separated), handling `.` and `..`. Null when
 * the result climbs out of the skill or names its root.
 */
export function resolveRelativePath(fromFile: string, path: string): string | null {
  const parts = fromFile.split("/").slice(0, -1);
  for (const segment of path.split("/")) {
    if (segment === "" || segment === ".") continue;
    if (segment === "..") {
      if (parts.length === 0) return null;
      parts.pop();
    } else {
      parts.push(segment);
    }
  }
  return parts.length > 0 ? parts.join("/") : null;
}
