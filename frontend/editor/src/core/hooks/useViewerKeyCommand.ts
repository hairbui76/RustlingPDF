/**
 * Extra key handling for the viewer, on top of what the viewer already binds.
 * Returns true when the event was consumed.
 *
 * Nothing claims a key here today. The deleted desktop layer bound R /
 * Shift+R to rotate; that is a plain UI binding with no native dependency, so
 * if it is wanted it belongs in the viewer's own keymap for every runtime
 * rather than behind a desktop check.
 */
export function useViewerKeyCommand(): (event: KeyboardEvent) => boolean {
  return () => false;
}
