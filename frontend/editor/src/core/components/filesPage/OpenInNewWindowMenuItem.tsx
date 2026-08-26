import { Menu } from "@mantine/core";
import { LocalIcon } from "@app/components/shared/LocalIcon";
import { useTranslation } from "react-i18next";
import { RustlingFileStub } from "@app/types/fileContext";
import { useOpenInNewWindow } from "@app/extensions/openInNewWindow";

interface OpenInNewWindowMenuItemProps {
  file: RustlingFileStub;
}

/**
 * Kebab menu item that opens a stored file in a separate window. Desktop-only:
 * the underlying extension is a no-op on web, so this renders nothing there
 * (and for any file that can't be opened in a new window).
 */
export function OpenInNewWindowMenuItem({
  file,
}: OpenInNewWindowMenuItemProps) {
  const { t } = useTranslation();
  const { canOpenInNewWindow, openInNewWindow } = useOpenInNewWindow();

  if (!canOpenInNewWindow(file)) return null;

  return (
    <Menu.Item
      leftSection={
        <LocalIcon
          icon="open-in-new-rounded"
          width="1.25rem"
          height="1.25rem"
        />
      }
      onClick={(e) => {
        e.stopPropagation();
        openInNewWindow(file);
      }}
    >
      {t("openInNewWindow", "Open in new window")}
    </Menu.Item>
  );
}
