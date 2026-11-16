export function classifyReaderLink(href: string): "internal" | "external" | "blocked" {
  const value = href.trim();
  if (/^(https?:\/\/|mailto:)/i.test(value)) return "external";
  if (/^[a-z][a-z\d+.-]*:/i.test(value) || value.startsWith("/") || value.includes("\\") || value.includes("\0")) return "blocked";
  return "internal";
}

export interface ReaderLocation { path: string; fragment: string | null; scrollTop: number }
export class ReaderHistory {
  private entries: ReaderLocation[] = [];
  private index = -1;
  get current() { return this.entries[this.index] ?? null; }
  get canBack() { return this.index > 0; }
  get canForward() { return this.index < this.entries.length - 1; }
  saveScroll(scrollTop: number) { if (this.current) this.current.scrollTop = scrollTop; }
  initial(defaultPath: string | null) {
    if (this.current) return { ...this.current, fragment: null };
    return defaultPath ? this.push(defaultPath) : null;
  }
  replaceCurrent(path: string) {
    if (!this.current) return this.push(path);
    this.entries[this.index] = { path, fragment: null, scrollTop: 0 };
    return { ...this.current! };
  }
  push(path: string, fragment: string | null = null) {
    this.entries = this.entries.slice(0, this.index + 1);
    this.entries.push({ path, fragment, scrollTop: 0 });
    this.index = this.entries.length - 1;
    return { ...this.current! };
  }
  move(delta: number) {
    const next = this.index + delta;
    if (next < 0 || next >= this.entries.length) return null;
    this.index = next;
    return { ...this.current!, fragment: null };
  }
}

export function imageBlobUrl(base64: string, mime: string) {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return URL.createObjectURL(new Blob([bytes], { type: mime }));
}

export function readerErrorKey(error: unknown) {
  const message = typeof error === "object" && error !== null && "message" in error ? String(error.message) : "";
  return /^reader\.[a-zA-Z]+$/.test(message) ? message : "reader.readFailed";
}

export function readerRequestId() {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}
