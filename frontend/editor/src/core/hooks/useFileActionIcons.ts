import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";
import { materialSymbol } from "@app/components/shared/LocalIcon";

const UploadIcon = materialSymbol("upload-rounded");
const DownloadOutlinedIcon = materialSymbol("download-rounded");
const FolderOpenOutlinedIcon = materialSymbol("folder-open-outline-rounded");
const SaveOutlinedIcon = materialSymbol("save-outline-rounded");

export interface FileActionIcons {
  upload: typeof UploadIcon;
  download: typeof DownloadOutlinedIcon;
  uploadIconName: string;
  downloadIconName: string;
  /** Also gates the WorkbenchBar's Save As button; undefined hides it. */
  saveAsIconName: string | undefined;
}

/**
 * Icons for the add-files and write-files actions.
 *
 * On desktop the files are already on the user's own disk, so up/down arrows
 * describe the wrong thing: nothing is uploaded and nothing is downloaded.
 * Open-folder and save are what actually happens.
 *
 * Web keeps `saveAsIconName` undefined: the browser's own download already
 * lets the user choose where the file goes, so a second Save As adds nothing.
 *
 * The names must be extractable by `scripts/generate-icons.js`, which bundles
 * only the icons it can find by scanning source. An unextracted name renders
 * as blank space with no error, because LocalIcon has no network fallback by
 * design — so changing a name here means checking that script still sees it.
 */
export function useFileActionIcons(): FileActionIcons {
  if (isDesktopRuntime()) {
    return {
      upload: FolderOpenOutlinedIcon,
      download: SaveOutlinedIcon,
      uploadIconName: "folder-open",
      downloadIconName: "save",
      saveAsIconName: "save-as",
    };
  }

  return {
    upload: UploadIcon,
    download: DownloadOutlinedIcon,
    uploadIconName: "upload",
    downloadIconName: "download",
    saveAsIconName: undefined,
  };
}
