import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge, Tooltip } from "@mantine/core";
import { Button } from "@app/ui/Button";
import { ActionIcon } from "@app/ui/ActionIcon";
import CloseIcon from "@mui/icons-material/Close";
import OpenInNewIcon from "@mui/icons-material/OpenInNew";
import DriveFileMoveIcon from "@mui/icons-material/DriveFileMove";
import DeleteIcon from "@mui/icons-material/Delete";
import DownloadIcon from "@mui/icons-material/Download";
import PictureAsPdfIcon from "@mui/icons-material/PictureAsPdf";
import HistoryIcon from "@mui/icons-material/History";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";

import { FileId } from "@app/types/file";
import { FolderRecord } from "@app/types/folder";
import { RustlingFileStub } from "@app/types/fileContext";
import { formatFileSize, getFileDate } from "@app/utils/fileUtils";
import {
  downloadFileFromStorage,
  downloadMultipleFiles,
} from "@app/utils/downloadUtils";
import { fileStorage } from "@app/services/fileStorage";
import {
  VersionTimeline,
  DetailField,
} from "@app/components/filesPage/VersionTimeline";

interface FileDetailsPanelProps {
  selectedFileIds: FileId[];
  fileMap: Map<FileId, RustlingFileStub>;
  currentFolder: FolderRecord | null;
  onClose: () => void;
  onAddToWorkspace: (fileIds: FileId[]) => void;
  onMove: (fileIds: FileId[]) => void;
  onRemove: (fileIds: FileId[]) => void;
  /** On small screens, show a compact "Version journey" button instead of the
   *  full inline timeline (which opens onOpenVersionHistory). */
  compactVersions?: boolean;
  onOpenVersionHistory?: () => void;
}

