// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// The documentation version switcher. `cargo xtask book-site` (which builds the
// deployed site) copies this file to the site root, next to the
// `versions.json` it reads, and adds it to every page with a relative path, so
// the site works under any prefix. It adds:
//
// - a version selector to the menu bar, which keeps the reader on the same
//   page in the other version when that page exists there, and on that
//   version's first page when it does not;
// - a banner on every version but the newest release, saying which version the
//   page describes and linking the same page in the newest release.
//
// It changes nothing when `versions.json` is missing (a plain `mdbook build`).
(() => {
  "use strict";

  const script = document.currentScript;
  if (!script) return;
  const root = new URL(".", script.src);
  const here = new URL(window.location.href);
  if (!here.pathname.startsWith(root.pathname)) return;
  const relative = here.pathname.slice(root.pathname.length);

  fetch(new URL("versions.json", root))
    .then((response) => (response.ok ? response.json() : null))
    .then((site) => {
      if (site && Array.isArray(site.versions)) render(site);
    })
    .catch(() => {});

  // The version this page belongs to, and its path inside that version.
  function locate(site) {
    const first = relative.split("/")[0];
    const named = site.versions.find((v) => v.path !== "" && v.path === first + "/");
    if (named) return { current: named, page: relative.slice(named.path.length) };
    const atRoot = site.versions.find((v) => v.path === "");
    return { current: atRoot, page: relative };
  }

  // `page` in `version` when it exists there, else that version's first page.
  async function destination(version, page) {
    const target = new URL(version.path + page, root);
    try {
      const response = await fetch(target, { method: "HEAD" });
      if (response.ok) return target.href + here.hash;
    } catch (_) {
      // Unreachable: fall back to the version's first page.
    }
    return new URL(version.path, root).href;
  }

  function render(site) {
    const { current, page } = locate(site);
    if (!current) return;
    addStyle();
    addSelector(site, current, page);
    const latest = site.versions.find((v) => v.id === site.latest);
    if (latest && current.id !== latest.id) addBanner(current, latest, page);
  }

  function addSelector(site, current, page) {
    const bar = document.querySelector("#mdbook-menu-bar .right-buttons, #menu-bar .right-buttons");
    if (!bar) return;
    const select = document.createElement("select");
    select.className = "mcpc-version";
    select.setAttribute("aria-label", "Documentation version");
    select.title = "Documentation version";
    for (const version of site.versions) {
      const option = document.createElement("option");
      option.value = version.id;
      option.textContent = version.label;
      option.selected = version.id === current.id;
      select.append(option);
    }
    select.addEventListener("change", async () => {
      const chosen = site.versions.find((v) => v.id === select.value);
      if (chosen) window.location.href = await destination(chosen, page);
    });
    bar.prepend(select);
  }

  function addBanner(current, latest, page) {
    const main = document.querySelector("#mdbook-content main, #content main");
    if (!main) return;
    const banner = document.createElement("div");
    banner.className = "mcpc-version-banner";
    banner.setAttribute("role", "note");
    const what =
      current.id === "dev"
        ? "the unreleased main branch, which can describe features no release has yet"
        : "version " + current.label;
    banner.append("This page documents " + what + ". The latest release is " + latest.label.replace(/ \(latest\)$/, "") + ": ");
    const link = document.createElement("a");
    link.textContent = "read this page there";
    link.href = new URL(latest.path, root).href;
    destination(latest, page).then((href) => {
      link.href = href;
    });
    banner.append(link, ".");
    main.prepend(banner);
  }

  function addStyle() {
    const style = document.createElement("style");
    style.textContent = `
      .mcpc-version {
        margin-inline-end: 10px;
        padding: 2px 4px;
        max-width: 11em;
        font: inherit;
        font-size: 1.4rem;
        color: var(--icons);
        background: var(--bg);
        border: 1px solid var(--theme-popup-border);
        border-radius: 4px;
        cursor: pointer;
      }
      .mcpc-version:hover, .mcpc-version:focus-visible { color: var(--icons-hover); }
      @media (max-width: 600px) { .mcpc-version { max-width: 6.5em; margin-inline-end: 4px; } }
      .mcpc-version-banner {
        margin: 0 0 1.5em;
        padding: 0.6em 1em;
        color: var(--fg);
        background: var(--quote-bg);
        border-inline-start: 4px solid var(--warning-border);
        border-radius: 2px;
      }
    `;
    document.head.append(style);
  }
})();
