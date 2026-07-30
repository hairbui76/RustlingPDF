import { Stack, Checkbox } from "@mantine/core";
import { useTranslation } from "react-i18next";
import { ConvertParameters } from "@app/hooks/tools/convert/useConvertParameters";

interface ConvertFromEbookSettingsProps {
  parameters: ConvertParameters;
  onParameterChange: <K extends keyof ConvertParameters>(
    key: K,
    value: ConvertParameters[K],
  ) => void;
  disabled?: boolean;
}

const ConvertFromEbookSettings = ({
  parameters,
  onParameterChange,
  disabled = false,
}: ConvertFromEbookSettingsProps) => {
  const { t } = useTranslation();

  const handleEmbedAllFontsChange = (value: boolean) => {
    onParameterChange("ebookOptions", {
      embedAllFonts: value,
      includeTableOfContents:
        parameters.ebookOptions?.includeTableOfContents ?? false,
      includePageNumbers: parameters.ebookOptions?.includePageNumbers ?? false,
    });
  };

  const handleIncludeTableOfContentsChange = (value: boolean) => {
    onParameterChange("ebookOptions", {
      embedAllFonts: parameters.ebookOptions?.embedAllFonts ?? false,
      includeTableOfContents: value,
      includePageNumbers: parameters.ebookOptions?.includePageNumbers ?? false,
    });
  };

  const handleIncludePageNumbersChange = (value: boolean) => {
    onParameterChange("ebookOptions", {
      embedAllFonts: parameters.ebookOptions?.embedAllFonts ?? false,
      includeTableOfContents:
        parameters.ebookOptions?.includeTableOfContents ?? false,
      includePageNumbers: value,
    });
  };

  // Initialize ebookOptions if not present
  const ebookOptions = parameters.ebookOptions || {
    embedAllFonts: false,
    includeTableOfContents: false,
    includePageNumbers: false,
  };

  return (
    <Stack gap="sm">
      <Checkbox
        label={t("convert.ebookOptions.embedAllFonts", "Embed all fonts")}
        description={t(
          "convert.ebookOptions.embedAllFontsDesc",
          "Embed all fonts from the eBook into the generated PDF",
        )}
        checked={ebookOptions.embedAllFonts}
        onChange={(event) =>
          handleEmbedAllFontsChange(event.currentTarget.checked)
        }
        disabled={disabled}
      />
      <Checkbox
        label={t(
          "convert.ebookOptions.includeTableOfContents",
          "Include table of contents",
        )}
        description={t(
          "convert.ebookOptions.includeTableOfContentsDesc",
          "Add a generated table of contents to the resulting PDF",
        )}
        checked={ebookOptions.includeTableOfContents}
        onChange={(event) =>
          handleIncludeTableOfContentsChange(event.currentTarget.checked)
        }
        disabled={disabled}
      />
      <Checkbox
        label={t(
          "convert.ebookOptions.includePageNumbers",
          "Include page numbers",
        )}
        description={t(
          "convert.ebookOptions.includePageNumbersDesc",
          "Add page numbers to the generated PDF",
        )}
        checked={ebookOptions.includePageNumbers}
        onChange={(event) =>
          handleIncludePageNumbersChange(event.currentTarget.checked)
        }
        disabled={disabled}
      />
    </Stack>
  );
};

export default ConvertFromEbookSettings;
