import { afterEach, vi } from "vitest";
import { resetModuleState } from "@/lib/module-state";

// Shared setup for every test file. Node-environment tests skip the DOM half.
if (typeof window !== "undefined") {
  const { cleanup } = await import("@testing-library/react");
  const { clearMocks } = await import("@tauri-apps/api/mocks");

  // `ignore()` logs expected, harmless failures at debug level: keep the test output readable.
  vi.spyOn(console, "debug").mockImplementation(() => {});

  // jsdom lacks the layout APIs React Flow, xterm and the dialogs reach for.
  class ResizeObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  // Assigned, not `vi.stubGlobal`: a test's `vi.unstubAllGlobals()` must not take them away.
  globalThis.ResizeObserver ??= ResizeObserverStub as unknown as typeof ResizeObserver;
  if (!window.matchMedia) {
    window.matchMedia = (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    });
  }
  Element.prototype.scrollIntoView ??= () => {};
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
  };

  afterEach(async () => {
    cleanup();
    clearMocks();
    // Shared polls, the deploy store and self-test runs live at module level: each test starts clean.
    resetModuleState();
    try {
      localStorage.clear();
    } catch {
      /* no storage */
    }
  });
}
