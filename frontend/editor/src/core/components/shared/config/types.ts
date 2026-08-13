import type React from "react";

// Single source of truth for all valid nav keys
export const VALID_NAV_KEYS = ["general", "hotkeys", "help"] as const;

// Derive the type from the array
export type NavKey = (typeof VALID_NAV_KEYS)[number];

// Nav structure of the settings modal. Lives here (not configNavSections) so
// consumers that only need the shape don't pull the whole section-component
// tree into their build's typecheck graph.
export interface ConfigNavItem {
  key: NavKey;
  label: string;
  icon: string;
  component: React.ReactNode;
  disabled?: boolean;
  disabledTooltip?: string;
  badge?: string;
  badgeColor?: string;
}

export interface ConfigNavSection {
  title: string;
  items: ConfigNavItem[];
}
