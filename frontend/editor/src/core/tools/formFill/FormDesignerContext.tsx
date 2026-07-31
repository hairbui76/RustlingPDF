import React, {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useReducer,
} from "react";
import type {
  DraftFormField,
  DraftWidget,
  FormFieldCreationRequest,
  FormFieldType,
  FormMode,
  WidgetCoordinates,
} from "@app/tools/formFill/types";

type Alignment = "left" | "top";

interface DesignerState {
  mode: FormMode;
  creationType: FormFieldType;
  fields: DraftFormField[];
  selectedIds: string[];
  appendWidget: boolean;
}

type DesignerAction =
  | { type: "SET_MODE"; mode: FormMode }
  | { type: "SET_CREATION_TYPE"; fieldType: FormFieldType }
  | { type: "SELECT"; id: string; additive: boolean }
  | { type: "SET_APPEND_WIDGET"; enabled: boolean }
  | {
      type: "ADD_WIDGET";
      widget: Omit<DraftWidget, "id">;
    }
  | {
      type: "UPDATE_FIELD";
      id: string;
      patch: Partial<Omit<DraftFormField, "id" | "widgets">>;
    }
  | {
      type: "UPDATE_WIDGET";
      fieldId: string;
      widgetId: string;
      patch: Partial<WidgetCoordinates>;
    }
  | { type: "DELETE_SELECTED" }
  | { type: "DUPLICATE_SELECTED" }
  | { type: "ALIGN_SELECTED"; alignment: Alignment }
  | { type: "RESET" };

const initialState: DesignerState = {
  mode: "fill",
  creationType: "text",
  fields: [],
  selectedIds: [],
  appendWidget: false,
};

let fallbackId = 0;

function newId(prefix: string): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return `${prefix}-${crypto.randomUUID()}`;
  }
  fallbackId += 1;
  return `${prefix}-${fallbackId}`;
}

function typeLabel(fieldType: FormFieldType): string {
  switch (fieldType) {
    case "combobox":
      return "Dropdown";
    case "listbox":
      return "List";
    case "checkbox":
      return "Checkbox";
    case "radio":
      return "Radio group";
    case "signature":
      return "Signature";
    case "button":
      return "Button";
    default:
      return "Text";
  }
}

function defaultOptions(fieldType: FormFieldType): string[] {
  if (fieldType === "checkbox") return ["Yes"];
  if (fieldType === "radio") return ["Option 1"];
  if (fieldType === "combobox" || fieldType === "listbox") {
    return ["Option 1", "Option 2"];
  }
  return [];
}

function fieldNumber(
  fields: DraftFormField[],
  fieldType: FormFieldType,
): number {
  return fields.filter((field) => field.type === fieldType).length + 1;
}

function newField(
  fields: DraftFormField[],
  fieldType: FormFieldType,
  widget: Omit<DraftWidget, "id">,
): DraftFormField {
  const number = fieldNumber(fields, fieldType);
  const name = `${fieldType}_${number}`;
  const options = defaultOptions(fieldType);
  return {
    id: newId("field"),
    name,
    type: fieldType,
    label: `${typeLabel(fieldType)} ${number}`,
    tooltip: "",
    required: false,
    readOnly: false,
    multiline: false,
    multiSelect: false,
    options,
    defaultValue: "",
    fontSize: 12,
    tabOrder: fields.length + 1,
    widgets: [
      {
        ...widget,
        id: newId("widget"),
        exportValue: fieldType === "radio" ? options[0] : widget.exportValue,
      },
    ],
  };
}