export function FileDetailsPanel({
  selectedFileIds,
  fileMap,
  currentFolder,
  onClose,
  onAddToWorkspace,
  onMove,
  onRemove,
  compactVersions = false,
  onOpenVersionHistory,
}: FileDetailsPanelProps) {
  const { t } = useTranslation();
  const files = useMemo(
    () =>
      selectedFileIds
        .map((id) => fileMap.get(id))
        .filter((f): f is RustlingFileStub => Boolean(f)),
    [selectedFileIds, fileMap],
  );

  // Hooks must run before any early return.
  const [downloading, setDownloading] = useState(false);
  // Metadata (size/type/dates) is collapsed by default so the panel stays
  // short and the action buttons keep their pinned footer in view.
  const [fieldsOpen, setFieldsOpen] = useState(false);
  // Version journey is collapsed by default so the panel stays short.
  const [versionsOpen, setVersionsOpen] = useState(false);
  // Version chain for the selected file; empty for v1 or multi-select.
  const [versionChain, setVersionChain] = useState<RustlingFileStub[]>([]);
  const singleFileForChain = files.length === 1 ? files[0] : null;
  useEffect(() => {
    if (!singleFileForChain) {
      setVersionChain([]);
      return;
    }
    let cancelled = false;
    const rootId = (singleFileForChain.originalFileId ??
      singleFileForChain.id) as FileId;
    fileStorage
      .getHistoryChainStubs(rootId)
      .then((chain) => {
        if (!cancelled) setVersionChain(chain);
      })
      .catch((err) => {
        console.error("Failed to load version history", err);
        if (!cancelled) setVersionChain([]);
      });
    return () => {
      cancelled = true;
    };
  }, [singleFileForChain]);

  if (files.length === 0) {
    return null;
  }

  const single = files.length === 1 ? files[0]! : null;
  const totalSize = files.reduce((sum, f) => sum + f.size, 0);
  const ext = single ? (single.name.split(".").pop() ?? "").toUpperCase() : "";
  const handleDownload = async () => {
    setDownloading(true);
    try {
      if (single) {
        await downloadFileFromStorage(single);
      } else {
        await downloadMultipleFiles(files);
      }
    } catch (err) {
      console.error("Download failed", err);
    } finally {
      setDownloading(false);
    }
  };

  return (
    <aside
      className="files-page-details"
      aria-label={t("filesPage.details", "Details")}
    >
      <div className="files-page-details-header">
        <strong>
          {single
            ? t("filesPage.details", "Details")
            : t("filesPage.detailsCount", "{{count}} files selected", {
                count: files.length,
              })}
        </strong>
        <Tooltip
          label={t("filesPage.closeDetails", "Close details")}
          withinPortal
        >
          <ActionIcon
            variant="tertiary"
            size="sm"
            onClick={onClose}
            aria-label={t("filesPage.closeDetails", "Close details")}
          >
            <CloseIcon fontSize="small" />
          </ActionIcon>
        </Tooltip>
      </div>

      <div className="files-page-details-body">
        {single ? (
          <>
            <div
              className={`files-page-details-thumb${
                compactVersions ? " is-compact" : ""
              }`}
            >
              {single.thumbnailUrl ? (
                <img src={single.thumbnailUrl} alt="" />
              ) : (
                <PictureAsPdfIcon
                  style={{ fontSize: "3rem", color: "var(--c-text-subtle)" }}
                />
              )}
            </div>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: "0.5rem",
                flexWrap: "wrap",
              }}
            >
              <h3 style={{ margin: 0, wordBreak: "break-word", flex: 1 }}>
                {single.name}
              </h3>
              {ext && (
                // Custom span; Mantine Badge default rendered invisible in dark mode.
                <span className="files-page-details-ext-tag">{ext}</span>
              )}
              {(single.versionNumber ?? 1) > 1 && (
                <Badge size="sm" color="blue">
                  v{single.versionNumber}
                </Badge>
              )}
            </div>
            <Button
              variant="tertiary"
              className="files-page-details-collapse-toggle"
              onClick={() => setFieldsOpen((o) => !o)}
              aria-expanded={fieldsOpen}
              rightSection={
                <KeyboardArrowDownIcon
                  className={`files-page-details-collapse-chevron${
                    fieldsOpen ? " is-open" : ""
                  }`}
                  fontSize="small"
                />
              }
            >
              <span>{t("filesPage.fileInfo", "File info")}</span>
            </Button>
            {fieldsOpen && (
              <div className="files-page-details-fieldlist">
                <DetailField
                  label={t("filesPage.field.size", "Size")}
                  value={formatFileSize(single.size)}
                />
                <DetailField
                  label={t("filesPage.field.type", "Type")}
                  value={single.type || "-"}
                />
                <DetailField
                  label={t("filesPage.field.modified", "Modified")}
                  value={getFileDate({ lastModified: single.lastModified })}
                />
                <DetailField
                  label={t("filesPage.field.added", "Added")}
                  value={
                    single.createdAt
                      ? getFileDate({ lastModified: single.createdAt })
                      : "-"
                  }
                />
                <DetailField
                  label={t("filesPage.field.folder", "Folder")}
                  value={
                    currentFolder
                      ? currentFolder.name
                      : t("filesPage.allFiles", "All files")
                  }
                />
              </div>
            )}
            {/* Version journey. Each tool run writes a new RustlingFile
                with the same `originalFileId` and an incremented
                `versionNumber`, so the chain reconstructs the edit
                timeline. The previous file manager exposed this and the
                refactored one had silently dropped it; this revival also
                shows WHICH tool was added at each step (the delta from
                the prior version) so the user can read the journey
                top-to-bottom. Long chains (> 6) collapse the middle. */}
            {versionChain.length > 1 &&
              (compactVersions && onOpenVersionHistory ? (
                <Button
                  leftSection={<HistoryIcon fontSize="small" />}
                  variant="secondary"
                  onClick={onOpenVersionHistory}
                >
                  {t(
                    "filesPage.viewVersionHistory",
                    "Version journey ({{count}})",
                    { count: versionChain.length },
                  )}
                </Button>
              ) : (
                <>
                  <Button
                    variant="quiet"
                    fullWidth
                    justify="between"
                    className="files-page-details-collapse-toggle"
                    onClick={() => setVersionsOpen((o) => !o)}
                    aria-expanded={versionsOpen}
                    rightSection={
                      <KeyboardArrowDownIcon
                        className={`files-page-details-collapse-chevron${
                          versionsOpen ? " is-open" : ""
                        }`}
                        fontSize="small"
                      />
                    }
                  >
                    <span>
                      {t(
                        "filesPage.viewVersionHistory",
                        "Version journey ({{count}})",
                        { count: versionChain.length },
                      )}
                    </span>
                  </Button>
                  {versionsOpen && (
                    <VersionTimeline
                      chain={versionChain}
                      currentId={single.id}
                      onAddToWorkspace={onAddToWorkspace}
                      onRemove={onRemove}
                      hideHeader
                    />
                  )}
                </>
              ))}
          </>
        ) : (
          <div className="files-page-details-fieldlist">
            <DetailField
              label={t("filesPage.field.totalSize", "Total size")}
              value={formatFileSize(totalSize)}
            />
            <DetailField
              label={t("filesPage.field.count", "Files")}
              value={String(files.length)}
            />
          </div>
        )}
      </div>

      <div className="files-page-details-actions">
        <Button
          leftSection={<OpenInNewIcon fontSize="small" />}
          onClick={() => onAddToWorkspace(selectedFileIds)}
        >
          {files.length === 1
            ? t("filesPage.addToWorkspace", "Add to workspace")
            : t("filesPage.addToWorkspaceCount", "Add {{count}} to workspace", {
                count: files.length,
              })}
        </Button>
        <Button
          leftSection={<DownloadIcon fontSize="small" />}
          variant="secondary"
          onClick={handleDownload}
          loading={downloading}
        >
          {single
            ? t("filesPage.download", "Download")
            : t("filesPage.downloadAll", "Download all")}
        </Button>
        <Button
          leftSection={<DriveFileMoveIcon fontSize="small" />}
          variant="secondary"
          onClick={() => onMove(selectedFileIds)}
        >
          {t("filesPage.moveTo", "Move to…")}
        </Button>
        <Button
          leftSection={<DeleteIcon fontSize="small" />}
          accent="danger"
          onClick={() => onRemove(selectedFileIds)}
        >
          {t("filesPage.remove", "Delete")}
        </Button>
      </div>
    </aside>
  );
}
