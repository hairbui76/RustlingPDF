import "@testing-library/jest-dom";
import { vi } from "vitest";

// Mirrors the editor's setup: jsdom lacks a handful of browser APIs that shared
// components (Mantine FocusTrap, responsive helpers) touch on render.

class LocalStorageMock implements Storage {
  private store: Record<string, string> = {};

  get length(): number {
    return Object.keys(this.store).length;
  }

  clear(): void {
    this.store = {};
  }

  getItem(key: string): string | null {
    return this.store[key] ?? null;
  }

  key(index: number): string | null {
    return Object.keys(this.store)[index] ?? null;
  }

  removeItem(key: string): void {
    delete this.store[key];
  }

  setItem(key: string, value: string): void {
    this.store[key] = value;
  }
}

Object.defineProperty(window, "localStorage", {
  value: new LocalStorageMock(),
  writable: true,
});

global.ResizeObserver = vi.fn().mockImplementation(() => ({
  observe: vi.fn(),
  unobserve: vi.fn(),
  disconnect: vi.fn(),
}));

global.IntersectionObserver = vi.fn().mockImplementation(() => ({
  observe: vi.fn(),
  unobserve: vi.fn(),
  disconnect: vi.fn(),
})) as unknown as typeof IntersectionObserver;

Object.defineProperty(window, "matchMedia", {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
});

// Provide a minimal DOMMatrix implementation for pdf.js in the test environment
// (mirrors src/core/setupTests.ts — portal tests pull in core modules that
// import pdfjs-dist at module load).
if (typeof globalThis.DOMMatrix === "undefined") {
  class DOMMatrixStub {
    a = 1;
    b = 0;
    c = 0;
    d = 1;
    e = 0;
    f = 0;

    constructor(init?: string | number[]) {
      if (Array.isArray(init) && init.length === 6) {
        [this.a, this.b, this.c, this.d, this.e, this.f] = init as [
          number,
          number,
          number,
          number,
          number,
          number,
        ];
      }
    }

    multiplySelf(): this {
      return this;
    }

    translateSelf(): this {
      return this;
    }

    scaleSelf(): this {
      return this;
    }

    rotateSelf(): this {
      return this;
    }

    inverse(): this {
      return this;
    }
  }

  Object.defineProperty(globalThis, "DOMMatrix", {
    value: DOMMatrixStub,
    writable: false,
    configurable: true,
  });
}
