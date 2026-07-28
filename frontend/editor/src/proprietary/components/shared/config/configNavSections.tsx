/**
 * Proprietary config nav: with login, teams, billing, and the server-side
 * admin settings API removed, the proprietary build exposes exactly the core
 * sections (Preferences + Help). Kept as a seam so desktop can extend it.
 */
export {
  useConfigNavSections,
  type ConfigNavSection,
  type ConfigNavItem,
  type ConfigColors,
} from "@core/components/shared/config/configNavSections";
