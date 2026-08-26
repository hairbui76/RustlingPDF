import { useMemo } from "react";
import { materialSymbol } from "@app/components/shared/LocalIcon";
import { useNavigationState } from "@app/contexts/NavigationContext";
import { useToolNavigation } from "@app/hooks/useToolNavigation";
import { useToolWorkflow } from "@app/contexts/ToolWorkflowContext";
import { ToolId } from "@app/types/toolId";

const CompressIcon = materialSymbol("compress-rounded");
const SwapHorizIcon = materialSymbol("swap-horiz-rounded");
const CleaningServicesIcon = materialSymbol("cleaning-services-rounded");
const CropIcon = materialSymbol("crop-rounded");
const TextFieldsIcon = materialSymbol("text-fields-rounded");

// Material UI Icons

export interface SuggestedTool {
  id: ToolId;
  title: string;
  icon: React.ComponentType<any>;
  href: string;
  onClick: (e: React.MouseEvent) => void;
}

const ALL_SUGGESTED_TOOLS: Omit<SuggestedTool, "href" | "onClick">[] = [
  {
    id: "compress",
    title: "Compress",
    icon: CompressIcon,
  },
  {
    id: "convert",
    title: "Convert",
    icon: SwapHorizIcon,
  },
  {
    id: "sanitize",
    title: "Sanitize",
    icon: CleaningServicesIcon,
  },
  {
    id: "split",
    title: "Split",
    icon: CropIcon,
  },
  {
    id: "ocr",
    title: "OCR",
    icon: TextFieldsIcon,
  },
];

export function useSuggestedTools(): SuggestedTool[] {
  const { selectedTool } = useNavigationState();
  const { getToolNavigation } = useToolNavigation();
  const { getSelectedTool } = useToolWorkflow();

  return useMemo(() => {
    // Filter out the current tool
    const filteredTools = ALL_SUGGESTED_TOOLS.filter(
      (tool) => tool.id !== selectedTool,
    );

    // Add navigation props to each tool
    return filteredTools.map((tool) => {
      const toolRegistryEntry = getSelectedTool(tool.id);
      if (!toolRegistryEntry) {
        // Fallback for tools not in registry
        return {
          ...tool,
          href: `/${tool.id}`,
          onClick: (e: React.MouseEvent) => {
            e.preventDefault();
          },
        };
      }

      const navProps = getToolNavigation(tool.id, toolRegistryEntry);
      return {
        ...tool,
        ...navProps,
      };
    });
  }, [selectedTool, getToolNavigation, getSelectedTool]);
}
