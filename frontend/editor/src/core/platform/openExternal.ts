/**
 * core/web implementation of the @app/platform/openExternal seam.
 *
 * The browser build opens external URLs in a new tab. Native desktop
 * integration can replace this behavior at the application boundary.
 */
export const openExternal = async (url: string): Promise<void> => {
  window.open(url, "_blank", "noopener,noreferrer");
};
