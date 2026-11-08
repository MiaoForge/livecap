import { describe, expect, it, vi } from "vitest";
import { navigateHash, observeHash, pageFromHash } from "./hashNavigation";

describe("hash navigation", () => {
  it.each(["settings", "usage", "knowledge", "records", "session"])(
    "opens a saved #%s URL",
    (page) => {
      expect(pageFromHash(`#${page}`)).toBe(page);
    },
  );
  it("defaults unsupported fragments to home", () => {
    expect(pageFromHash("#missing")).toBe("home");
    expect(pageFromHash("")).toBe("home");
  });
  it("synchronizes history changes and removes listeners on disposal", () => {
    const events = new EventTarget();
    const location = { hash: "#settings" };
    const history = {
      pushState: vi.fn((_state, _unused, url) => {
        location.hash = String(url);
      }),
      replaceState: vi.fn((_state, _unused, url) => {
        location.hash = String(url);
      }),
    };
    const browser = {
      location,
      history,
      addEventListener: events.addEventListener.bind(events),
      removeEventListener: events.removeEventListener.bind(events),
    } as unknown as Window;
    const changed = vi.fn();
    const dispose = observeHash(browser, changed);
    expect(changed).toHaveBeenLastCalledWith("settings");
    navigateHash(browser, "usage");
    navigateHash(browser, "usage");
    expect(history.pushState).toHaveBeenCalledTimes(1);
    location.hash = "#settings";
    events.dispatchEvent(new Event("popstate"));
    expect(changed).toHaveBeenLastCalledWith("settings");
    location.hash = "#knowledge";
    events.dispatchEvent(new Event("hashchange"));
    expect(changed).toHaveBeenLastCalledWith("knowledge");
    location.hash = "#bad";
    events.dispatchEvent(new Event("popstate"));
    expect(changed).toHaveBeenLastCalledWith("home");
    expect(history.replaceState).toHaveBeenLastCalledWith(null, "", "#home");
    dispose();
    changed.mockClear();
    events.dispatchEvent(new Event("hashchange"));
    events.dispatchEvent(new Event("popstate"));
    expect(changed).not.toHaveBeenCalled();
  });
});
