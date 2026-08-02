import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import {
  RECOMMENDED_GROUP_ID,
  type ToolGroupId,
} from "@app/hooks/tools/useToolGroups";

interface ToolGroupContextValue {
  /** The group whose tools the right-hand panel is listing. */
  selectedGroup: ToolGroupId;
  setSelectedGroup: (id: ToolGroupId) => void;
}

const ToolGroupContext = createContext<ToolGroupContextValue | null>(null);

/**
 * Which top-level tool group is showing.
 *
 * Shared between the bottom group bar (which lives in the workbench column)
 * and the right-hand tool panel (which lists that group's tools), so it can't
 * live in either of them.
 */
export function ToolGroupProvider({ children }: { children: ReactNode }) {
  const [selectedGroup, setSelectedGroupState] =
    useState<ToolGroupId>(RECOMMENDED_GROUP_ID);

  const setSelectedGroup = useCallback((id: ToolGroupId) => {
    setSelectedGroupState(id);
  }, []);

  const value = useMemo<ToolGroupContextValue>(
    () => ({ selectedGroup, setSelectedGroup }),
    [selectedGroup, setSelectedGroup],
  );

  return (
    <ToolGroupContext.Provider value={value}>
      {children}
    </ToolGroupContext.Provider>
  );
}

export function useToolGroupSelection(): ToolGroupContextValue {
  const ctx = useContext(ToolGroupContext);
  if (!ctx) {
    throw new Error(
      "useToolGroupSelection must be used within a ToolGroupProvider",
    );
  }
  return ctx;
}
