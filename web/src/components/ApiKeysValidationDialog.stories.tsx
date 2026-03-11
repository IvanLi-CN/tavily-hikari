import React, { useEffect, useMemo, useState } from "react";
import type { Meta, StoryObj } from "@storybook/react";

import {
  ApiKeysValidationDialog,
  computeExhaustedKeys,
  computeValidKeys,
  computeValidationCounts,
  type KeysValidationState,
} from "./ApiKeysValidationDialog";

function ModalHarness(props: { initial: KeysValidationState }): JSX.Element {
  const [open, setOpen] = useState(true);
  const [state, setState] = useState<KeysValidationState>(props.initial);

  useEffect(() => {
    setOpen(true);
    setState(props.initial);
  }, [props.initial]);

  const counts = useMemo(() => computeValidationCounts(state), [state]);
  const validKeys = useMemo(() => computeValidKeys(state), [state]);
  const exhaustedKeys = useMemo(() => computeExhaustedKeys(state), [state]);

  return (
    <ApiKeysValidationDialog
      open={open}
      state={state}
      counts={counts}
      validKeys={validKeys}
      exhaustedKeys={exhaustedKeys}
      onClose={() => {
        setOpen(false);
      }}
      onRetryFailed={() => {
        // Fake retry: convert failures to ok to showcase the UI.
        setState((prev) => ({
          ...prev,
          rows: prev.rows.map((r) =>
            r.status === "unauthorized" || r.status === "forbidden" || r.status === "invalid" || r.status === "error"
              ? { ...r, status: "ok", detail: undefined, attempts: r.attempts + 1, quota_limit: 1000, quota_remaining: 999 }
              : r,
          ),
        }));
      }}
      onRetryOne={(apiKey) => {
        setState((prev) => ({
          ...prev,
          rows: prev.rows.map((r) =>
            r.api_key === apiKey && (r.status === "unauthorized" || r.status === "forbidden" || r.status === "invalid" || r.status === "error")
              ? { ...r, status: "ok", detail: undefined, attempts: r.attempts + 1, quota_limit: 1000, quota_remaining: 888 }
              : r,
          ),
        }));
      }}
      onImportValid={() => {
        // Fake import report.
        setState((prev) => ({
          ...prev,
          importing: false,
          importReport: {
            summary: {
              input_lines: prev.input_lines,
              valid_lines: prev.valid_lines,
              unique_in_input: prev.unique_in_input,
              duplicate_in_input: prev.duplicate_in_input,
              created: 1,
              undeleted: 0,
              existed: 1,
              failed: 0,
            },
            results: [
              { api_key: "tvly-OK-NEW", status: "created" },
              { api_key: "tvly-OK-EXISTING", status: "existed" },
            ],
          },
        }));
      }}
    />
  );
}

const meta = {
  title: "Admin/Components/ApiKeysValidationDialog",
  component: ModalHarness,
  parameters: { layout: "fullscreen" },
  render: (args) => <ModalHarness {...args} />,
} satisfies Meta<typeof ModalHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

export const MixedResults: Story = {
  args: {
    initial: {
      group: "default",
      input_lines: 7,
      valid_lines: 6,
      unique_in_input: 5,
      duplicate_in_input: 1,
      checking: false,
      importing: false,
      rows: [
        { api_key: "tvly-OK-NEW", status: "ok", quota_limit: 1000, quota_remaining: 123, attempts: 1 },
        { api_key: "tvly-OK-EXHAUSTED", status: "ok_exhausted", quota_limit: 1000, quota_remaining: 0, attempts: 1 },
        {
          api_key: "tvly-UNAUTHORIZED",
          status: "unauthorized",
          detail: "Tavily usage request failed with 401 Unauthorized. This usually means the key is invalid or revoked.",
          attempts: 1,
        },
        {
          api_key: "tvly-ERROR",
          status: "error",
          detail:
            "Upstream returned 502 Bad Gateway. Click/hover the badge to see this message. On mobile, focus the badge to reveal it.",
          attempts: 1,
        },
        { api_key: "tvly-OK-NEW", status: "duplicate_in_input", attempts: 0 },
      ],
    },
  },
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};

export const CheckingInProgress: Story = {
  args: {
    initial: {
      group: "default",
      input_lines: 3,
      valid_lines: 3,
      unique_in_input: 3,
      duplicate_in_input: 0,
      checking: true,
      importing: false,
      rows: [
        { api_key: "tvly-PENDING-1", status: "pending", attempts: 0 },
        { api_key: "tvly-PENDING-2", status: "pending", attempts: 0 },
        { api_key: "tvly-PENDING-3", status: "pending", attempts: 0 },
      ],
    },
  },
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};

export const PostImportNoRemainingRows: Story = {
  args: {
    initial: {
      group: "default",
      input_lines: 4,
      valid_lines: 0,
      unique_in_input: 0,
      duplicate_in_input: 0,
      checking: false,
      importing: false,
      rows: [],
      importReport: {
        summary: {
          input_lines: 4,
          valid_lines: 4,
          unique_in_input: 4,
          duplicate_in_input: 0,
          created: 1,
          undeleted: 1,
          existed: 2,
          failed: 0,
        },
        results: [
          { api_key: "tvly-IMPORTED-1", status: "created" },
          { api_key: "tvly-IMPORTED-2", status: "undeleted" },
          { api_key: "tvly-IMPORTED-3", status: "existed" },
          { api_key: "tvly-IMPORTED-4", status: "existed" },
        ],
      },
    },
  },
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};

export const PostImportWithRemainingRows: Story = {
  args: {
    initial: {
      group: "default",
      input_lines: 5,
      valid_lines: 1,
      unique_in_input: 1,
      duplicate_in_input: 0,
      checking: false,
      importing: false,
      rows: [
        {
          api_key: "tvly-INVALID-REMAINING",
          status: "invalid",
          detail: "400 Bad Request",
          attempts: 1,
        },
      ],
      importReport: {
        summary: {
          input_lines: 5,
          valid_lines: 5,
          unique_in_input: 5,
          duplicate_in_input: 0,
          created: 2,
          undeleted: 1,
          existed: 1,
          failed: 1,
        },
        results: [
          { api_key: "tvly-IMPORTED-1", status: "created" },
          { api_key: "tvly-IMPORTED-2", status: "created" },
          { api_key: "tvly-IMPORTED-3", status: "undeleted" },
          { api_key: "tvly-IMPORTED-4", status: "existed" },
          { api_key: "tvly-INVALID-REMAINING", status: "failed", error: "400 Bad Request" },
        ],
      },
    },
  },
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};
