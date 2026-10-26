import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";
import { resetCharts } from "./src/test/chart-mock";

vi.mock("next/navigation", async () => {
  const { navigation } = await import("./src/test/navigation");
  return {
    usePathname: () => navigation.pathname,
    useRouter: () => ({ push: vi.fn(), replace: vi.fn(), refresh: vi.fn(), back: vi.fn(), forward: vi.fn(), prefetch: vi.fn() }),
    useSearchParams: () => new URLSearchParams(),
    notFound: () => {
      throw new Error("NEXT_NOT_FOUND");
    },
  };
});

vi.mock("lightweight-charts", async (importOriginal) => {
  const actual = await importOriginal<typeof import("lightweight-charts")>();
  const { mockChartModule } = await import("./src/test/chart-mock");
  return { ...actual, ...mockChartModule };
});

vi.mock("@number-flow/react", async () => {
  const { NumberFlowMock } = await import("./src/test/number-flow-mock");
  return { default: NumberFlowMock };
});

/**
 * Unmounts each test's tree, then cuts every iframe it held out of that removed tree. jsdom keeps an
 * iframe's window alive after the element goes, and the window's `_frameElement` still links the
 * iframe to its old parent and siblings, so without this every removed tree with an iframe in it
 * (the landing page's app tab) stays in memory with its React fibers: about 50 MB a render, until
 * the worker runs out of heap and its remaining tests never run.
 */
afterEach(() => {
  const frames = typeof document === "undefined" ? [] : [...document.querySelectorAll("iframe")];
  cleanup();
  for (const frame of frames) frame.remove();
  resetCharts();
});

const dom = typeof window !== "undefined";

// jsdom runs no CSS animations, so Base UI would keep closed popups mounted waiting for their exit.
if (dom) Object.assign(globalThis, { BASE_UI_ANIMATIONS_DISABLED: true });

if (dom && !window.matchMedia) {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }),
  });
}

class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
if (dom && !("ResizeObserver" in window)) {
  Object.defineProperty(window, "ResizeObserver", { writable: true, value: ResizeObserverStub });
}

if (dom && !Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}

if (dom && !Element.prototype.getAnimations) {
  Element.prototype.getAnimations = () => [];
}
