import type { Meta, StoryObj } from "@storybook/react";

import LanguageSwitcher from "./LanguageSwitcher";

const meta = {
  title: "Components/LanguageSwitcher",
  component: LanguageSwitcher,
  render: () => (
    <div style={{ padding: 24, display: "flex", justifyContent: "flex-end" }}>
      <LanguageSwitcher />
    </div>
  ),
} satisfies Meta<typeof LanguageSwitcher>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

