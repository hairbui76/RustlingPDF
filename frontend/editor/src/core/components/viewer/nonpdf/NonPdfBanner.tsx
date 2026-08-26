import { Button } from "@app/ui/Button";
import { LocalIcon } from "@app/components/shared/LocalIcon";
import { useTranslation } from "react-i18next";

interface NonPdfBannerProps {
  onConvertToPdf?: () => void;
}

export function NonPdfBanner({ onConvertToPdf }: NonPdfBannerProps) {
  const { t } = useTranslation();

  if (!onConvertToPdf) return null;

  return (
    <Button
      size="sm"
      variant="secondary"
      accent="warning"
      leftSection={
        <LocalIcon
          icon="picture-as-pdf-rounded"
          style={{ fontSize: "0.9rem" }}
        />
      }
      onClick={onConvertToPdf}
      style={{
        position: "absolute",
        top: 8,
        right: 8,
        zIndex: 10,
      }}
    >
      {t("viewer.nonPdf.convertToPdf")}
    </Button>
  );
}
