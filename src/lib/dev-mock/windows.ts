// The app's other windows, as the browser preview opens them: a new tab each.

/** The app opens Settings and the server setup in their own windows; here, a browser tab. The
 *  preview's switches (see `Scenario`) go along, so the new tab shows the same machine. */
export function openWindow(path: string) {
  const carried = new URLSearchParams(window.location.search);
  for (const own of ["mock", "window", "id", "kind", "runtime", "step", "onboarding", "link"]) carried.delete(own);
  const extra = carried.toString();
  window.open(`${path}${path.includes("?") ? "&" : "?"}mock${extra ? `&${extra}` : ""}`, "_blank");
}