function reducer(state: DesignerState, action: DesignerAction): DesignerState {
  switch (action.type) {
    case "SET_MODE":
      return {
        ...state,
        mode: action.mode,
        appendWidget: false,
      };
    case "SET_CREATION_TYPE":
      return {
        ...state,
        creationType: action.fieldType,
        appendWidget: false,
      };
    case "SELECT": {
      const selectedIds = action.additive
        ? state.selectedIds.includes(action.id)
          ? state.selectedIds.filter((id) => id !== action.id)
          : [...state.selectedIds, action.id]
        : [action.id];
      return { ...state, selectedIds };
    }
    case "SET_APPEND_WIDGET":
      return { ...state, appendWidget: action.enabled };
    case "ADD_WIDGET": {
      const selected = state.fields.find(
        (field) => field.id === state.selectedIds.at(-1),
      );
      if (state.appendWidget && selected) {
        const optionNumber = selected.widgets.length + 1;
        const exportValue =
          selected.type === "radio"
            ? selected.options[optionNumber - 1] || `Option ${optionNumber}`
            : action.widget.exportValue;
        const options =
          selected.type === "radio" && selected.options.length < optionNumber
            ? [...selected.options, exportValue!]
            : selected.options;
        return {
          ...state,
          appendWidget: false,
          fields: state.fields.map((field) =>
            field.id === selected.id
              ? {
                  ...field,
                  options,
                  widgets: [
                    ...field.widgets,
                    {
                      ...action.widget,
                      id: newId("widget"),
                      exportValue,
                    },
                  ],
                }
              : field,
          ),
        };
      }
      const field = newField(state.fields, state.creationType, action.widget);
      return {
        ...state,
        fields: [...state.fields, field],
        selectedIds: [field.id],
        appendWidget: false,
      };
    }
    case "UPDATE_FIELD": {
      return {
        ...state,
        fields: state.fields.map((field) => {
          if (field.id !== action.id) return field;
          const updated = { ...field, ...action.patch };
          if (action.patch.options && updated.type === "radio") {
            updated.widgets = updated.widgets.map((widget, index) => ({
              ...widget,
              exportValue:
                action.patch.options?.[index] ||
                widget.exportValue ||
                `Option ${index + 1}`,
            }));
          }
          return updated;
        }),
      };
    }
    case "UPDATE_WIDGET":
      return {
        ...state,
        fields: state.fields.map((field) =>
          field.id === action.fieldId
            ? {
                ...field,
                widgets: field.widgets.map((widget) =>
                  widget.id === action.widgetId
                    ? { ...widget, ...action.patch }
                    : widget,
                ),
              }
            : field,
        ),
      };
    case "DELETE_SELECTED":
      return {
        ...state,
        fields: state.fields.filter(
          (field) => !state.selectedIds.includes(field.id),
        ),
        selectedIds: [],
        appendWidget: false,
      };
    case "DUPLICATE_SELECTED": {
      const copies = state.fields
        .filter((field) => state.selectedIds.includes(field.id))
        .map((field) => ({
          ...field,
          id: newId("field"),
          name: `${field.name}_copy`,
          label: `${field.label} copy`,
          tabOrder:
            field.tabOrder === undefined ? undefined : field.tabOrder + 1,
          widgets: field.widgets.map((widget) => ({
            ...widget,
            id: newId("widget"),
          })),
        }));
      return {
        ...state,
        fields: [...state.fields, ...copies],
        selectedIds: copies.map((field) => field.id),
      };
    }
    case "ALIGN_SELECTED": {
      const selected = state.fields.filter((field) =>
        state.selectedIds.includes(field.id),
      );
      if (selected.length < 2) return state;
      const targets = new Map<number, number>();
      for (const field of selected) {
        for (const widget of field.widgets) {
          const value = action.alignment === "left" ? widget.x : widget.y;
          targets.set(
            widget.pageIndex,
            Math.min(targets.get(widget.pageIndex) ?? value, value),
          );
        }
      }
      return {
        ...state,
        fields: state.fields.map((field) =>
          !state.selectedIds.includes(field.id)
            ? field
            : {
                ...field,
                widgets: field.widgets.map((widget) => ({
                  ...widget,
                  [action.alignment === "left" ? "x" : "y"]:
                    targets.get(widget.pageIndex) ??
                    (action.alignment === "left" ? widget.x : widget.y),
                })),
              },
        ),
      };
    }
    case "RESET":
      return { ...initialState, mode: state.mode };
    default:
      return state;
  }
}

