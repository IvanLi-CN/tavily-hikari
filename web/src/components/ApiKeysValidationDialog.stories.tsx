import React, { useEffect, useMemo, useRef, useState } from "react";
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

function openVisibleRegistrationBubbles(maxCount?: number): void {
  const triggers = Array.from(
    document.querySelectorAll<HTMLElement>("[data-registration-ip-trigger='true']"),
  ).filter((candidate) => candidate.getClientRects().length > 0);

  const visibleTriggers = triggers.slice(0, maxCount ?? triggers.length);
  for (const trigger of visibleTriggers) {
    trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    trigger.dispatchEvent(new MouseEvent("mouseenter", { bubbles: false }));
  }

  visibleTriggers[0]?.focus();
}

function staggerVisibleRegistrationBubbles(): void {
  const offsets = [
    { left: -56, top: 0 },
    { left: 0, top: 44 },
    { left: 64, top: 88 },
  ];
  const bubbles = Array.from(document.querySelectorAll<HTMLElement>(".key-validation-bubble")).filter(
    (candidate) => candidate.getClientRects().length > 0,
  );

  bubbles.forEach((bubble, index) => {
    const offset = offsets[index] ?? offsets[offsets.length - 1];
    bubble.style.marginLeft = `${offset.left}px`;
    bubble.style.marginTop = `${offset.top}px`;
  });
}

function RegistrationIpPreviewCanvas(): JSX.Element {
  const timeoutRef = useRef<number | null>(null);

  useEffect(() => {
    timeoutRef.current = window.setTimeout(() => {
      openVisibleRegistrationBubbles(1);
    }, 180);
    return () => {
      if (timeoutRef.current != null) window.clearTimeout(timeoutRef.current);
    };
  }, []);

  return (
    <ModalHarness
      initial={{
        group: "default",
        input_lines: 7,
        valid_lines: 7,
        unique_in_input: 5,
        duplicate_in_input: 2,
        checking: false,
        importing: false,
        rows: [
          {
            api_key: "tvly-OK-NEW",
            status: "ok",
            registration_ip: "8.8.8.8",
            registration_region: "US",
            assigned_proxy_key: "__direct__",
            assigned_proxy_label: "Direct",
            assigned_proxy_match_kind: "registration_ip",
            quota_limit: 1000,
            quota_remaining: 123,
            attempts: 1,
          },
          {
            api_key: "tvly-OK-EXHAUSTED",
            status: "ok_exhausted",
            registration_ip: "2606:4700:4700::1111",
            registration_region: "US Westfield (MA)",
            assigned_proxy_key: "http://1.1.1.1:8080",
            assigned_proxy_label: "Westfield Relay",
            assigned_proxy_match_kind: "same_region",
            quota_limit: 1000,
            quota_remaining: 0,
            attempts: 1,
          },
          {
            api_key: "tvly-UNAUTHORIZED",
            status: "unauthorized",
            detail: "Tavily usage request failed with 401 Unauthorized. This usually means the key is invalid or revoked.",
            attempts: 1,
          },
          {
            api_key: "tvly-ERROR",
            status: "error",
            detail: "Upstream returned 502 Bad Gateway.",
            attempts: 1,
          },
          {
            api_key: "tvly-DUPLICATE",
            status: "duplicate_in_input",
            registration_ip: "8.8.8.8",
            registration_region: "US",
            assigned_proxy_key: "http://9.9.9.9:8080",
            assigned_proxy_label: "Fallback Relay",
            assigned_proxy_match_kind: "other",
            attempts: 0,
          },
        ],
      }}
    />
  );
}

export const RegistrationIpPreview: Story = {
  args: {
    initial: {
      group: "default",
      input_lines: 0,
      valid_lines: 0,
      unique_in_input: 0,
      duplicate_in_input: 0,
      checking: false,
      importing: false,
      rows: [],
    },
  },
  render: () => <RegistrationIpPreviewCanvas />,
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};

export const AssignedProxyMatchKinds: Story = {
  args: {
    initial: {
      group: "default",
      input_lines: 3,
      valid_lines: 3,
      unique_in_input: 3,
      duplicate_in_input: 0,
      checking: false,
      importing: false,
      rows: [
        {
          api_key: "tvly-MATCH-IP",
          status: "ok",
          registration_ip: "8.8.8.8",
          registration_region: "US",
          assigned_proxy_key: "__direct__",
          assigned_proxy_label: "Direct",
          assigned_proxy_match_kind: "registration_ip",
          quota_limit: 1000,
          quota_remaining: 123,
          attempts: 1,
        },
        {
          api_key: "tvly-MATCH-REGION",
          status: "ok",
          registration_ip: "2606:4700:4700::1111",
          registration_region: "US Westfield (MA)",
          assigned_proxy_key: "http://1.1.1.1:8080",
          assigned_proxy_label: "Westfield Relay",
          assigned_proxy_match_kind: "same_region",
          quota_limit: 1000,
          quota_remaining: 456,
          attempts: 1,
        },
        {
          api_key: "tvly-MATCH-OTHER",
          status: "ok",
          registration_ip: "9.9.9.9",
          registration_region: "ZZ",
          assigned_proxy_key: "http://9.9.9.9:8080",
          assigned_proxy_label: "Fallback Relay",
          assigned_proxy_match_kind: "other",
          quota_limit: 1000,
          quota_remaining: 789,
          attempts: 1,
        },
      ],
    },
  },
  render: (args) => {
    function AssignedProxyMatchKindsCanvas(): JSX.Element {
      const timeoutRef = useRef<number | null>(null);

      useEffect(() => {
        timeoutRef.current = window.setTimeout(() => {
          openVisibleRegistrationBubbles();
          window.setTimeout(() => {
            staggerVisibleRegistrationBubbles();
          }, 40);
        }, 180);
        return () => {
          if (timeoutRef.current != null) window.clearTimeout(timeoutRef.current);
        };
      }, []);

      return <ModalHarness initial={args.initial} />;
    }

    return <AssignedProxyMatchKindsCanvas />;
  },
  name: "Assigned Proxy Match Kinds",
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
    docs: {
      description: {
        story:
          "Showcases the three assigned proxy match-color states inside the registration bubble: registration IP (success), same region (info), and fallback/other (warning).",
      },
    },
  },
};
