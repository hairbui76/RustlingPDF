import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { SettingsShell, type SettingsNavSection } from "@app/ui/SettingsShell";
import { Button } from "@app/ui/Button";

const SECTIONS: SettingsNavSection[] = [
  {
    title: "Application",
    items: [
      { key: "general", label: "General" },
      { key: "appearance", label: "Appearance" },
      { key: "updates", label: "Updates" },
    ],
  },
  {
    title: "Information",
    items: [
      { key: "privacy", label: "Privacy" },
      { key: "licenses", label: "Licenses" },
      { key: "help", label: "Help", badge: "New" },
    ],
  },
];

const LABELS: Record<string, string> = {
  general: "General",
  appearance: "Appearance",
  updates: "Updates",
  privacy: "Privacy",
  licenses: "Licenses",
  help: "Help",
};

const meta: Meta<typeof SettingsShell> = {
  title: "Shared/SettingsShell",
  component: SettingsShell,
  parameters: { layout: "fullscreen" },
};
export default meta;

type Story = StoryObj<typeof SettingsShell>;

export const Default: Story = {
  render: () => {
    const [active, setActive] = useState("general");
    return (
      <div style={{ height: "36rem", border: "1px solid var(--c-border)" }}>
        <SettingsShell
          sections={SECTIONS}
          activeKey={active}
          onSelect={setActive}
          title={LABELS[active]}
          onClose={() => {}}
          footer={
            <>
              <Button variant="tertiary">Cancel</Button>
              <Button>Save changes</Button>
            </>
          }
        >
          <p style={{ color: "var(--c-text-subtle)" }}>
            Content for the “{LABELS[active]}” section renders here.
          </p>
        </SettingsShell>
      </div>
    );
  },
};
