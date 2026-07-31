import { describe, expect, test } from "vitest";
import { PDFDocument } from "@cantoo/pdf-lib";
import {
  FULL_IMAGE_CORNERS,
  clampNormalized,
  createScansPdf,
  createScansPdfBytes,
  fitScanDimensions,
  moveScan,
  type MobileScan,
} from "@app/pages/mobileScannerProcessing";

const scans: MobileScan[] = [
  { id: "one", dataUrl: "data:one" },
  { id: "two", dataUrl: "data:two" },
  { id: "three", dataUrl: "data:three" },
];
const onePixelJpeg =
  "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////2wBDAf//////////////////////////////////////////////////////////////////////////////////////wAARCAABAAEDASIAAhEBAxEB/8QAFQABAQAAAAAAAAAAAAAAAAAAAAf/xAAUEAEAAAAAAAAAAAAAAAAAAAAA/9oADAMBAAIQAxAAAAF//8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABBQJ//8QAFBEBAAAAAAAAAAAAAAAAAAAAAP/aAAgBAwEBPwF//8QAFBEBAAAAAAAAAAAAAAAAAAAAAP/aAAgBAgEBPwF//8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQAGPwJ//8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPyF//9oADAMBAAIAAwAAABAf/8QAFBEBAAAAAAAAAAAAAAAAAAAAAP/aAAgBAwEBPxB//8QAFBEBAAAAAAAAAAAAAAAAAAAAAP/aAAgBAgEBPxB//8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxB//9k=";

describe("mobile scanner processing", () => {
  test("reorders pages without mutating the source batch", () => {
    const moved = moveScan(scans, 1, -1);
    expect(moved.map((scan) => scan.id)).toEqual(["two", "one", "three"]);
    expect(scans.map((scan) => scan.id)).toEqual(["one", "two", "three"]);
    expect(moveScan(scans, 0, -1)).toBe(scans);
    expect(moveScan(scans, 2, 1)).toBe(scans);
  });

  test("clamps manual perspective points to image space", () => {
    expect(clampNormalized(-0.2)).toBe(0);
    expect(clampNormalized(0.4)).toBe(0.4);
    expect(clampNormalized(1.8)).toBe(1);
    expect(FULL_IMAGE_CORNERS.bottomRightCorner).toEqual({ x: 1, y: 1 });
  });

  test("bounds large phone photos without changing their aspect ratio", () => {
    expect(fitScanDimensions(4_000, 3_000)).toEqual({
      width: 3_000,
      height: 2_250,
    });
    expect(fitScanDimensions(1_200, 900)).toEqual({
      width: 1_200,
      height: 900,
    });
  });

  test("exports ordered scans as one multi-page PDF", async () => {
    const file = await createScansPdf([onePixelJpeg, onePixelJpeg], "scan.pdf");
    const pdf = await PDFDocument.load(
      await createScansPdfBytes([onePixelJpeg, onePixelJpeg]),
    );
    expect(file.name).toBe("scan.pdf");
    expect(file.type).toBe("application/pdf");
    expect(pdf.getPageCount()).toBe(2);
  });
});
