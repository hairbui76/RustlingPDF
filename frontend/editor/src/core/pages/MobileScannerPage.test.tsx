import { beforeEach, describe, expect, test, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { ThemeProvider } from "@app/components/shared/ThemeProvider";
import { PreferencesProvider } from "@app/contexts/PreferencesContext";
import MobileScannerPage from "@app/pages/MobileScannerPage";

const translate = vi.hoisted(() => (key: string) => key);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: translate,
    i18n: { changeLanguage: vi.fn() },
  }),
  initReactI18next: { type: "3rdParty", init: vi.fn() },
  I18nextProvider: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock("@app/utils/loadJscanify", () => ({
  loadJscanify: vi.fn().mockResolvedValue(undefined),
}));

class ScannerStub {
  findPaperContour() {
    return undefined;
  }
  getCornerPoints() {
    throw new Error("not used");
  }
  extractPaper(canvas: HTMLCanvasElement) {
    return canvas;
  }
}

function renderScanner(url: string) {
  return render(
    <PreferencesProvider>
      <ThemeProvider>
        <MemoryRouter initialEntries={[url]}>
          <MobileScannerPage />
        </MemoryRouter>
      </ThemeProvider>
    </PreferencesProvider>,
  );
}

describe("MobileScannerPage", () => {
  beforeEach(() => {
    Object.defineProperty(window, "jscanify", {
      configurable: true,
      value: ScannerStub,
    });
  });

  test("opens directly in private local mode without a transfer session", async () => {
    renderScanner("/mobile-scanner");
    expect(await screen.findByText("mobileScanner.localOnly")).toBeVisible();
    expect(screen.getByText("mobileScanner.title")).toBeVisible();
    expect(screen.getByText("mobileScanner.localPrivacy")).toBeVisible();
  });

  test("an expired QR session can fall back to local scanning", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false }));
    const user = userEvent.setup();
    renderScanner("/mobile-scanner?session=expired");

    await user.click(
      await screen.findByRole("button", {
        name: "mobileScanner.continueLocally",
      }),
    );
    expect(await screen.findByText("mobileScanner.title")).toBeVisible();
    expect(screen.getByText("mobileScanner.localOnly")).toBeVisible();
    vi.unstubAllGlobals();
  });
});
