import type { FileId } from "@app/types/file";

/**
 * Deciding which tool outputs may claim which file on the user's disk.
 *
 * `localFilePath` is not decoration: a stub that carries one will be
 * *overwritten in place* by the next save, with no dialog and no confirmation.
 * Handing an output the wrong path silently destroys a different document, so
 * the only acceptable failure mode here is to carry no path at all — the
 * output then gets a Save As dialog, which is a mild inconvenience rather than
 * data loss.
 *
 * Two rules produce that guarantee:
 *
 * 1. **Provenance must be known, not guessed.** The caller supplies, per
 *    output, the id of the input it came from, or `null` where the pairing is
 *    genuinely unknowable — a single backend call returning a ZIP tells us
 *    nothing about which member came from which upload. Pairing by array index
 *    across two lists built in different orders is exactly how an output ends
 *    up inheriting another file's path.
 * 2. **A path may be claimed by at most one output.** A 1→N operation gives
 *    several outputs the same source, and letting each write to it would run
 *    N overwrites of one file, leaving the user with the last fragment in
 *    place of their document.
 */
/**
 * Index entries by a name, mapping any name claimed more than once to null.
 *
 * Used to match tool outputs back to inputs by filename. Overwriting on
 * collision — the obvious `map.set(name, id)` in a loop — silently picks
 * whichever entry came last, and that choice decides which file on disk an
 * output may overwrite. Two inputs can legitimately share a name now that they
 * are distinguished by path rather than metadata, so ambiguity has to be
 * representable.
 */
export function indexUniqueNames<T>(
  entries: readonly { name: string; id: T }[],
): Map<string, T | null> {
  const index = new Map<string, T | null>();
  for (const entry of entries) {
    index.set(entry.name, index.has(entry.name) ? null : entry.id);
  }
  return index;
}

export interface CarrySource {
  id: FileId;
  localFilePath?: string;
}

export interface CarryOutput {
  id: FileId;
  /** Input this output was produced from, or null when not known. */
  sourceId: FileId | null;
}

export interface CarryAssignment {
  outputId: FileId;
  localFilePath: string;
}

export function planLocalFilePathCarry(
  sources: readonly CarrySource[],
  outputs: readonly CarryOutput[],
): CarryAssignment[] {
  const pathById = new Map<FileId, string>();
  for (const source of sources) {
    if (source.localFilePath) {
      pathById.set(source.id, source.localFilePath);
    }
  }
  if (pathById.size === 0) {
    return [];
  }

  // How many outputs descend from each source. More than one means the
  // operation fanned out, and none of them owns the original file.
  const claimsPerSource = new Map<FileId, number>();
  for (const output of outputs) {
    if (output.sourceId !== null && pathById.has(output.sourceId)) {
      claimsPerSource.set(
        output.sourceId,
        (claimsPerSource.get(output.sourceId) ?? 0) + 1,
      );
    }
  }

  const assignments: CarryAssignment[] = [];
  const takenPaths = new Set<string>();
  for (const output of outputs) {
    if (output.sourceId === null) continue;
    if (claimsPerSource.get(output.sourceId) !== 1) continue;

    const localFilePath = pathById.get(output.sourceId);
    if (!localFilePath) continue;
    // Belt and braces: two distinct sources should never report the same path,
    // but if they ever did, the second claim would be an overwrite.
    if (takenPaths.has(localFilePath)) continue;

    takenPaths.add(localFilePath);
    assignments.push({ outputId: output.id, localFilePath });
  }

  return assignments;
}
