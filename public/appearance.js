// Applies the saved appearance (Settings > Appearance) before the first paint, so the app never
// flashes Dark before switching to Black or Light. Kept in sync with src/lib/appearance.ts.
(function () {
  try {
    var m = localStorage.getItem("cyberctf.appearance");
    if (m === "black" || m === "light") document.documentElement.classList.add(m);
  } catch {
    /* storage unavailable: Dark */
  }
})();
