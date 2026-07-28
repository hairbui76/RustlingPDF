import { useState, useMemo } from "react";
import { Modal, Stack, Group } from "@mantine/core";
import { Button } from "@app/ui/Button";
import { ActionIcon } from "@app/ui/ActionIcon";
import { useTranslation } from "react-i18next";
import CloseIcon from "@mui/icons-material/Close";
import LocalIcon from "@app/components/shared/LocalIcon";
import AnimatedSlideBackground from "@app/components/onboarding/slides/AnimatedSlideBackground";
import WelcomeSlide from "@app/components/onboarding/slides/WelcomeSlide";
import { Z_INDEX_OVER_FULLSCREEN_SURFACE } from "@app/styles/zIndex";
import styles from "@app/components/onboarding/InitialOnboardingModal/InitialOnboardingModal.module.css";

const ONBOARDING_KEY = "stirling-desktop-onboarding-seen";

/**
 * Desktop-specific onboarding modal.
 * Shown on first launch: a single welcome slide. The app runs entirely
 * against the bundled local backend — there is no account to sign in to.
 */
export function DesktopOnboardingModal() {
  const { t } = useTranslation();
  const [visible, setVisible] = useState(
    () => !localStorage.getItem(ONBOARDING_KEY),
  );

  const dismiss = () => {
    localStorage.setItem(ONBOARDING_KEY, "true");
    setVisible(false);
  };

  // Call WelcomeSlide as a data factory (not a component render) — memoised so it
  // isn't reconstructed on every render while the modal is open.
  const welcomeSlide = useMemo(() => WelcomeSlide(), []);

  if (!visible) return null;

  return (
    <Modal
      opened={visible}
      onClose={dismiss}
      closeOnClickOutside
      centered
      size="lg"
      radius="lg"
      withCloseButton={false}
      zIndex={Z_INDEX_OVER_FULLSCREEN_SURFACE}
      styles={{
        body: { padding: 0 },
        content: {
          overflow: "hidden",
          border: "none",
          background: "var(--c-surface)",
          maxHeight: "90vh",
          display: "flex",
          flexDirection: "column",
        },
      }}
    >
      <Stack
        gap={0}
        className={styles.modalContent}
        style={{
          height: "100%",
          maxHeight: "90vh",
          display: "flex",
          flexDirection: "column",
        }}
      >
        {/* Hero section */}
        <div className={styles.heroWrapper} style={{ flexShrink: 0 }}>
          <AnimatedSlideBackground
            gradientStops={welcomeSlide.background.gradientStops}
            circles={welcomeSlide.background.circles}
            isActive
            slideKey="desktop-welcome"
          />
          <ActionIcon
            onClick={dismiss}
            variant="tertiary"
            size="md"
            aria-label={t("close", "Close")}
            style={{
              position: "absolute",
              top: 16,
              right: 16,
              backgroundColor: "rgba(255, 255, 255, 0.2)",
              color: "white",
              backdropFilter: "blur(4px)",
              zIndex: 10,
            }}
          >
            <CloseIcon fontSize="small" />
          </ActionIcon>
          <div className={styles.heroLogo}>
            <div className={styles.heroLogoCircle}>
              <LocalIcon
                icon="rocket-launch"
                width={64}
                height={64}
                className={styles.heroIcon}
              />
            </div>
          </div>
        </div>

        {/* Body section */}
        <div
          className={styles.modalBody}
          style={{ flex: 1, overflowY: "auto", overflowX: "hidden" }}
        >
          <Stack gap={16}>
            <div className={`${styles.title} ${styles.titleText}`}>
              {welcomeSlide.title}
            </div>
            <div className={styles.bodyText}>
              <div className={`${styles.bodyCopy} ${styles.bodyCopyInner}`}>
                {welcomeSlide.body}
              </div>
              <style>{`.${styles.bodyCopyInner} strong { color: var(--c-text); font-weight: 600; }`}</style>
            </div>
            <div className={styles.buttonContainer}>
              <Group justify="flex-end">
                <Button onClick={dismiss} accent="neutral">
                  {t("onboarding.buttons.getStarted", "Get started")}
                </Button>
              </Group>
            </div>
          </Stack>
        </div>
      </Stack>
    </Modal>
  );
}
