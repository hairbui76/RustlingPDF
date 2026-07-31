import React from "react";
import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useDocumentUnderstandingParameters } from "@app/hooks/tools/documentUnderstanding/useDocumentUnderstandingParameters";
import DocumentUnderstandingSettings from "@app/tools/documentUnderstanding/DocumentUnderstandingSettings";
import DocumentUnderstandingResults from "@app/tools/documentUnderstanding/DocumentUnderstandingResults";
import type { DocumentUnderstandingResponse } from "@app/tools/documentUnderstanding/types";

function SettingsHarness() {
  const parameters = useDocumentUnderstandingParameters();
  return (
    <>
      <DocumentUnderstandingSettings parameters={parameters} aiEnabled={true} />
      <output data-testid="parameters">
        {JSON.stringify(parameters.parameters)}
      </output>
    </>
  );
}

describe("Document understanding panels", () => {
  it("keeps provider disclosure visible and builds extraction fields", async () => {
    const user = userEvent.setup();
    render(
      <MantineProvider>
        <SettingsHarness />
      </MantineProvider>,
    );

    expect(
      screen.getByText(/bounded extracted page text is sent/i),
    ).toBeInTheDocument();
    await user.click(screen.getByText("Extract"));
    await user.type(
      screen.getByLabelText("What should be extracted?"),
      "Invoice number",
    );
    await user.click(screen.getByRole("button", { name: "Add field" }));

    const parameters = JSON.parse(
      screen.getByTestId("parameters").textContent ?? "{}",
    );
    expect(parameters.mode).toBe("extraction");
    expect(parameters.extractionFields).toHaveLength(2);
    expect(parameters.extractionFields[0]).toMatchObject({
      key: "field_1",
      description: "Invoice number",
      valueType: "string",
    });
  });

  it("shows partial translation blocks instead of silently dropping them", () => {
    const response: DocumentUnderstandingResponse = {
      schemaVersion: 1,
      operation: "translation",
      providerDisclosure:
        "Extracted document text was sent to the configured provider.",
      source: {
        fileName: "source.pdf",
        pagesProcessed: 1,
        charactersProcessed: 12,
        maxPages: 200,
        maxCharacters: 200_000,
      },
      result: {
        sourceLanguage: "English",
        targetLanguage: "Vietnamese",
        pages: [
          {
            pageNumber: 1,
            blocks: [
              {
                blockId: "p1-b1",
                sourceText: "Hello",
                translatedText: "Xin chào",
              },
              {
                blockId: "p1-b2",
                sourceText: "Still pending",
                translatedText: "",
              },
            ],
          },
        ],
      },
    };
    render(
      <MantineProvider>
        <DocumentUnderstandingResults response={response} />
      </MantineProvider>,
    );

    expect(screen.getByText("Xin chào")).toBeInTheDocument();
    expect(screen.getByText("Still pending")).toBeInTheDocument();
    expect(screen.getByText("Not translated")).toBeInTheDocument();
    expect(screen.getByText(/configured provider/i)).toBeInTheDocument();
  });
});
