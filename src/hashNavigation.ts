export const workspacePages = [
  "home",
  "records",
  "knowledge",
  "usage",
  "settings",
  "session",
] as const;
export type WorkspacePage = (typeof workspacePages)[number];

export function pageFromHash(hash: string): WorkspacePage {
  const page = hash.replace(/^#/, "");
  return workspacePages.includes(page as WorkspacePage)
    ? (page as WorkspacePage)
    : "home";
}

export function navigateHash(
  browser: { location: { hash: string }; history: Pick<History, "pushState"> },
  page: string,
): WorkspacePage {
  const next = pageFromHash(`#${page}`);
  if (browser.location.hash !== `#${next}`)
    browser.history.pushState(null, "", `#${next}`);
  return next;
}

export function observeHash(
  browser: Pick<
    Window,
    "location" | "history" | "addEventListener" | "removeEventListener"
  >,
  onChange: (page: WorkspacePage) => void,
): () => void {
  const sync = () => {
    const page = pageFromHash(browser.location.hash);
    if (browser.location.hash !== `#${page}`)
      browser.history.replaceState(null, "", `#${page}`);
    onChange(page);
  };
  sync();
  browser.addEventListener("hashchange", sync);
  browser.addEventListener("popstate", sync);
  return () => {
    browser.removeEventListener("hashchange", sync);
    browser.removeEventListener("popstate", sync);
  };
}
