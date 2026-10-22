/**
 * Startup phase marks (#394): named `performance` marks for the browser's road from
 * navigation to the first world frame, read back by `?debug`'s `loadingReport()`.
 * Each phase is marked once per page, at its first occurrence.
 */
const PREFIX = "vale:";

export function markStartup(phase: string): void {
  const name = PREFIX + phase;
  if (typeof performance === "undefined" || typeof performance.mark !== "function") { return; }
  if (performance.getEntriesByName(name, "mark").length === 0) { performance.mark(name); }
}

/** Every startup mark so far, in milliseconds since navigation. */
export function startupMarks(): Record<string, number> {
  if (typeof performance === "undefined" || typeof performance.getEntriesByType !== "function") { return {}; }
  return Object.fromEntries(performance.getEntriesByType("mark")
    .filter((mark) => mark.name.startsWith(PREFIX))
    .map((mark) => [mark.name.slice(PREFIX.length), Math.round(mark.startTime)]));
}
