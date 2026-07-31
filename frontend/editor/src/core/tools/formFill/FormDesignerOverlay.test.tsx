import React from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  FormDesignerProvider,
  useFormDesigner,
} from "@app/tools/formFill/FormDesignerContext";
import { FormDesignerOverlay } from "@app/tools/formFill/FormDesignerOverlay";

function DesignerState() {
  const designer = useFormDesigner();
  return (
    <output data-testid="designer-state">
      {JSON.stringify(designer.fields)}
    </output>
  );
}

function fields() {
  return JSON.parse(screen.getByTestId("designer-state").textContent || "[]");
}

describe("FormDesignerOverlay", () => {
  it("draws, moves, and resizes a field in upper-left PDF coordinates", () => {
    Object.defineProperty(globalThis, "PointerEvent", {
      configurable: true,
      value: MouseEvent,
    });
    Object.defineProperty(HTMLElement.prototype, "setPointerCapture", {
      configurable: true,
      value: vi.fn(),
    });

    const { container } = render(
      <FormDesignerProvider>
        <FormDesignerOverlay
          pageIndex={0}
          pageWidth={400}
          pageHeight={600}
          scaleX={2}
          scaleY={2}
        />
        <DesignerState />
      </FormDesignerProvider>,
    );
    const overlay = container.querySelector(
      "[data-form-designer-page='0']",
    ) as HTMLDivElement;
    vi.spyOn(overlay, "getBoundingClientRect").mockReturnValue({
      left: 10,
      top: 20,
      right: 410,
      bottom: 620,
      width: 400,
      height: 600,
      x: 10,
      y: 20,
      toJSON: () => undefined,
    });

    fireEvent.pointerDown(overlay, {
      pointerId: 1,
      clientX: 30,
      clientY: 40,
    });
    fireEvent.pointerMove(overlay, {
      pointerId: 1,
      clientX: 230,
      clientY: 100,
    });
    fireEvent.pointerUp(overlay, {
      pointerId: 1,
      clientX: 230,
      clientY: 100,
    });

    expect(fields()[0].widgets[0]).toMatchObject({
      pageIndex: 0,
      x: 10,
      y: 10,
      width: 100,
      height: 30,
    });

    fireEvent.pointerDown(
      screen.getByRole("button", { name: "Text 1 field" }),
      {
        pointerId: 2,
        clientX: 30,
        clientY: 40,
      },
    );
    fireEvent.pointerMove(overlay, {
      pointerId: 2,
      clientX: 50,
      clientY: 80,
    });
    fireEvent.pointerUp(overlay, {
      pointerId: 2,
      clientX: 50,
      clientY: 80,
    });

    expect(fields()[0].widgets[0]).toMatchObject({
      x: 20,
      y: 30,
      width: 100,
      height: 30,
    });

    fireEvent.pointerDown(screen.getByLabelText("Resize field"), {
      pointerId: 3,
      clientX: 230,
      clientY: 100,
    });
    fireEvent.pointerMove(overlay, {
      pointerId: 3,
      clientX: 270,
      clientY: 120,
    });
    fireEvent.pointerUp(overlay, {
      pointerId: 3,
      clientX: 270,
      clientY: 120,
    });

    expect(fields()[0].widgets[0]).toMatchObject({
      x: 20,
      y: 30,
      width: 120,
      height: 40,
    });
  });
});
