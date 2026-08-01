import React from "react";
// `@iconify/react/offline` deliberately, NOT `@iconify/react`. The default
// entrypoint silently fetches any icon missing from the bundled set from
// api.iconify.design, which would leak the user's IP, User-Agent and the icon
// name with no consent and no off switch. The offline build contains no API
// client at all, so a missing icon renders nothing and is caught in review
// instead of phoning home. `scripts/generate-icons.js` bundles every icon the
// codebase references, which is what makes this safe.
import { addCollection, Icon } from "@iconify/react/offline";
import iconSet from "../../../assets/material-symbols-icons.json"; // eslint-disable-line no-restricted-imports -- Outside app paths

// Load icons synchronously at import time - guaranteed to be ready on first render
try {
  if (iconSet) {
    addCollection(iconSet);
    const localIconCount = Object.keys(iconSet.icons || {}).length;
    console.info(
      `✅ Local icons loaded: ${localIconCount} icons (${Math.round(JSON.stringify(iconSet).length / 1024)}KB)`,
    );
  }
} catch {
  console.error(
    "Local icon set failed to load — icons will not render. Re-run `task frontend:prepare`.",
  );
}

interface LocalIconProps {
  icon: string;
  width?: string | number;
  height?: string | number;
  style?: React.CSSProperties;
  className?: string;
}

/**
 * LocalIcon component that uses our locally bundled Material Symbols icons
 * instead of loading from CDN
 */
export const LocalIcon: React.FC<LocalIconProps> = ({
  icon,
  width,
  height,
  style,
  ...props
}) => {
  // Convert our icon naming convention to the local collection format
  const iconName = icon.startsWith("material-symbols:")
    ? icon
    : `material-symbols:${icon}`;

  const iconStyle: React.CSSProperties = { ...style };

  // Use width if provided, otherwise fall back to height
  const size = width || height;
  if (size && typeof size === "string") {
    // If it's a CSS unit string (like '1.5rem'), use it as fontSize
    iconStyle.fontSize = size;
  } else if (typeof size === "number") {
    // If it's a number, treat it as pixels
    iconStyle.fontSize = `${size}px`;
  }

  // Renders from the bundled collection only; there is no network fallback.
  return <Icon icon={iconName} style={iconStyle} {...props} />;
};

export default LocalIcon;
