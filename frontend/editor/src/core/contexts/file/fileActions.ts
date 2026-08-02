/**
 * File actions - Unified file operations with single addFiles helper
 */

import {
  RustlingFileStub,
  FileContextAction,
  FileContextState,
  createNewRustlingFileStub,
  createFileId,
  createQuickKey,
  createRustlingFile,
  ProcessedFileMetadata,
} from "@app/types/fileContext";
import { FileId, ToolOperation } from "@app/types/file";
import { generateThumbnailWithMetadata } from "@app/utils/thumbnailUtils";
import { FileLifecycleManager } from "@app/contexts/file/lifecycle";
import { buildQuickKeySet } from "@app/contexts/file/fileSelectors";
import {
  addLocalPath,
  buildLocalPathIndex,
  isDuplicateFile,
} from "@app/contexts/file/fileDeduplication";
import { RustlingFile } from "@app/types/fileContext";
import { fileStorage } from "@app/services/fileStorage";
import { zipFileService } from "@app/services/zipFileService";
import { FileAnalyzer } from "@app/services/fileAnalyzer";
import {
  peekLocalFilePath,
  takeLocalFilePath,
} from "@app/services/localFilePathRegistry";
import {
  reportBulkAddProgress,
  clearBulkAddProgress,
} from "@app/services/bulkAddProgress";
const DEBUG = process.env.NODE_ENV === "development";
const HYDRATION_CONCURRENCY = 2;
let activeHydrations = 0;
const hydrationQueue: Array<() => Promise<void>> = [];

const scheduleMetadataHydration = (task: () => Promise<void>): void => {
  hydrationQueue.push(task);
  // Defer start to next tick to ensure initial ADD_FILES dispatch completes
  Promise.resolve().then(drainHydrationQueue);
};

const drainHydrationQueue = (): void => {
  if (activeHydrations >= HYDRATION_CONCURRENCY) {
    return;
  }
  const nextTask = hydrationQueue.shift();
  if (!nextTask) return;

  activeHydrations++;
  nextTask()
    .catch((error) => {
      console.error("[Hydration] Task failed with error:", error);
    })
    .finally(() => {
      activeHydrations--;
      drainHydrationQueue();
    });
};

/**
 * Simple mutex to prevent race conditions in addFiles
 */
class SimpleMutex {
  private locked = false;
  private queue: Array<() => void> = [];

  async lock(): Promise<void> {
    if (!this.locked) {
      this.locked = true;
      return Promise.resolve();
    }

    return new Promise<void>((resolve) => {
      this.queue.push(() => {
        this.locked = true;
        resolve();
      });
    });
  }

  unlock(): void {
    if (this.queue.length > 0) {
      const next = this.queue.shift()!;
      next();
    } else {
      this.locked = false;
    }
  }
}

// Global mutex for addFiles operations
const addFilesMutex = new SimpleMutex();

/**
 * Helper to create ProcessedFile metadata structure
 */
export function createProcessedFile(
  pageCount: number,
  thumbnail?: string,
  pageRotations?: number[],
  pageDimensions?: Array<{ width: number; height: number }>,
): ProcessedFileMetadata {
  return {
    totalPages: pageCount,
    pages: Array.from({ length: pageCount }, (_, index) => ({
      pageNumber: index + 1,
      thumbnail: index === 0 ? thumbnail : undefined, // Only first page gets thumbnail initially
      rotation: pageRotations?.[index] ?? 0,
      splitBefore: false,
      width: pageDimensions?.[index]?.width,
      height: pageDimensions?.[index]?.height,
    })),
    thumbnailUrl: thumbnail,
    lastProcessed: Date.now(),
  };
}

/**
 * Generate fresh ProcessedFileMetadata for a file
 * Used when tools process files to ensure metadata matches actual file content
 */
