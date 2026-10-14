import { afterEach, vi } from "vitest";

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
    // Self-test runs are shared across views for the app's lifetime: each test starts clean.
    // Imported here, not at the top: a test file's own module mocks (settings' image JSON) must
    // apply before anything pulls those modules in.
    (await import("@/features/machine/use-self-test")).resetSelfTests();
    (await import("@/lib/shared-poll")).resetSharedPolls();
    (await import("@/lib/deploy-store")).resetDeployStore();
    (await import("@/lib/sign-in")).resetSignIn();
    try {
      localStorage.clear();
    } catch {
      /* no storage */
    }
  });
}
