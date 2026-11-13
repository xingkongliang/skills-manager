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
