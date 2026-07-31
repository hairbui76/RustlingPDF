import { RustlingFile, RustlingFileStub } from "@app/types/fileContext";
import {
  createChildStub,
  generateProcessedFileMetadata,
} from "@app/contexts/file/fileActions";
import { createRustlingFile } from "@app/types/fileContext";
import { ToolId } from "@app/types/toolId";

/**
 * Create RustlingFiles and RustlingFileStubs from exported files
 * Used when saving page editor changes to create version history
 */
export async function createRustlingFilesAndStubs(
  files: File[],
  parentStub: RustlingFileStub,
  toolId: ToolId,
): Promise<{ rustlingFiles: RustlingFile[]; stubs: RustlingFileStub[] }> {
  const rustlingFiles: RustlingFile[] = [];
  const stubs: RustlingFileStub[] = [];

  for (const file of files) {
    const processedFileMetadata = await generateProcessedFileMetadata(file);
    const childStub = createChildStub(
      parentStub,
      { toolId, timestamp: Date.now() },
      file,
      processedFileMetadata?.thumbnailUrl,
      processedFileMetadata,
    );

    const rustlingFile = createRustlingFile(file, childStub.id);
    rustlingFiles.push(rustlingFile);
    stubs.push(childStub);
  }

  return { rustlingFiles, stubs };
}
