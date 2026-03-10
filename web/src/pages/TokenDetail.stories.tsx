import { useLayoutEffect, useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { addons } from "storybook/preview-api";
import { SELECT_STORY } from "storybook/internal/core-events";

import TokenDetail from "./TokenDetail";

const tokenId = "a1b2";

interface StoryTokenDetail {
  id: string;
  enabled: boolean;
  note: string | null;
  owner: { userId: string; displayName: string; username: string } | null;
  total_requests: number;
  created_at: number;
  last_used_at: number;
  quota_state: string;
  quota_hourly_used: number;
  quota_hourly_limit: number;
  quota_daily_used: number;
  quota_daily_limit: number;
  quota_monthly_used: number;
  quota_monthly_limit: number;
  quota_hourly_reset_at: number;
  quota_daily_reset_at: number;
  quota_monthly_reset_at: number;
}

const tokenDetailMock: StoryTokenDetail = {
  id: tokenId,
  enabled: true,
  note: "primary token",
  owner: { userId: "usr_alice", displayName: "Alice Chen", username: "alice" },
  total_requests: 9241,
  created_at: 1_762_100_200,
  last_used_at: 1_762_390_010,
  quota_state: "normal",
  quota_hourly_used: 33,
  quota_hourly_limit: 100,
  quota_daily_used: 192,
  quota_daily_limit: 500,
  quota_monthly_used: 2511,
  quota_monthly_limit: 5000,
  quota_hourly_reset_at: 1_762_393_600,
  quota_daily_reset_at: 1_762_444_400,
  quota_monthly_reset_at: 1_764_806_400,
};

const tokenDetailUnboundMock: StoryTokenDetail = {
  ...tokenDetailMock,
  id: "z9x8",
  owner: null,
  note: "unassigned sandbox token",
};

const metricsMock = {
  total_requests: 9241,
  success_count: 9003,
  error_count: 175,
  quota_exhausted_count: 63,
  last_activity: 1_762_390_010,
};

const logsMock = Array.from({ length: 8 }, (_, idx) => ({
  id: 3000 + idx,
  method: "POST",
  path: "/mcp",
  query: null,
  http_status: idx % 4 === 0 ? 429 : 200,
  mcp_status: idx % 4 === 0 ? -1 : 0,
  business_credits: idx % 3 === 0 ? null : idx + 1,
  result_status: idx % 4 === 0 ? "quota_exhausted" : "success",
  error_message: idx % 4 === 0 ? "quota exhausted" : null,
  created_at: 1_762_390_010 - idx * 420,
}));

const usageSeriesMock = Array.from({ length: 16 }, (_, idx) => ({
  bucket_start: 1_762_360_000 + idx * 3600,
  success_count: 10 + idx,
  system_failure_count: idx % 3,
  external_failure_count: idx % 2,
}));

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function installFetchMock(detailOverride = tokenDetailMock): () => void {
  const originalFetch = window.fetch.bind(window);
  const activeTokenId = detailOverride.id;

  window.fetch = async (
    input: RequestInfo | URL,
    init?: RequestInit,
  ): Promise<Response> => {
    const request = input instanceof Request ? input : new Request(input, init);
    const url = new URL(request.url, window.location.origin);

    if (url.pathname === `/api/tokens/${activeTokenId}`) {
      return jsonResponse(detailOverride);
    }

    if (url.pathname === `/api/tokens/${activeTokenId}/metrics`) {
      return jsonResponse(metricsMock);
    }

    if (url.pathname === `/api/tokens/${activeTokenId}/logs/page`) {
      const perPage = Number(url.searchParams.get("per_page") ?? "20");
      const page = Number(url.searchParams.get("page") ?? "1");
      return jsonResponse({
        items: logsMock.slice(0, perPage),
        page,
        per_page: perPage,
        total: logsMock.length,
      });
    }

    if (url.pathname === `/api/tokens/${activeTokenId}/metrics/usage-series`) {
      return jsonResponse(usageSeriesMock);
    }

    if (url.pathname === `/api/tokens/${activeTokenId}/secret/rotate`) {
      return jsonResponse({ token: "th-a1b2-1234567890abcdef" });
    }

    return originalFetch(input, init);
  };

  return () => {
    window.fetch = originalFetch;
  };
}

function installEventSourceMock(): () => void {
  const OriginalEventSource = window.EventSource;

  class MockEventSource {
    static CONNECTING = 0;
    static OPEN = 1;
    static CLOSED = 2;

    public readonly url: string;
    public readonly withCredentials = false;
    public readyState = MockEventSource.OPEN;
    public onopen: ((this: EventSource, ev: Event) => unknown) | null = null;
    public onerror: ((this: EventSource, ev: Event) => unknown) | null = null;
    public onmessage:
      | ((this: EventSource, ev: MessageEvent) => unknown)
      | null = null;

    private listeners = new Map<
      string,
      Set<EventListenerOrEventListenerObject>
    >();

    constructor(url: string) {
      this.url = url;
      window.setTimeout(() => {
        this.onopen?.call(this as unknown as EventSource, new Event("open"));
      }, 0);
    }

    addEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject,
    ): void {
      if (!this.listeners.has(type)) {
        this.listeners.set(type, new Set());
      }
      this.listeners.get(type)?.add(listener);
    }

    removeEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject,
    ): void {
      this.listeners.get(type)?.delete(listener);
    }

    dispatchEvent(event: Event): boolean {
      const bucket = this.listeners.get(event.type);
      if (!bucket) return true;
      bucket.forEach((listener) => {
        if (typeof listener === "function") {
          listener.call(this, event);
        } else {
          listener.handleEvent(event);
        }
      });
      return true;
    }

    close(): void {
      this.readyState = MockEventSource.CLOSED;
    }
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (window as any).EventSource = MockEventSource;

  return () => {
    window.EventSource = OriginalEventSource;
  };
}

function openStoryInManager(storyId: string): void {
  addons.getChannel().emit(SELECT_STORY, { storyId });
}

function TokenDetailStoryCanvas({
  detail = tokenDetailMock,
}: {
  detail?: StoryTokenDetail;
}): JSX.Element {
  const [ready, setReady] = useState(false);

  useLayoutEffect(() => {
    const cleanupFetch = installFetchMock(detail);
    const cleanupEventSource = installEventSourceMock();
    setReady(true);

    return () => {
      cleanupFetch();
      cleanupEventSource();
      setReady(false);
    };
  }, [detail]);

  if (!ready) {
    return <div style={{ minHeight: "100vh" }} />;
  }

  return (
    <TokenDetail
      id={detail.id}
      onBack={() => undefined}
      onOpenUser={(userId) => {
        if (!userId) return;
        openStoryInManager('admin-pages--user-detail');
      }}
    />
  );
}

const meta = {
  title: "Admin/Pages/TokenDetail",
  parameters: {
    layout: "fullscreen",
  },
  render: (args) => <TokenDetailStoryCanvas {...args} />,
} satisfies Meta<typeof TokenDetailStoryCanvas>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: { detail: tokenDetailMock },
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};

export const Unbound: Story = {
  args: { detail: tokenDetailUnboundMock },
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};
