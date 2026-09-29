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

afterEach(() => {
  cleanup();
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