export async function generateProcessedFileMetadata(
  file: File,
): Promise<ProcessedFileMetadata | undefined> {
  // Only generate metadata for PDF files
  if (!file.type.startsWith("application/pdf")) {
    return undefined;
  }

  try {
    // Generate unrotated thumbnails for PageEditor (rotation applied via CSS)
    const unrotatedResult = await generateThumbnailWithMetadata(file, false);

    // Generate rotated thumbnail for file manager display
    const rotatedResult = await generateThumbnailWithMetadata(file, true);

    const processedFile = createProcessedFile(
      unrotatedResult.pageCount,
      unrotatedResult.thumbnail, // Page thumbnails (unrotated)
      unrotatedResult.pageRotations,
      unrotatedResult.pageDimensions,
    );

    // Use rotated thumbnail for file manager
    processedFile.thumbnailUrl = rotatedResult.thumbnail;

    if (unrotatedResult.isEncrypted || rotatedResult.isEncrypted) {
      processedFile.isEncrypted = true;
    }

    return processedFile;
  } catch (error) {
    if (DEBUG)
      console.warn(
        `📄 Failed to generate processedFileMetadata for ${file.name}:`,
        error,
      );
  }

  return undefined;
}

/**
 * Create a child RustlingFileStub from a parent stub with proper history management.
 * Used when a tool processes an existing file to create a new version with incremented history.
 *
 * @param parentStub - The parent RustlingFileStub to create a child from
 * @param operation - Tool operation information (toolName, timestamp)
 * @param resultingFile - The processed File object
 * @param thumbnail - Optional thumbnail for the child
 * @param processedFileMetadata - Optional fresh metadata for the processed file
 * @returns New child RustlingFileStub with proper version history
 */
export function createChildStub(
  parentStub: RustlingFileStub,
  operation: ToolOperation,
  resultingFile: File,
  thumbnail?: string,
  processedFileMetadata?: ProcessedFileMetadata,
): RustlingFileStub {
  const newFileId = createFileId();

  // Build new tool history by appending to parent's history
  const parentToolHistory = parentStub.toolHistory || [];
  const newToolHistory = [...parentToolHistory, operation];

  // Calculate new version number
  const newVersionNumber = (parentStub.versionNumber || 1) + 1;

  // Determine original file ID (root of the version chain)
  const originalFileId = parentStub.originalFileId || parentStub.id;

  // Copy parent metadata but exclude processedFile to prevent stale data.
  //
  // `localFilePath` is excluded too, and that exclusion is load-bearing. It
  // names a specific file on the user's disk that this stub may overwrite in
  // place, so inheriting it by metadata spread means the caller decides which
  // file gets overwritten *implicitly*, via whichever stub it happened to pass
  // as the parent. Where outputs are paired to inputs by array index that
  // parent can be the wrong one, and a 1→N operation would hand the same path
  // to every output. Ownership of a path is now always an explicit assignment
  // by the caller — see the `carryLocalFilePaths` step in useToolOperation.
  const {
    processedFile: _processedFile,
    localFilePath: _localFilePath,
    ...parentMetadata
  } = parentStub;

  const childStub = {
    // Copy parent metadata (excluding processedFile)
    ...parentMetadata,

    // Update identity and version info
    id: newFileId,
    versionNumber: newVersionNumber,
    parentFileId: parentStub.id,
    originalFileId: originalFileId,
    toolHistory: newToolHistory,
    createdAt: Date.now(),
    isLeaf: true, // New child is the current leaf node
    name: resultingFile.name,
    size: resultingFile.size,
    type: resultingFile.type,
    lastModified: resultingFile.lastModified,
    thumbnailUrl: thumbnail,

    // Set fresh processedFile metadata (no inheritance from parent)
    processedFile: processedFileMetadata,

    // Dirty iff the parent was a file on disk: this output is a modified
    // version of it that has not been written back yet. The path itself is
    // attached separately and explicitly (see above); this flag only records
    // that unsaved work exists.
    isDirty: parentStub.localFilePath ? true : undefined,
  };

  if (DEBUG) {
    console.log("[createChildStub] Created child:", {
      childId: newFileId,
      parentId: parentStub.id,
      parentLocalFilePath: parentStub.localFilePath,
      // The child never has one at this point — it is assigned afterwards, and
      // only where the input→output pairing was provably one to one.
      childIsDirty: childStub.isDirty,
      versionNumber: newVersionNumber,
    });
  }

  return childStub;
}

