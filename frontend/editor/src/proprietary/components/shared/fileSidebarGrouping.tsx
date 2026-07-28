// Classification override of the Files-sidebar grouping seam: Recent, one group
// per visible category, then Other. Labels cache onto stubs via a lazy backfill.

import { useEffect, useMemo, useRef, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { useIndexedDB } from "@app/contexts/IndexedDBContext";
import { useClassificationEnabled } from "@app/hooks/useClassificationEnabled";
import { fileStorage } from "@app/services/fileStorage";
import { readStubClassificationLabels } from "@app/services/fileClassification";
import {
  getSidebarCategories,
  subscribeSidebarCategories,
} from "@app/services/fileSidebarCategories";
import { buildLabelGroups } from "@app/components/shared/fileSidebarGroupingLogic";
import { scheduleIdle } from "@app/utils/scheduleIdle";
import type { FileId } from "@app/types/file";
import type { StirlingFileStub } from "@app/types/fileContext";
import type { FileSidebarGroup } from "@core/components/shared/fileSidebarGrouping";

export type { FileSidebarGroup };
// Pure grouping logic lives in a component-free module so tests don't drag in the picker's UI deps.
export {
  buildLabelGroups,
  bucketStubsByLabel,
} from "@app/components/shared/fileSidebarGroupingLogic";
// The sidebar's group-picker button + modal (core renders a null stub).
export { FileSidebarGroupControls } from "@app/components/shared/FileSidebarGroupControls";

/** Files read per effect pass, so a big library backfills over several ticks. */
const BACKFILL_BATCH = 3;

export function useFileSidebarGroups(
  stubs: StirlingFileStub[],
): FileSidebarGroup[] | null {
  const { t } = useTranslation();
  // Classification off (core): flat list, no category fetch or backfill.
  const enabled = useClassificationEnabled();
  const { bumpRevision } = useIndexedDB();
  // Reads keyed by id+lastModified, so a new file version is re-read exactly once.
  const attempted = useRef<Set<string>>(new Set());
  const attemptKey = (s: StirlingFileStub) =>
    `${s.id as string}:${s.lastModified ?? 0}`;

  // Backfill labels from file metadata onto stubs, a few per idle pass.
  // The heuristic path stamps stubs directly.
  useEffect(() => {
    if (!enabled) return;
    const pending = stubs
      .filter(
        (s) => !s.classificationLabels && !attempted.current.has(attemptKey(s)),
      )
      .slice(0, BACKFILL_BATCH);
    if (pending.length === 0) return;
    let cancelled = false;
    const cancelIdle = scheduleIdle(() => {
      if (cancelled) return;
      void (async () => {
        let wrote = false;
        for (const stub of pending) {
          const labels = await readStubClassificationLabels(stub);
          if (cancelled) return;
          attempted.current.add(attemptKey(stub));
          if (labels) {
            const ok = await fileStorage.updateFileMetadata(stub.id as FileId, {
              classificationLabels: labels,
            });
            if (ok) wrote = true;
          }
        }
        // One revision bump per batch → the sidebar re-reads and re-groups.
        if (!cancelled && wrote) bumpRevision();
      })();
    });
    return () => {
      cancelled = true;
      cancelIdle();
    };
  }, [enabled, stubs, bumpRevision]);

  const categories = useSyncExternalStore(
    subscribeSidebarCategories,
    getSidebarCategories,
  );
  return useMemo(
    () => (enabled ? buildLabelGroups(stubs, t, categories) : null),
    [enabled, stubs, t, categories],
  );
}
