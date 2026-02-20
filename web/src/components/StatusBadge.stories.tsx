import type { Meta, StoryObj } from "@storybook/react";

import { StatusBadge } from "./StatusBadge";

const meta = {
  title: "Components/StatusBadge",
  component: StatusBadge,
  args: {
    tone: "success",
    children: "Success",
  },
} satisfies Meta<typeof StatusBadge>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Success: Story = { args: { tone: "success", children: "Success" } };
export const Warning: Story = { args: { tone: "warning", children: "Warning" } };
export const Error: Story = { args: { tone: "error", children: "Error" } };
export const Info: Story = { args: { tone: "info", children: "Info" } };
export const Neutral: Story = { args: { tone: "neutral", children: "Neutral" } };