interface AddFileOptions {
  files?: File[];

  // For 'processed' files
  filesWithThumbnails?: Array<{
    file: File;
    thumbnail?: string;
    pageCount?: number;
  }>;

  // Insertion position
  insertAfterPageId?: string;

  // Auto-selection after adding
  selectFiles?: boolean;

  /** Persist to IDB without dispatching to workspace state. */
  skipWorkspaceDispatch?: boolean;

  // Auto-unzip control
  autoUnzip?: boolean;
  autoUnzipFileLimit?: number;
  skipAutoUnzip?: boolean; // When true: always unzip (except HTML). Used for file uploads. When false: respect autoUnzip/autoUnzipFileLimit preferences. Used for tool outputs.
  confirmLargeExtraction?: (
    fileCount: number,
    fileName: string,
  ) => Promise<boolean>; // Optional callback to confirm extraction of large ZIP files
  allowDuplicates?: boolean;
}

/**
 * Unified file addition helper - replaces addFiles
 */
export async function addFiles(
  options: AddFileOptions,
  stateRef: React.MutableRefObject<FileContextState>,
  filesRef: React.MutableRefObject<Map<FileId, File>>,
  dispatch: React.Dispatch<FileContextAction>,
  lifecycleManager: FileLifecycleManager,
  enablePersistence: boolean = false,
): Promise<RustlingFile[]> {
  // Acquire mutex to prevent race conditions
  await addFilesMutex.lock();

  try {
    const rustlingFileStubs: RustlingFileStub[] = [];
    const rustlingFiles: RustlingFile[] = [];
    // Hydration tasks are scheduled per-file to update thumbnails/metadata without blocking add flow

    // Build quickKey lookup from existing files for deduplication
    const existingQuickKeys = buildQuickKeySet(stateRef.current.files.byId);
    // Which on-disk files each quickKey already stands for — the tiebreaker
    // when metadata alone cannot tell two documents apart. See
    // contexts/file/fileDeduplication.ts.
    const pathIndex = buildLocalPathIndex(stateRef.current.files.byId);

    const { files = [], allowDuplicates = false } = options;

    // ZIP pre-processing: Extract ZIP files with configurable behavior
    // - File uploads: skipAutoUnzip=true → always extract (except HTML)
    // - Tool outputs: skipAutoUnzip=false → respect user preferences
    const filesToProcess: File[] = [];
    const autoUnzip = options.autoUnzip ?? true; // Default to true
    const autoUnzipFileLimit = options.autoUnzipFileLimit ?? 4; // Default limit
    const skipAutoUnzip = options.skipAutoUnzip ?? false;
    const confirmLargeExtraction = options.confirmLargeExtraction;

    for (const file of files) {
      // Check if file is a ZIP
      if (zipFileService.isZipFile(file)) {
        try {
          if (DEBUG)
            console.log(`📄 addFiles: Detected ZIP file: ${file.name}`);

          // Check if ZIP contains HTML files - if so, keep as ZIP
          const containsHtml = await zipFileService.containsHtmlFiles(file);
          if (containsHtml) {
            if (DEBUG)
              console.log(
                `📄 addFiles: ZIP contains HTML, keeping as ZIP: ${file.name}`,
              );
            filesToProcess.push(file);
            continue;
          }

          // Apply extraction with preferences
          const extractedFiles = await zipFileService.extractWithPreferences(
            file,
            {
              autoUnzip,
              autoUnzipFileLimit,
              skipAutoUnzip,
              confirmLargeExtraction,
            },
          );

          if (extractedFiles.length === 1 && extractedFiles[0] === file) {
            // ZIP was not extracted (over limit or autoUnzip disabled)
            if (DEBUG)
              console.log(
                `📄 addFiles: ZIP not extracted (preferences): ${file.name}`,
              );
          } else {
            // ZIP was extracted
            if (DEBUG)
              console.log(
                `📄 addFiles: Extracted ${extractedFiles.length} files from ZIP: ${file.name}`,
              );
          }

          filesToProcess.push(...extractedFiles);
        } catch (error) {
          console.error(
            `📄 addFiles: Failed to process ZIP file ${file.name}:`,
            error,
          );
          // On error, keep the ZIP file as-is
          filesToProcess.push(file);
        }
      } else {
        // Not a ZIP file, add as-is
        filesToProcess.push(file);
      }
    }

    if (DEBUG)
      console.log(
        `📄 addFiles: After ZIP processing, ${filesToProcess.length} files to add`,
      );

    // Collect hydrations to schedule after dispatch so updateRustlingFileStub finds files in state.
    const pendingHydrations: Array<() => Promise<void>> = [];

    // Stream the batch into the workspace in chunks. The per-file pre-scan below
    // (dedupe, encryption sniff — which reads each PDF's bytes) takes real time
    // for a big folder drop; a single end-of-loop dispatch would leave the UI
    // frozen-looking for seconds and then dump hundreds of rows in one render.
    // Chunked dispatch keeps rows (and their thumbnail hydrations) streaming in,
    // and the progress store drives the sidebar's "Adding files…" indicator.
    const DISPATCH_CHUNK = 25;
    let flushedStubs = 0;
    let flushedHydrations = 0;
    const flushChunk = () => {
      if (
        !options.skipWorkspaceDispatch &&
        rustlingFileStubs.length > flushedStubs
      ) {
        dispatch({
          type: "ADD_FILES",
          payload: { rustlingFileStubs: rustlingFileStubs.slice(flushedStubs) },
        });
        flushedStubs = rustlingFileStubs.length;
      }
      // Hydrations only after their chunk is dispatched, so
      // updateRustlingFileStub finds the files in state.
      while (flushedHydrations < pendingHydrations.length) {
        scheduleMetadataHydration(pendingHydrations[flushedHydrations++]);
      }
    };

    reportBulkAddProgress(0, filesToProcess.length);
    let scannedCount = 0;

    for (const file of filesToProcess) {
      const quickKey = createQuickKey(file);

      // Soft deduplication. A path is stronger evidence than the metadata
      // key: a file opened from disk is only a duplicate if the workspace
      // already holds that same path. Files with no path fall back to the key
      // exactly as before, so web behaviour is unchanged.
      if (
        !allowDuplicates &&
        isDuplicateFile({
          quickKey,
          localFilePath: peekLocalFilePath(file),
          existingQuickKeys,
          pathIndex,
        })
      ) {
        // Not silent: a skipped file the user explicitly chose is otherwise
        // indistinguishable from one that failed to open.
        console.warn(
          `[FileActions] Skipped "${file.name}" — already in the workspace`,
        );
        reportBulkAddProgress(++scannedCount, filesToProcess.length);
        continue;
      }

      const fileId = createFileId();
      filesRef.current.set(fileId, file);

      // Create new filestub with minimal metadata; hydrate thumbnails/processedFile asynchronously
      const fileStub = createNewRustlingFileStub(file, fileId);
      // Early encryption detection for PDFs — set the flag before dispatch so the
      // viewer gate and modal queue pick it up immediately instead of after hydration
      if (file.type === "application/pdf") {
        try {
          if (await FileAnalyzer.isPDFUserPasswordProtected(file)) {
            fileStub.processedFile = (fileStub.processedFile || {
              pages: [],
            }) as any;
            fileStub.processedFile!.isEncrypted = true;
          }
        } catch (error) {
          // Never block upload on analysis failure — but log so it's debuggable
          // if an unencrypted file later appears to "hang" during processing.
          console.warn(
            "[FileActions] Early encryption detection failed for",
            file.name,
            error,
          );
        }
      }

      // Attach the on-disk path this File was read from (desktop only).
      //
      // Looked up by the File OBJECT, never by quickKey. quickKey is
      // `name|size|lastModified` — two distinct documents collide on it
      // routinely, and resolving a path through a colliding key hands one
      // file the path of another, which the next in-place save then
      // overwrites. See services/localFilePathRegistry.ts.
      const localFilePath = takeLocalFilePath(file);
      if (localFilePath) {
        if (DEBUG) {
          // DEBUG-gated: this fires per file, and a 300-file drop emitting a
          // log line each measurably stalls the main thread with devtools open.
          console.log(`[FileActions] localFilePath: ${localFilePath}`);
        }
        fileStub.localFilePath = localFilePath;
        // Track it for the rest of this batch too, so a third copy of a file
        // already added here is still recognised as a duplicate.
        addLocalPath(pathIndex, quickKey, localFilePath);
      }

      // Store insertion position if provided
      if (options.insertAfterPageId !== undefined) {
        fileStub.insertAfterPageId = options.insertAfterPageId;
      }

      if (!allowDuplicates) {
        existingQuickKeys.add(quickKey);
      }
      rustlingFileStubs.push(fileStub);

      // Create RustlingFile directly
      const rustlingFile = createRustlingFile(file, fileId);
      rustlingFiles.push(rustlingFile);

      // Capture per-file hydration task — scheduled after batch dispatch below
      pendingHydrations.push(async () => {
        const targetFile = filesRef.current.get(fileId);
        if (!targetFile) {
          return;
        }

        let processedFileMetadata: ProcessedFileMetadata | undefined;
        let thumbnail: string | undefined;

        if (targetFile.type.startsWith("application/pdf")) {
          if (fileStub.processedFile?.isEncrypted) {
            // Pre-dispatch detection already flagged this PDF as encrypted; PDF.js
            // can't produce thumbnails/metadata without the password, so re-parsing
            // here would just duplicate work. Metadata is refreshed after unlock.
            processedFileMetadata = fileStub.processedFile;
          } else {
            processedFileMetadata =
              await generateProcessedFileMetadata(targetFile);
            thumbnail = processedFileMetadata?.thumbnailUrl;
          }
        } else {
          try {
            const { generateThumbnailForFile } =
              await import("@app/utils/thumbnailUtils");
            thumbnail = await generateThumbnailForFile(targetFile);
          } catch (error) {
            console.warn(
              `[addFiles] Thumbnail generation failed for ${fileId}:`,
              error,
            );
          }
        }

        const updates: Partial<RustlingFileStub> = {};
        const primaryThumbnail =
          thumbnail ||
          processedFileMetadata?.thumbnailUrl ||
          processedFileMetadata?.pages?.[0]?.thumbnail;

        if (processedFileMetadata) {
          updates.processedFile = processedFileMetadata;
          updates.thumbnailUrl = primaryThumbnail;
        } else if (thumbnail) {
          updates.thumbnailUrl = primaryThumbnail;
        }

        if (primaryThumbnail && primaryThumbnail.startsWith("blob:")) {
          lifecycleManager.trackBlobUrl(primaryThumbnail);
        }

        if (Object.keys(updates).length > 0) {
          lifecycleManager.updateRustlingFileStub(fileId, updates, stateRef);
        }

        // Persist the thumbnail to IndexedDB so it's available in future sessions.
        // The file was stored before hydration ran, so it had no thumbnail yet.
        // Skip blob URLs — they're session-only and won't be valid after reload.
        if (
          primaryThumbnail &&
          enablePersistence &&
          !primaryThumbnail.startsWith("blob:")
        ) {
          try {
            await fileStorage.updateThumbnail(fileId, primaryThumbnail);
          } catch {
            // Non-critical — regenerated lazily on next hover
          }
        }
      });

      reportBulkAddProgress(++scannedCount, filesToProcess.length);
      if (rustlingFileStubs.length - flushedStubs >= DISPATCH_CHUNK) {
        flushChunk();
      }
    }

    // Flush the remainder (also the sole dispatch for small batches).
    flushChunk();

    // Persist to storage if enabled using fileStorage service
    if (enablePersistence && rustlingFiles.length > 0) {
      await Promise.all(
        rustlingFiles.map(async (rustlingFile, index) => {
          try {
            // Get corresponding stub with all metadata
            const fileStub = rustlingFileStubs[index];

            // Store using the cleaner signature - pass RustlingFile + RustlingFileStub directly
            await fileStorage.storeRustlingFile(rustlingFile, fileStub);

            if (DEBUG)
              console.log(
                `📄 addFiles: Stored file ${rustlingFile.name} with metadata:`,
                fileStub,
              );
          } catch (error) {
            console.error(
              "Failed to persist file to storage:",
              rustlingFile.name,
              error,
            );
          }
        }),
      );
    }

    return rustlingFiles;
  } finally {
    clearBulkAddProgress();
    // Always release mutex even if error occurs
    addFilesMutex.unlock();
  }
}

