import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  AppConfigProvider,
  useAppConfig,
} from "@app/contexts/AppConfigContext";
import apiClient from "@app/services/apiClient";
import { expectConsole } from "@app/tests/failOnConsole";

vi.mock("@app/services/apiClient");

describe("AppConfigContext", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  const wrapper = ({ children }: { children: ReactNode }) => (
    <AppConfigProvider>{children}</AppConfigProvider>
  );

  it("fetches and exposes the application configuration", async () => {
    const config = {
      appNameNavbar: "RustlingPDF",
      languages: ["en-US", "en-GB"],
    };
    vi.mocked(apiClient.get).mockResolvedValueOnce({
      status: 200,
      data: config,
    } as any);

    const { result } = renderHook(() => useAppConfig(), { wrapper });
    expect(result.current.loading).toBe(true);
    expect(result.current.config).toBeNull();

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.config).toEqual(config);
    expect(result.current.error).toBeNull();
    expect(apiClient.get).toHaveBeenCalledWith("/api/v1/config/app-config", {
      suppressErrorToast: true,
    });
  });

  it("exposes an empty fallback and error when the request fails", async () => {
    expectConsole.error(/\[AppConfig\] Failed to fetch app config/);
    vi.mocked(apiClient.get).mockRejectedValueOnce(new Error("offline"));

    const { result } = renderHook(() => useAppConfig(), { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(result.current.config).toEqual({});
    expect(result.current.error).toBe("offline");
  });

  it("supports an explicit refetch", async () => {
    vi.mocked(apiClient.get)
      .mockResolvedValueOnce({ status: 200, data: { appVersion: "1" } } as any)
      .mockResolvedValueOnce({ status: 200, data: { appVersion: "2" } } as any);

    const { result } = renderHook(() => useAppConfig(), { wrapper });
    await waitFor(() =>
      expect(result.current.config).toEqual({ appVersion: "1" }),
    );

    await act(async () => result.current.refetch());
    expect(result.current.config).toEqual({ appVersion: "2" });
    expect(apiClient.get).toHaveBeenCalledTimes(2);
  });

  it("can use an initial configuration without fetching", () => {
    const initialConfig = { appNameNavbar: "Offline RustlingPDF" };
    const localWrapper = ({ children }: { children: ReactNode }) => (
      <AppConfigProvider
        initialConfig={initialConfig}
        bootstrapMode="non-blocking"
        autoFetch={false}
      >
        {children}
      </AppConfigProvider>
    );

    const { result } = renderHook(() => useAppConfig(), {
      wrapper: localWrapper,
    });
    expect(result.current.config).toEqual(initialConfig);
    expect(result.current.loading).toBe(false);
    expect(apiClient.get).not.toHaveBeenCalled();
  });
});