export interface FormDesignerContextValue extends DesignerState {
  setMode: (mode: FormMode) => void;
  setCreationType: (fieldType: FormFieldType) => void;
  selectField: (id: string, additive?: boolean) => void;
  setAppendWidget: (enabled: boolean) => void;
  addWidget: (widget: Omit<DraftWidget, "id">) => void;
  updateField: (
    id: string,
    patch: Partial<Omit<DraftFormField, "id" | "widgets">>,
  ) => void;
  updateWidget: (
    fieldId: string,
    widgetId: string,
    patch: Partial<WidgetCoordinates>,
  ) => void;
  deleteSelected: () => void;
  duplicateSelected: () => void;
  alignSelected: (alignment: Alignment) => void;
  reset: () => void;
  selectedField: DraftFormField | null;
  creationRequests: FormFieldCreationRequest[];
}

const FormDesignerContext = createContext<FormDesignerContextValue | null>(
  null,
);

export function FormDesignerProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const [state, dispatch] = useReducer(reducer, initialState);
  const setMode = useCallback(
    (mode: FormMode) => dispatch({ type: "SET_MODE", mode }),
    [],
  );
  const setCreationType = useCallback(
    (fieldType: FormFieldType) =>
      dispatch({ type: "SET_CREATION_TYPE", fieldType }),
    [],
  );
  const selectField = useCallback(
    (id: string, additive = false) =>
      dispatch({ type: "SELECT", id, additive }),
    [],
  );
  const setAppendWidget = useCallback(
    (enabled: boolean) => dispatch({ type: "SET_APPEND_WIDGET", enabled }),
    [],
  );
  const addWidget = useCallback(
    (widget: Omit<DraftWidget, "id">) =>
      dispatch({ type: "ADD_WIDGET", widget }),
    [],
  );
  const updateField = useCallback(
    (id: string, patch: Partial<Omit<DraftFormField, "id" | "widgets">>) =>
      dispatch({ type: "UPDATE_FIELD", id, patch }),
    [],
  );
  const updateWidget = useCallback(
    (fieldId: string, widgetId: string, patch: Partial<WidgetCoordinates>) =>
      dispatch({ type: "UPDATE_WIDGET", fieldId, widgetId, patch }),
    [],
  );
  const deleteSelected = useCallback(
    () => dispatch({ type: "DELETE_SELECTED" }),
    [],
  );
  const duplicateSelected = useCallback(
    () => dispatch({ type: "DUPLICATE_SELECTED" }),
    [],
  );
  const alignSelected = useCallback(
    (alignment: Alignment) => dispatch({ type: "ALIGN_SELECTED", alignment }),
    [],
  );
  const reset = useCallback(() => dispatch({ type: "RESET" }), []);

  const selectedField =
    state.fields.find((field) => field.id === state.selectedIds.at(-1)) ?? null;
  const creationRequests = useMemo<FormFieldCreationRequest[]>(
    () =>
      state.fields.map(({ id: _id, widgets, ...field }) => ({
        ...field,
        label: field.label || undefined,
        tooltip: field.tooltip || undefined,
        defaultValue: field.defaultValue || undefined,
        options: field.options.length > 0 ? field.options : undefined,
        widgets: widgets.map(({ id: _widgetId, ...widget }) => widget),
      })),
    [state.fields],
  );

  const value = useMemo<FormDesignerContextValue>(
    () => ({
      ...state,
      setMode,
      setCreationType,
      selectField,
      setAppendWidget,
      addWidget,
      updateField,
      updateWidget,
      deleteSelected,
      duplicateSelected,
      alignSelected,
      reset,
      selectedField,
      creationRequests,
    }),
    [
      state,
      setMode,
      setCreationType,
      selectField,
      setAppendWidget,
      addWidget,
      updateField,
      updateWidget,
      deleteSelected,
      duplicateSelected,
      alignSelected,
      reset,
      selectedField,
      creationRequests,
    ],
  );

  return (
    <FormDesignerContext.Provider value={value}>
      {children}
    </FormDesignerContext.Provider>
  );
}

export function useFormDesigner(): FormDesignerContextValue {
  const context = useContext(FormDesignerContext);
  if (!context) {
    throw new Error(
      "useFormDesigner must be used within a FormDesignerProvider",
    );
  }
  return context;
}

export default FormDesignerContext;