/**
 * Consume files helper - replace unpinned input files with output files
 * Now accepts pre-created RustlingFiles and RustlingFileStubs to preserve all metadata
 */
export async function consumeFiles(
  inputFileIds: FileId[],
  outputRustlingFiles: RustlingFile[],
  outputRustlingFileStubs: RustlingFileStub[],
  filesRef: React.MutableRefObject<Map<FileId, File>>,
  dispatch: React.Dispatch<FileContextAction>,
): Promise<FileId[]> {
  if (DEBUG)
    console.log(
      `📄 consumeFiles: Processing ${inputFileIds.length} input files, ${outputRustlingFiles.length} output files with pre-created stubs`,
    );

  // Validate that we have matching files and stubs
  if (outputRustlingFiles.length !== outputRustlingFileStubs.length) {
    throw new Error(
      `Mismatch between output files (${outputRustlingFiles.length}) and stubs (${outputRustlingFileStubs.length})`,
    );
  }

  // Store RustlingFiles in filesRef using their existing IDs (no ID generation needed)
  for (let i = 0; i < outputRustlingFiles.length; i++) {
    const rustlingFile = outputRustlingFiles[i];
    const stub = outputRustlingFileStubs[i];

    if (rustlingFile.fileId !== stub.id) {
      console.warn(
        `📄 consumeFiles: ID mismatch between RustlingFile (${rustlingFile.fileId}) and stub (${stub.id})`,
      );
    }

    filesRef.current.set(rustlingFile.fileId, rustlingFile);

    if (DEBUG)
      console.log(
        `📄 consumeFiles: Stored RustlingFile ${rustlingFile.name} with ID ${rustlingFile.fileId}`,
      );
  }

  // Persist the durable half: mark inputs non-leaf and store output versions.
  await fileStorage.persistVersionedOutputs(
    inputFileIds,
    outputRustlingFiles,
    outputRustlingFileStubs,
  );

  // Dispatch the consume action with pre-created stubs (no processing needed)
  dispatch({
    type: "CONSUME_FILES",
    payload: {
      inputFileIds,
      outputRustlingFileStubs: outputRustlingFileStubs,
    },
  });

  if (DEBUG)
    console.log(
      `📄 consumeFiles: Successfully consumed files - removed ${inputFileIds.length} inputs, added ${outputRustlingFileStubs.length} outputs`,
    );
  // Return the output file IDs for undo tracking
  return outputRustlingFileStubs.map((stub) => stub.id);
}

