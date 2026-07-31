import { describe, expect, test, vi } from "vitest";
import type { ToolRegistry } from "@app/data/toolsTaxonomy";
import { ToolType } from "@app/hooks/tools/shared/useToolOperation";
import {
  executeAutomationSequence,
  processMultiFileResponse,
} from "@app/utils/automationExecutor";
import { expectConsole } from "@app/tests/failOnConsole";

// Regression coverage for the automation-side mirror of the merge bug:
// non-canonical Content-Types previously misrouted a PDF into ZIP extraction
// and yielded a bogus `automation_*.zip`.

const PDF_BYTES = new Uint8Array([
  0x25,
  0x50,
  0x44,
  0x46,
  0x2d,
  0x31,
  0x2e,
  0x37, // "%PDF-1.7"
  0x0a,
  0x25,
  0xe2,
  0xe3,
  0xcf,
  0xd3,
  0x0a,
]);

const inputFiles = [
  new File(["fake"], "input.pdf", { type: "application/pdf" }),
];

async function run(contentType: string) {
  return processMultiFileResponse(
    new Blob([PDF_BYTES]),
    { "content-type": contentType },
    inputFiles,
    "automated_",
    false,
  );
}

describe("processMultiFileResponse (automation execution)", () => {
  test('PDF body + "application/octet-stream" -> PDF, not .zip', async () => {
    const result = await run("application/octet-stream");
    expect(result.length).toBe(1);
    expect(result[0].name).not.toMatch(/\.zip$/);
  });

  test('PDF body + "application/pdf;charset=UTF-8" -> PDF, not .zip', async () => {
    const result = await run("application/pdf;charset=UTF-8");
    expect(result.length).toBe(1);
    expect(result[0].name).not.toMatch(/\.zip$/);
  });

  test('PDF body + "APPLICATION/PDF" -> PDF, not .zip', async () => {
    const result = await run("APPLICATION/PDF");
    expect(result.length).toBe(1);
    expect(result[0].name).not.toMatch(/\.zip$/);
  });
});

describe("executeAutomationSequence", () => {
  test("runs steps in order and passes each step's output to the next step", async () => {
    const initialFile = new File(["input"], "input.pdf", {
      type: "application/pdf",
    });
    const compressedFile = new File(["compressed"], "compressed.pdf", {
      type: "application/pdf",
    });
    const rotatedFile = new File(["rotated"], "rotated.pdf", {
      type: "application/pdf",
    });
    const executionOrder: string[] = [];

    const compress = vi.fn(async (parameters, files: File[]) => {
      executionOrder.push("compress");
      expect(parameters).toEqual({ quality: "high" });
      expect(files).toEqual([initialFile]);
      return { files: [compressedFile] };
    });
    const rotate = vi.fn(async (parameters, files: File[]) => {
      executionOrder.push("rotate");
      expect(parameters).toEqual({ angle: 90 });
      expect(files).toEqual([compressedFile]);
      return { files: [rotatedFile] };
    });
    const onStepStart = vi.fn();
    const onStepComplete = vi.fn();
    const onStepError = vi.fn();

    const result = await executeAutomationSequence(
      {
        name: "Web ready",
        operations: [
          { operation: "compress", parameters: { quality: "high" } },
          { operation: "rotate", parameters: { angle: 90 } },
        ],
      },
      [initialFile],
      customProcessorRegistry({ compress, rotate }),
      onStepStart,
      onStepComplete,
      onStepError,
    );

    expect(result).toEqual([rotatedFile]);
    expect(executionOrder).toEqual(["compress", "rotate"]);
    expect(onStepStart.mock.calls).toEqual([
      [0, "compress"],
      [1, "rotate"],
    ]);
    expect(onStepComplete.mock.calls).toEqual([
      [0, [compressedFile]],
      [1, [rotatedFile]],
    ]);
    expect(onStepError).not.toHaveBeenCalled();
  });

  test("stops the chain and reports the failing step", async () => {
    expectConsole.error(/rotate/);

    const initialFile = new File(["input"], "input.pdf", {
      type: "application/pdf",
    });
    const compressedFile = new File(["compressed"], "compressed.pdf", {
      type: "application/pdf",
    });
    const compress = vi.fn(async () => ({ files: [compressedFile] }));
    const rotate = vi.fn(async () => {
      throw new Error("rotation exploded");
    });
    const watermark = vi.fn(async () => ({ files: [] }));
    const onStepComplete = vi.fn();
    const onStepError = vi.fn();

    await expect(
      executeAutomationSequence(
        {
          name: "Failing chain",
          operations: [
            { operation: "compress", parameters: {} },
            { operation: "rotate", parameters: {} },
            { operation: "watermark", parameters: {} },
          ],
        },
        [initialFile],
        customProcessorRegistry({ compress, rotate, watermark }),
        undefined,
        onStepComplete,
        onStepError,
      ),
    ).rejects.toThrow("rotate operation failed: rotation exploded");

    expect(onStepComplete).toHaveBeenCalledOnce();
    expect(onStepComplete).toHaveBeenCalledWith(0, [compressedFile]);
    expect(onStepError).toHaveBeenCalledOnce();
    expect(onStepError).toHaveBeenCalledWith(
      1,
      "rotate operation failed: rotation exploded",
    );
    expect(watermark).not.toHaveBeenCalled();
  });
});

type TestProcessor = (
  parameters: Record<string, unknown>,
  files: File[],
) => Promise<{ files: File[] }>;

function customProcessorRegistry(
  processors: Record<string, TestProcessor>,
): ToolRegistry {
  return Object.fromEntries(
    Object.entries(processors).map(([operation, customProcessor]) => [
      operation,
      {
        operationConfig: {
          operationType: operation,
          toolType: ToolType.custom,
          customProcessor,
        },
      },
    ]),
  ) as unknown as ToolRegistry;
}
