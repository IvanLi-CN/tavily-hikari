import type { Meta, StoryObj } from "@storybook/react";
import { useEffect, useState } from "react";

import RollingNumber from "./RollingNumber";

function RollingNumberDeltaDemo(props: { from: number; to: number }): JSX.Element {
  const [value, setValue] = useState<number>(props.from);

  useEffect(() => {
    const t = window.setTimeout(() => setValue(props.to), 600);
    return () => window.clearTimeout(t);
  }, [props.to]);

  return <RollingNumber value={value} />;
}

const meta = {
  title: "Components/RollingNumber",
  component: RollingNumber,
  args: {
    value: 123456,
    loading: false,
  },
} satisfies Meta<typeof RollingNumber>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
export const Loading: Story = { args: { loading: true } };
export const Empty: Story = { args: { value: null } };
export const Increase: Story = {
  render: () => <RollingNumberDeltaDemo from={123} to={4567} />,
};
export const Decrease: Story = {
  render: () => <RollingNumberDeltaDemo from={9876} to={54} />,
};