/**
/**
 * Undoes a previous consumeFiles operation by restoring input files and removing output files (unless pinned)
 */
export async function undoConsumeFiles(
  inputFiles: File[],
  inputRustlingFileStubs: RustlingFileStub[],
  outputFileIds: FileId[],
  filesRef: React.MutableRefObject<Map<FileId, File>>,
  dispatch: React.Dispatch<FileContextAction>,
  indexedDB?: {
    saveFile: (
      file: File,
      fileId: FileId,
      existingThumbnail?: string,
    ) => Promise<any>;
    deleteFile: (fileId: FileId) => Promise<void>;
    bumpRevision?: () => void;
  } | null,
): Promise<void> {
  if (DEBUG)
    console.log(
      `📄 undoConsumeFiles: Restoring ${inputRustlingFileStubs.length} input files, removing ${outputFileIds.length} output files`,
    );

  // Validate inputs
  if (inputFiles.length !== inputRustlingFileStubs.length) {
    throw new Error(
      `Mismatch between input files (${inputFiles.length}) and records (${inputRustlingFileStubs.length})`,
    );
  }

  // Create a backup of current filesRef state for rollback
  const backupFilesRef = new Map(filesRef.current);

  try {
    // Sync filesRef before dispatch — prevents bumpRevision re-renders from seeing stale output IDs with no File objects.
    outputFileIds.forEach((id) => filesRef.current.delete(id));
    inputFiles.forEach((file, index) => {
      const record = inputRustlingFileStubs[index];
      if (file && record && file.size > 0) {
        filesRef.current.set(record.id, file);
      }
    });

    // Mark restored files dirty if they have a local path (they now differ from disk).
    const stubsWithDirtyMarked = inputRustlingFileStubs.map((stub) =>
      stub.localFilePath ? { ...stub, isDirty: true } : stub,
    );

    // Dispatch with filesRef and state.files.ids now in sync.
    dispatch({
      type: "UNDO_CONSUME_FILES",
      payload: {
        inputRustlingFileStubs: stubsWithDirtyMarked,
        outputFileIds,
      },
    });

    // IDB cleanup fire-and-forget — state is already consistent when bumpRevision fires.
    if (indexedDB) {
      outputFileIds.forEach((fileId) => {
        indexedDB.deleteFile(fileId).catch((error) => {
          console.error(
            "📄 undoConsumeFiles: Failed to delete output file from IDB:",
            fileId,
            error,
          );
          // Bump revision so the sidebar re-reads IDB and orphaned files reappear.
          indexedDB.bumpRevision?.();
        });
      });
    }

    // Restore isLeaf in IDB — modal reads IDB directly and misses files if isLeaf=false.
    await Promise.all(
      inputRustlingFileStubs.map((stub) =>
        fileStorage.markFileAsLeaf(stub.id).catch((error) => {
          console.warn(
            `📄 undoConsumeFiles: Failed to restore isLeaf for ${stub.id}:`,
            error,
          );
        }),
      ),
    );

    if (DEBUG)
      console.log(
        `📄 undoConsumeFiles: Successfully undone consume operation - restored ${inputRustlingFileStubs.length} inputs, removed ${outputFileIds.length} outputs`,
      );
  } catch (error) {
    // Rollback filesRef to previous state
    if (DEBUG)
      console.error(
        "📄 undoConsumeFiles: Error during undo, rolling back filesRef",
        error,
      );
    filesRef.current.clear();
    backupFilesRef.forEach((file, id) => {
      filesRef.current.set(id, file);
    });
    throw error; // Re-throw to let caller handle
  }
}

