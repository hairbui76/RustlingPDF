import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  TOOL_INTENT_ACTIONS,
  resolveToolIntent,
  navigateToToolIntent,
} from "@app/services/toolIntentService";

describe("toolIntentService", () => {
  describe("TOOL_INTENT_ACTIONS", () => {
    it("is the canonical action set shared with launch_intent.rs and provisioning.wxs", () => {
      // Guards the single-source-of-truth triple: if this changes, the WXS
      // verbs and the Rust allowlist must change in the same commit.
      expect([...TOOL_INTENT_ACTIONS]).toEqual([
        "open",
        "merge",
        "compress",
        "convert",
      ]);
    });
  });

  describe("resolveToolIntent", () => {
    it("maps tool actions to their tool ids", () => {
      expect(resolveToolIntent("merge")).toBe("merge");
      expect(resolveToolIntent("compress")).toBe("compress");
      expect(resolveToolIntent("convert")).toBe("convert");
    });

    it("maps the open action to no tool (plain open)", () => {
      expect(resolveToolIntent("open")).toBeNull();
    });

    it("degrades unknown and missing intents to plain open", () => {
      expect(resolveToolIntent("frobnicate")).toBeNull();
      expect(resolveToolIntent("")).toBeNull();
      expect(resolveToolIntent(null)).toBeNull();
      expect(resolveToolIntent(undefined)).toBeNull();
    });
  });

  describe("navigateToToolIntent", () => {
    const popstateListener = vi.fn();

    beforeEach(() => {
      window.history.replaceState(null, "", "/");
      popstateListener.mockClear();
      window.addEventListener("popstate", popstateListener);
    });

    afterEach(() => {
      window.removeEventListener("popstate", popstateListener);
      window.history.replaceState(null, "", "/");
    });

    it("pushes the tool route and dispatches popstate for a mapped intent", () => {
      expect(navigateToToolIntent("merge")).toBe(true);
      expect(window.location.pathname).toBe("/merge");
      expect(popstateListener).toHaveBeenCalledTimes(1);
    });

    it("routes each mapped action to its own tool route", () => {
      navigateToToolIntent("compress");
      expect(window.location.pathname).toBe("/compress");
      navigateToToolIntent("convert");
      expect(window.location.pathname).toBe("/convert");
    });

    it("still dispatches popstate when already on the tool route", () => {
      window.history.replaceState(null, "", "/merge");
      expect(navigateToToolIntent("merge")).toBe(true);
      expect(window.location.pathname).toBe("/merge");
      expect(popstateListener).toHaveBeenCalledTimes(1);
    });

    it("does not navigate for the open action", () => {
      expect(navigateToToolIntent("open")).toBe(false);
      expect(window.location.pathname).toBe("/");
      expect(popstateListener).not.toHaveBeenCalled();
    });

    it("does not navigate for unknown or missing intents", () => {
      expect(navigateToToolIntent("frobnicate")).toBe(false);
      expect(navigateToToolIntent(null)).toBe(false);
      expect(navigateToToolIntent(undefined)).toBe(false);
      expect(window.location.pathname).toBe("/");
      expect(popstateListener).not.toHaveBeenCalled();
    });
  });
});
