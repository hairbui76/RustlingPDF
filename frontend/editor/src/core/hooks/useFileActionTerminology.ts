import { useTranslation } from "react-i18next";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

/**
 * Wording for the add-files and write-files actions.
 *
 * "Upload" and "Download" describe moving bytes between the user's machine and
 * somewhere else. On desktop there is no somewhere else: the files are already
 * on the user's disk and the app writes back to that same disk, so the
 * accurate verbs are Open and Save.
 */
export function useFileActionTerminology() {
  const { t } = useTranslation();

  if (isDesktopRuntime()) {
    return {
      uploadFiles: t("fileManager.openFiles", "Open Files"),
      uploadFile: t("fileManager.openFile", "Open File"),
      upload: t("fileUpload.open", "Open"),
      dropFilesHere: t(
        "fileUpload.dropFilesHereOpen",
        "Drop files here or click the open button",
      ),
      addFiles: t("fileManager.openFiles", "Open Files"),
      mobileUpload: t("landing.mobileUpload", "Upload from Mobile"),
      uploadFromComputer: t(
        "fileSidebar.openFromComputer",
        "Open from computer",
      ),
      download: t("save", "Save"),
      downloadAll: t("workbenchBar.saveAll", "Save All"),
      downloadSelected: t("fileManager.saveSelected", "Save Selected"),
      downloadUnavailable: t(
        "saveUnavailable",
        "Save unavailable for this item",
      ),
      noFilesInStorage: t(
        "fileUpload.noFilesInStorageOpen",
        "No files available in storage. Open some files first.",
      ),
    };
  }

  return {
    uploadFiles: t("fileUpload.uploadFiles", "Upload Files"),
    uploadFile: t("fileUpload.uploadFile", "Upload File"),
    upload: t("fileUpload.upload", "Upload"),
    dropFilesHere: t(
      "fileUpload.dropFilesHere",
      "Drop files here or click the upload button",
    ),
    addFiles: t("landing.addFiles", "Add Files"),
    mobileUpload: t("landing.mobileUpload", "Upload from Mobile"),
    uploadFromComputer: t("landing.uploadFromComputer", "Upload from computer"),
    download: t("download", "Download"),
    downloadAll: t("workbenchBar.downloadAll", "Download All"),
    downloadSelected: t("fileManager.downloadSelected", "Download Selected"),
    downloadUnavailable: t(
      "downloadUnavailable",
      "Download unavailable for this item",
    ),
    noFilesInStorage: t(
      "fileUpload.noFilesInStorage",
      "No files available in storage. Upload some files first.",
    ),
  };
}