/**
 * Action factory functions
 */

/**
 * Add files using existing RustlingFileStubs from storage - preserves all metadata
 * Use this when loading files that already exist in storage (FileManager, etc.)
 * RustlingFileStubs come with proper thumbnails, history, processing state
 */
export async function addRustlingFileStubs(
  rustlingFileStubs: RustlingFileStub[],
  options: { insertAfterPageId?: string; selectFiles?: boolean } = {},
  stateRef: React.MutableRefObject<FileContextState>,
  filesRef: React.MutableRefObject<Map<FileId, File>>,
  dispatch: React.Dispatch<FileContextAction>,
  lifecycleManager: FileLifecycleManager,
): Promise<RustlingFile[]> {
  await addFilesMutex.lock();

  try {
    // Show loading indicator while preparing files from storage
    if (rustlingFileStubs.length > 0) {
      dispatch({
        type: "SET_PROCESSING",
        payload: { isProcessing: true, progress: 0 },
      });
    }

    const loadedFiles: RustlingFile[] = [];
    let firstFileDispatched = false;

    // Process and dispatch files one by one for progressive UI updates
    for (const stub of rustlingFileStubs) {
      // Dedup by stable fileId. Two distinct files in history can share
      // name|size|lastModified (and therefore quickKey), so quickKey dedup
      // here would silently drop a legitimately different file.
      if (stateRef.current.files.byId[stub.id]) {
        if (DEBUG)
          console.log(
            `📄 Skipping already-loaded RustlingFileStub: ${stub.name}`,
          );
        continue;
      }

      // Use the original stub (preserves thumbnails, history, metadata!)
      const record = { ...stub };

      // Store insertion position if provided
      if (options.insertAfterPageId !== undefined) {
        record.insertAfterPageId = options.insertAfterPageId;
      }

      // Dispatch each file immediately as we process it (progressive loading)
      dispatch({ type: "ADD_FILES", payload: { rustlingFileStubs: [record] } });

      // Clear loading indicator after first file appears
      if (!firstFileDispatched) {
        firstFileDispatched = true;
        dispatch({
          type: "SET_PROCESSING",
          payload: { isProcessing: false, progress: 0 },
        });
      }

      // Load File object and hydrate metadata in background (non-blocking)
      const fileId = stub.id;

      // Load File object from IndexedDB asynchronously
      scheduleMetadataHydration(async () => {
        const rustlingFile = await fileStorage.getRustlingFile(fileId);
        if (!rustlingFile) {
          return;
        }

        // Store the loaded file in filesRef
        filesRef.current.set(fileId, rustlingFile);

        // Check if processedFile data needs regeneration
        if (rustlingFile.type.startsWith("application/pdf")) {
          const needsProcessing =
            !stub.processedFile ||
            !stub.processedFile.pages ||
            stub.processedFile.pages.length === 0 ||
            stub.processedFile.totalPages !== stub.processedFile.pages.length;

          if (needsProcessing) {
            // Regenerate metadata
            const processedFileMetadata =
              await generateProcessedFileMetadata(rustlingFile);

            if (processedFileMetadata) {
              const updates: Partial<RustlingFileStub> = {
                processedFile: processedFileMetadata,
              };

              // Update thumbnail only if current stub doesn't have one
              const currentStub = stateRef.current.files.byId[fileId];
              if (
                !currentStub?.thumbnailUrl &&
                processedFileMetadata.thumbnailUrl
              ) {
                updates.thumbnailUrl = processedFileMetadata.thumbnailUrl;
                if (processedFileMetadata.thumbnailUrl.startsWith("blob:")) {
                  lifecycleManager.trackBlobUrl(
                    processedFileMetadata.thumbnailUrl,
                  );
                }
              }

              lifecycleManager.updateRustlingFileStub(
                fileId,
                updates,
                stateRef,
              );
              return;
            }
          }
        }

        // Stub dispatch triggers re-render so the viewer appears (ADD_FILES alone doesn't update selectors).
        lifecycleManager.updateRustlingFileStub(fileId, {}, stateRef);
      });
    }

    return loadedFiles;
  } finally {
    addFilesMutex.unlock();
  }
}

