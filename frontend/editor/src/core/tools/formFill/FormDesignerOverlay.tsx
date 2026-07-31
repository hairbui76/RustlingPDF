import React, { useMemo, useRef, useState } from "react";
import { useFormDesigner } from "@app/tools/formFill/FormDesignerContext";
import type { WidgetCoordinates } from "@app/tools/formFill/types";

interface DesignGesture {
  kind: "create" | "move" | "resize";
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
  fieldId?: string;
  widgetId?: string;
  original?: WidgetCoordinates;
}

function roundPoint(value: number): number {
  return Math.round(value * 10) / 10;
}

export function FormDesignerOverlay({
  pageIndex,
  pageWidth,
  pageHeight,
  scaleX,
  scaleY,
}: {
  pageIndex: number;
  pageWidth: number;
  pageHeight: number;
  scaleX: number;
  scaleY: number;
}) {
  const designer = useFormDesigner();
  const overlayRef = useRef<HTMLDivElement>(null);
  const [gesture, setGesture] = useState<DesignGesture | null>(null);
  const pdfWidth = pageWidth / scaleX;
  const pdfHeight = pageHeight / scaleY;
  const pageFields = useMemo(
    () =>
      designer.fields.flatMap((field) =>
        field.widgets
          .filter((widget) => widget.pageIndex === pageIndex)
          .map((widget) => ({ field, widget })),
      ),
    [designer.fields, pageIndex],
  );

  const pointFromEvent = (event: React.PointerEvent) => {
    const rectangle = overlayRef.current?.getBoundingClientRect();
    return {
      x: Math.max(
        0,
        Math.min(pdfWidth, (event.clientX - (rectangle?.left ?? 0)) / scaleX),
      ),
      y: Math.max(
        0,
        Math.min(pdfHeight, (event.clientY - (rectangle?.top ?? 0)) / scaleY),
      ),
    };
  };

  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    const point = pointFromEvent(event);
    setGesture({
      kind: "create",
      startX: point.x,
      startY: point.y,
      currentX: point.x,
      currentY: point.y,
    });
  };

  const startWidgetGesture = (
    event: React.PointerEvent<HTMLDivElement>,
    kind: "move" | "resize",
    fieldId: string,
    widgetId: string,
    widget: WidgetCoordinates,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    designer.selectField(
      fieldId,
      event.ctrlKey || event.metaKey || event.shiftKey,
    );
    const point = pointFromEvent(event);
    setGesture({
      kind,
      startX: point.x,
      startY: point.y,
      currentX: point.x,
      currentY: point.y,
      fieldId,
      widgetId,
      original: widget,
    });
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!gesture) return;
    const point = pointFromEvent(event);
    setGesture((current) =>
      current ? { ...current, currentX: point.x, currentY: point.y } : current,
    );
    if (!gesture.fieldId || !gesture.widgetId || !gesture.original) return;
    const deltaX = point.x - gesture.startX;
    const deltaY = point.y - gesture.startY;
    if (gesture.kind === "move") {
      designer.updateWidget(gesture.fieldId, gesture.widgetId, {
        x: roundPoint(
          Math.max(
            0,
            Math.min(
              pdfWidth - gesture.original.width,
              gesture.original.x + deltaX,
            ),
          ),
        ),
        y: roundPoint(
          Math.max(
            0,
            Math.min(
              pdfHeight - gesture.original.height,
              gesture.original.y + deltaY,
            ),
          ),
        ),
      });
    } else if (gesture.kind === "resize") {
      designer.updateWidget(gesture.fieldId, gesture.widgetId, {
        width: roundPoint(
          Math.max(
            4,
            Math.min(
              pdfWidth - gesture.original.x,
              gesture.original.width + deltaX,
            ),
          ),
        ),
        height: roundPoint(
          Math.max(
            4,
            Math.min(
              pdfHeight - gesture.original.y,
              gesture.original.height + deltaY,
            ),
          ),
        ),
      });
    }
  };

  const handlePointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!gesture) return;
    if (gesture.kind === "create") {
      const point = pointFromEvent(event);
      const dragWidth = Math.abs(point.x - gesture.startX);
      const dragHeight = Math.abs(point.y - gesture.startY);
      const activeType =
        designer.appendWidget && designer.selectedField
          ? designer.selectedField.type
          : designer.creationType;
      const compact = activeType === "checkbox" || activeType === "radio";
      const defaultWidth = compact ? 18 : 140;
      const defaultHeight = compact ? 18 : 28;
      const x =
        dragWidth < 4 ? gesture.startX : Math.min(gesture.startX, point.x);
      const y =
        dragHeight < 4 ? gesture.startY : Math.min(gesture.startY, point.y);
      const width = Math.min(
        pdfWidth - x,
        dragWidth < 4 ? defaultWidth : dragWidth,
      );
      const height = Math.min(
        pdfHeight - y,
        dragHeight < 4 ? defaultHeight : dragHeight,
      );
      if (width >= 4 && height >= 4) {
        designer.addWidget({
          pageIndex,
          x: roundPoint(x),
          y: roundPoint(y),
          width: roundPoint(width),
          height: roundPoint(height),
        });
      }
    }
    setGesture(null);
  };

  const preview =
    gesture?.kind === "create"
      ? {
          left: Math.min(gesture.startX, gesture.currentX) * scaleX,
          top: Math.min(gesture.startY, gesture.currentY) * scaleY,
          width: Math.abs(gesture.currentX - gesture.startX) * scaleX,
          height: Math.abs(gesture.currentY - gesture.startY) * scaleY,
        }
      : null;

  return (
    <div
      ref={overlayRef}
      data-form-designer-page={pageIndex}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={() => setGesture(null)}
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 30,
        pointerEvents: "auto",
        cursor: "crosshair",
        touchAction: "none",
      }}
    >
      {pageFields.map(({ field, widget }) => {
        const selected = designer.selectedIds.includes(field.id);
        return (
          <div
            key={widget.id}
            role="button"
            tabIndex={0}
            aria-label={`${field.label || field.name} field`}
            onPointerDown={(event) =>
              startWidgetGesture(event, "move", field.id, widget.id, widget)
            }
            style={{
              position: "absolute",
              left: widget.x * scaleX,
              top: widget.y * scaleY,
              width: widget.width * scaleX,
              height: widget.height * scaleY,
              boxSizing: "border-box",
              border: `2px solid ${
                selected ? "#1971c2" : "rgba(25, 113, 194, 0.65)"
              }`,
              borderRadius: 2,
              background: selected
                ? "rgba(34, 139, 230, 0.18)"
                : "rgba(34, 139, 230, 0.1)",
              boxShadow: selected
                ? "0 0 0 2px rgba(34, 139, 230, 0.2)"
                : "none",
              color: "#0b4f8a",
              fontSize: 11,
              fontWeight: 700,
              overflow: "hidden",
              padding: 2,
              cursor: "move",
              userSelect: "none",
            }}
          >
            {field.label || field.name}
            <div
              aria-label="Resize field"
              onPointerDown={(event) =>
                startWidgetGesture(event, "resize", field.id, widget.id, widget)
              }
              style={{
                position: "absolute",
                right: -1,
                bottom: -1,
                width: 12,
                height: 12,
                background: "#1971c2",
                border: "2px solid white",
                cursor: "nwse-resize",
              }}
            />
          </div>
        );
      })}
      {preview && (
        <div
          style={{
            position: "absolute",
            ...preview,
            border: "2px dashed #1971c2",
            background: "rgba(34, 139, 230, 0.12)",
            pointerEvents: "none",
          }}
        />
      )}
      {designer.appendWidget && (
        <div
          style={{
            position: "absolute",
            top: 8,
            left: 8,
            padding: "4px 8px",
            borderRadius: 4,
            background: "#1971c2",
            color: "white",
            fontSize: 11,
            fontWeight: 700,
            pointerEvents: "none",
          }}
        >
          Draw another widget for {designer.selectedField?.label}
        </div>
      )}
    </div>
  );
}

export default FormDesignerOverlay;
