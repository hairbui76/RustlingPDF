import React, { type ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  FormDesignerProvider,
  useFormDesigner,
} from "@app/tools/formFill/FormDesignerContext";

function wrapper({ children }: { children: ReactNode }) {
  return <FormDesignerProvider>{children}</FormDesignerProvider>;
}

describe("FormDesignerContext", () => {
  it("creates API-ready fields and supports alignment and duplication", () => {
    const { result } = renderHook(() => useFormDesigner(), { wrapper });
    act(() => {
      result.current.setMode("make");
      result.current.addWidget({
        pageIndex: 0,
        x: 40,
        y: 30,
        width: 120,
        height: 24,
      });
      result.current.addWidget({
        pageIndex: 0,
        x: 10,
        y: 80,
        width: 120,
        height: 24,
      });
    });

    const ids = result.current.fields.map((field) => field.id);
    act(() => {
      result.current.selectField(ids[0]);
      result.current.selectField(ids[1], true);
      result.current.alignSelected("left");
    });
    expect(result.current.fields.map((field) => field.widgets[0].x)).toEqual([
      10, 10,
    ]);

    act(() => result.current.duplicateSelected());
    expect(result.current.fields).toHaveLength(4);
    expect(result.current.creationRequests).toHaveLength(4);
    expect(result.current.creationRequests[0]).not.toHaveProperty("id");
    expect(result.current.creationRequests[0].widgets[0]).not.toHaveProperty(
      "id",
    );
  });

  it("adds multiple radio widgets with one export value per widget", () => {
    const { result } = renderHook(() => useFormDesigner(), { wrapper });
    act(() => {
      result.current.setCreationType("radio");
      result.current.addWidget({
        pageIndex: 0,
        x: 10,
        y: 10,
        width: 18,
        height: 18,
      });
    });
    act(() => {
      result.current.setAppendWidget(true);
      result.current.addWidget({
        pageIndex: 0,
        x: 10,
        y: 40,
        width: 18,
        height: 18,
      });
    });

    const radio = result.current.fields[0];
    expect(radio.options).toEqual(["Option 1", "Option 2"]);
    expect(radio.widgets.map((widget) => widget.exportValue)).toEqual([
      "Option 1",
      "Option 2",
    ]);
    expect(result.current.creationRequests[0].widgets).toHaveLength(2);
  });
});