export const createFileActions = (
  dispatch: React.Dispatch<FileContextAction>,
) => ({
  setSelectedFiles: (fileIds: FileId[]) =>
    dispatch({ type: "SET_SELECTED_FILES", payload: { fileIds } }),
  setSelectedPages: (pageNumbers: number[]) =>
    dispatch({ type: "SET_SELECTED_PAGES", payload: { pageNumbers } }),
  clearSelections: () => dispatch({ type: "CLEAR_SELECTIONS" }),
  setProcessing: (isProcessing: boolean, progress = 0) =>
    dispatch({ type: "SET_PROCESSING", payload: { isProcessing, progress } }),
  setHasUnsavedChanges: (hasChanges: boolean) =>
    dispatch({ type: "SET_UNSAVED_CHANGES", payload: { hasChanges } }),
  pinFile: (fileId: FileId) =>
    dispatch({ type: "PIN_FILE", payload: { fileId } }),
  unpinFile: (fileId: FileId) =>
    dispatch({ type: "UNPIN_FILE", payload: { fileId } }),
  resetContext: () => dispatch({ type: "RESET_CONTEXT" }),
  markFileError: (fileId: FileId) =>
    dispatch({ type: "MARK_FILE_ERROR", payload: { fileId } }),
  clearFileError: (fileId: FileId) =>
    dispatch({ type: "CLEAR_FILE_ERROR", payload: { fileId } }),
  clearAllFileErrors: () => dispatch({ type: "CLEAR_ALL_FILE_ERRORS" }),
  updateRustlingFileStub: (
    fileId: FileId,
    updates: Partial<RustlingFileStub>,
  ) =>
    dispatch({ type: "UPDATE_FILE_RECORD", payload: { id: fileId, updates } }),
});
