import {
  RustlingFile,
  FileId,
  RustlingFileStub,
  createRustlingFile,
  ProcessedFileMetadata,
  createNewRustlingFileStub,
} from "@app/types/fileContext";

/**
 * Builds parallel inputFileIds and inputRustlingFileStubs arrays from the valid input files.
 * Falls back to a fresh stub when the file is not found in the current context state
 * (e.g. it was removed between operation start and this point).
 */
export function buildInputTracking(
  validFiles: RustlingFile[],
  selectors: {
    getRustlingFileStub: (id: FileId) => RustlingFileStub | undefined;
  },
): { inputFileIds: FileId[]; inputRustlingFileStubs: RustlingFileStub[] } {
  const inputFileIds: FileId[] = [];
  const inputRustlingFileStubs: RustlingFileStub[] = [];
  for (const file of validFiles) {
    const fileId = file.fileId;
    const record = selectors.getRustlingFileStub(fileId);
    if (record) {
      inputFileIds.push(fileId);
      inputRustlingFileStubs.push(record);
    } else {
      console.debug(`No file stub found for file: ${file.name}`);
      inputFileIds.push(fileId);
      inputRustlingFileStubs.push(createNewRustlingFileStub(file, fileId));
    }
  }
  return { inputFileIds, inputRustlingFileStubs };
}

/**
 * Creates parallel outputRustlingFileStubs and outputRustlingFiles arrays from processed files.
 * The stubFactory determines how each stub is constructed (child version vs fresh root).
 */
export function buildOutputPairs(
  processedFiles: File[],
  thumbnails: string[],
  metadataArray: Array<ProcessedFileMetadata | undefined>,
  stubFactory: (
    file: File,
    thumbnail: string,
    metadata: ProcessedFileMetadata | undefined,
    index: number,
  ) => RustlingFileStub,
): {
  outputRustlingFileStubs: RustlingFileStub[];
  outputRustlingFiles: RustlingFile[];
} {
  const outputRustlingFileStubs = processedFiles.map((file, index) =>
    stubFactory(file, thumbnails[index], metadataArray[index], index),
  );
  const outputRustlingFiles = processedFiles.map((file, index) =>
    createRustlingFile(file, outputRustlingFileStubs[index].id),
  );
  return { outputRustlingFileStubs, outputRustlingFiles };
}
