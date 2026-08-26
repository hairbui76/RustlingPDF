/**
 * Shared field type metadata: icons and color mappings.
 * Used by FormFill, FormFieldSidebar, and any future form tools.
 */
import React from "react";
import { LocalIcon } from "@app/components/shared/LocalIcon";
import type { FormFieldType } from "@app/tools/formFill/types";

export const FIELD_TYPE_ICON: Record<FormFieldType, React.ReactNode> = {
  text: (
    <LocalIcon icon="text-fields-rounded" width="inherit" height="inherit" />
  ),
  checkbox: (
    <LocalIcon icon="check-box-rounded" width="inherit" height="inherit" />
  ),
  combobox: (
    <LocalIcon
      icon="arrow-drop-down-circle-rounded"
      width="inherit"
      height="inherit"
    />
  ),
  listbox: <LocalIcon icon="list-rounded" width="inherit" height="inherit" />,
  radio: (
    <LocalIcon icon="radio-button-checked" width="inherit" height="inherit" />
  ),
  button: <LocalIcon icon="draw-rounded" width="inherit" height="inherit" />,
  signature: <LocalIcon icon="draw-rounded" width="inherit" height="inherit" />,
};

export const FIELD_TYPE_COLOR: Record<FormFieldType, string> = {
  text: "blue",
  checkbox: "green",
  combobox: "violet",
  listbox: "cyan",
  radio: "orange",
  button: "gray",
  signature: "pink",
};
