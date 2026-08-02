import { openFileDialog } from "@app/services/fileDialogService";
import { rememberLocalFilePath } from "@app/services/localFilePathRegistry";
import { getDocumentFileDialogFilter } from "@app/utils/fileDialogUtils";

interface OpenFilesFromDiskOptions {
  multiple?: boolean;
  filters?: Array<{
    name: string;
    extensions: string[];
  }>;
  onFallbackOpen?: () => void;
}

export async function openFilesFromDisk(
  options: OpenFilesFromDiskOptions = {},
): Promise<File[]> {
  const filesWithPaths = await openFileDialog({
    multiple: options.multiple ?? true,
    filters: options.filters ?? getDocumentFileDialogFilter(),
  });

  if (filesWithPaths.length > 0) {
    for (const { file, path } of filesWithPaths) {
      // Registered against the File object. Two files that happen to share a
      // name, size and timestamp cannot take each other's path.
      rememberLocalFilePath(file, path);
    }
    return filesWithPaths.map((entry) => entry.file);
  }

  options.onFallbackOpen?.();
  return [];
}
