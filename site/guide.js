// Shared by the install guides: copy buttons, live release info, and Mac chip detection.
(() => {
  const REPO = "p1e2r/lodestone-launcher";

  document.querySelectorAll("[data-copy]").forEach((b) => b.addEventListener("click", async () => {
    const text = document.getElementById(b.dataset.copy).textContent;
    const label = b.querySelector("span");
    try { await navigator.clipboard.writeText(text); label.textContent = "Copied"; }
    catch { label.textContent = "Select & copy"; }
    setTimeout(() => (label.textContent = "Copy"), 1800);
  }));

  const mb = (b) => b >= 1048576 ? `${(b / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`;
  fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: { Accept: "application/vnd.github+json" } })
    .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
    .then((rel) => {
      const date = new Date(rel.published_at).toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" });
      const sizes = Object.fromEntries((rel.assets || []).map((a) => [a.name, mb(a.size)]));
      document.querySelectorAll("[data-size]").forEach((el) => { if (sizes[el.dataset.size]) el.textContent = sizes[el.dataset.size]; });
      document.querySelectorAll("[data-latest]").forEach((el) => { el.textContent = `Version ${rel.tag_name.replace(/^v/, "")} · released ${date}`; });
    })
    .catch(() => {});

  // On the macOS guide, mark the build that matches this Mac.
  const boxes = document.querySelectorAll("[data-mac]");
  if (!boxes.length || !/Mac/.test(navigator.userAgent) || /iPhone|iPad/.test(navigator.userAgent)) return;
  (async () => {
    let arch = null;
    try {
      if (navigator.userAgentData && navigator.userAgentData.getHighEntropyValues) {
        const v = await navigator.userAgentData.getHighEntropyValues(["architecture"]);
        arch = v.architecture === "x86" ? "intel" : v.architecture === "arm" ? "arm" : null;
      }
    } catch {}
    if (!arch) {
      try {
        const gl = document.createElement("canvas").getContext("webgl");
        const ext = gl && gl.getExtension("WEBGL_debug_renderer_info");
        const r = ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : "";
        arch = /Intel|AMD|Radeon/i.test(r) ? "intel" : /Apple M/i.test(r) ? "arm" : null;
      } catch {}
    }
    if (arch) boxes.forEach((b) => b.classList.toggle("mine", b.dataset.mac === arch));
  })();
})();
