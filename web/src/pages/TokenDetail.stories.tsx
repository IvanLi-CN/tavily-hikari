import { useLayoutEffect, useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { addons } from "storybook/preview-api";
import { SELECT_STORY } from "storybook/internal/core-events";

import TokenDetail from "./TokenDetail";

const tokenId = "a1b2";
type StoryMode = "default" | "initial_loading" | "switch_loading" | "refreshing";
type StoryDataset = "default" | "dense";

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

const tokenDetailDenseMock: StoryTokenDetail = {
  ...tokenDetailMock,
  id: "hv9k",
  note: "high-volume analytics token",
  total_requests: 182_411,
  last_used_at: 1_762_390_250,
  quota_hourly_used: 87,
  quota_daily_used: 412,
  quota_monthly_used: 4_812,
};

const metricsMock = {
  total_requests: 9241,
  success_count: 9003,
  error_count: 175,
  quota_exhausted_count: 63,
  last_activity: 1_762_390_010,
};

const denseMetricsMock = {
  total_requests: 12_840,
  success_count: 12_101,
  error_count: 511,
  quota_exhausted_count: 228,
  last_activity: 1_762_390_250,
};

const requestKindOptionsMock = [
  { key: "api:crawl", label: "API | crawl", protocol_group: "api", billing_group: "billable" },
  { key: "api:extract", label: "API | extract", protocol_group: "api", billing_group: "billable" },
  { key: "api:map", label: "API | map", protocol_group: "api", billing_group: "billable" },
  { key: "api:research", label: "API | research", protocol_group: "api", billing_group: "billable" },
  { key: "api:research-result", label: "API | research result", protocol_group: "api", billing_group: "non_billable" },
  { key: "api:search", label: "API | search", protocol_group: "api", billing_group: "billable" },
  { key: "api:unknown-path", label: "API | unknown path", protocol_group: "api", billing_group: "non_billable" },
  { key: "mcp:extract", label: "MCP | extract", protocol_group: "mcp", billing_group: "billable" },
  { key: "mcp:initialize", label: "MCP | initialize", protocol_group: "mcp", billing_group: "non_billable" },
  { key: "mcp:map", label: "MCP | map", protocol_group: "mcp", billing_group: "billable" },
  {
    key: "mcp:notifications/initialized",
    label: "MCP | notifications/initialized",
    protocol_group: "mcp",
    billing_group: "non_billable",
  },
  { key: "mcp:ping", label: "MCP | ping", protocol_group: "mcp", billing_group: "non_billable" },
  { key: "mcp:research", label: "MCP | research", protocol_group: "mcp", billing_group: "billable" },
  { key: "mcp:resources/list", label: "MCP | resources/list", protocol_group: "mcp", billing_group: "non_billable" },
  {
    key: "mcp:resources/templates/list",
    label: "MCP | resources/templates/list",
    protocol_group: "mcp",
    billing_group: "non_billable",
  },
  { key: "mcp:search", label: "MCP | search", protocol_group: "mcp", billing_group: "billable" },
  { key: "mcp:third-party-tool", label: "MCP | third-party tool", protocol_group: "mcp", billing_group: "non_billable" },
  { key: "mcp:tools/list", label: "MCP | tools/list", protocol_group: "mcp", billing_group: "non_billable" },
  { key: "mcp:unknown-method", label: "MCP | unknown method", protocol_group: "mcp", billing_group: "non_billable" },
  { key: "mcp:unknown-payload", label: "MCP | unknown payload", protocol_group: "mcp", billing_group: "non_billable" },
  { key: "mcp:unsupported-path", label: "MCP | unsupported path", protocol_group: "mcp", billing_group: "non_billable" },
];

const storyKeyIds = ["MZli", "Qn8R", "U2vK"] as const;

const logTemplates = [
  {
    method: "POST",
    path: "/api/tavily/crawl",
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: 4,
    request_kind_key: "api:crawl",
    request_kind_label: "API | crawl",
    request_kind_detail: null,
    result_status: "success",
    key_effect_code: "none",
    key_effect_summary: null,
    error_message: null,
  },
  {
    method: "POST",
    path: "/api/tavily/extract",
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: 3,
    request_kind_key: "api:extract",
    request_kind_label: "API | extract",
    request_kind_detail: null,
    result_status: "success",
    key_effect_code: "none",
    key_effect_summary: null,
    error_message: null,
  },
  {
    method: "POST",
    path: "/api/tavily/map",
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: 2,
    request_kind_key: "api:map",
    request_kind_label: "API | map",
    request_kind_detail: null,
    result_status: "success",
    key_effect_code: "none",
    key_effect_summary: null,
    error_message: null,
  },
  {
    method: "POST",
    path: "/api/tavily/research",
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: 5,
    request_kind_key: "api:research",
    request_kind_label: "API | research",
    request_kind_detail: null,
    result_status: "success",
    key_effect_code: "none",
    key_effect_summary: null,
    error_message: null,
  },
  {
    method: "GET",
    path: "/api/tavily/research/req_42",
    query: null,
    http_status: 404,
    mcp_status: 404,
    business_credits: null,
    request_kind_key: "api:research-result",
    request_kind_label: "API | research result",
    request_kind_detail: null,
    result_status: "error",
    key_effect_code: "none",
    key_effect_summary: null,
    error_message: "research request not found",
  },
  {
    method: "POST",
    path: "/api/tavily/search",
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: 2,
    request_kind_key: "api:search",
    request_kind_label: "API | search",
    request_kind_detail: null,
    result_status: "success",
    key_effect_code: "none",
    key_effect_summary: null,
    error_message: null,
  },
  {
    method: "POST",
    path: "/mcp",
    query: null,
    http_status: 429,
    mcp_status: -1,
    business_credits: null,
    request_kind_key: "mcp:unknown-payload",
    request_kind_label: "MCP | unknown payload",
    request_kind_detail: "tool: crawl",
    result_status: "quota_exhausted",
    key_effect_code: "marked_exhausted",
    key_effect_summary: "Automatically marked this key as exhausted",
    error_message: "quota exhausted",
  },
  {
    method: "POST",
    path: "/mcp",
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: 2,
    request_kind_key: "mcp:extract",
    request_kind_label: "MCP | extract",
    request_kind_detail: null,
    result_status: "success",
    key_effect_code: "restored_active",
    key_effect_summary: "The system automatically restored this key to active",
    error_message: null,
  },
  {
    method: "POST",
    path: "/mcp",
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: null,
    request_kind_key: "mcp:initialize",
    request_kind_label: "MCP | initialize",
    request_kind_detail: null,
    result_status: "success",
    key_effect_code: "none",
    key_effect_summary: null,
    error_message: null,
  },
  {
    method: "POST",
    path: "/mcp",
    query: null,
    http_status: 202,
    mcp_status: null,
    business_credits: null,
    request_kind_key: "mcp:notifications/initialized",
    request_kind_label: "MCP | notifications/initialized",
    request_kind_detail: null,
    result_status: "unknown",
    key_effect_code: "none",
    key_effect_summary: null,
    error_message: null,
  },
  {
    method: "POST",
    path: "/mcp",
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: null,
    request_kind_key: "mcp:ping",
    request_kind_label: "MCP | ping",
    request_kind_detail: null,
    result_status: "success",
    key_effect_code: "none",
    key_effect_summary: null,
    error_message: null,
  },
  {
    method: "POST",
    path: "/mcp",
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: null,
    request_kind_key: "mcp:resources/list",
    request_kind_label: "MCP | resources/list",
    request_kind_detail: null,
    result_status: "success",
    key_effect_code: "none",
    key_effect_summary: null,
    error_message: null,
  },
  {
    method: "POST",
    path: "/mcp",
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: null,
    request_kind_key: "mcp:resources/templates/list",
    request_kind_label: "MCP | resources/templates/list",
    request_kind_detail: null,
    result_status: "success",
    key_effect_code: "none",
    key_effect_summary: null,
    error_message: null,
  },
  {
    method: "POST",
    path: "/mcp",
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: 2,
    request_kind_key: "mcp:search",
    request_kind_label: "MCP | search",
    request_kind_detail: null,
    result_status: "success",
    key_effect_code: "quarantined",
    key_effect_summary: "The system automatically quarantined this key",
    error_message: null,
  },
  {
    method: "POST",
    path: "/mcp",
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: null,
    request_kind_key: "mcp:tools/list",
    request_kind_label: "MCP | tools/list",
    request_kind_detail: null,
    result_status: "success",
    key_effect_code: "none",
    key_effect_summary: null,
    error_message: null,
  },
] as const;

function requestKindProtocolGroup(key: string): 'api' | 'mcp' {
  return key.startsWith('mcp:') ? 'mcp' : 'api'
}

function requestKindBillingGroup(key: string): 'billable' | 'non_billable' {
  if (
    key === 'api:research-result' ||
    key === 'api:unknown-path' ||
    key === 'api:usage' ||
    key.startsWith('mcp:initialize') ||
    key.startsWith('mcp:ping') ||
    key.startsWith('mcp:tools/list') ||
    key === 'mcp:unsupported-path' ||
    key === 'mcp:unknown-payload' ||
    key === 'mcp:unknown-method' ||
    key === 'mcp:third-party-tool' ||
    key.startsWith('mcp:resources/') ||
    key.startsWith('mcp:prompts/') ||
    key.startsWith('mcp:notifications/')
  ) {
    return 'non_billable'
  }
  return 'billable'
}

function requestOperationalClass(
  resultStatus: string,
  requestKindKey: string,
): 'success' | 'neutral' | 'quota_exhausted' {
  if (resultStatus === 'quota_exhausted') return 'quota_exhausted'
  if (
    requestKindProtocolGroup(requestKindKey) === 'mcp' &&
    requestKindBillingGroup(requestKindKey) === 'non_billable'
  ) {
    return 'neutral'
  }
  return 'success'
}

function buildLogsMock(
  count: number,
  startId: number,
  createdAtStart: number,
  createdAtStep: number,
) {
  return Array.from({ length: count }, (_, idx) => {
    const template = logTemplates[idx % logTemplates.length];
    return {
      id: startId + idx,
      key_id: storyKeyIds[idx % storyKeyIds.length],
      auth_token_id: tokenId,
      ...template,
      business_credits:
        template.business_credits == null ? null : template.business_credits + Math.floor(idx / logTemplates.length),
      created_at: createdAtStart - idx * createdAtStep,
      operationalClass: requestOperationalClass(template.result_status, template.request_kind_key),
      requestKindProtocolGroup: requestKindProtocolGroup(template.request_kind_key),
      requestKindBillingGroup: requestKindBillingGroup(template.request_kind_key),
    };
  });
}

const logsMock = buildLogsMock(24, 3000, 1_762_390_010, 420);

const logsPageTwoMock = buildLogsMock(24, 4000, 1_762_380_010, 420);

const denseLogsMock = buildLogsMock(96, 5000, 1_762_390_250, 75);

const denseLogsPageTwoMock = buildLogsMock(96, 6000, 1_762_383_050, 75);

const usageSeriesMock = Array.from({ length: 16 }, (_, idx) => ({
  bucket_start: 1_762_360_000 + idx * 3600,
  success_count: 10 + idx,
  system_failure_count: idx % 3,
  external_failure_count: idx % 2,
}));

const denseUsageSeriesMock = Array.from({ length: 16 }, (_, idx) => ({
  bucket_start: 1_762_360_000 + idx * 3600,
  success_count: 38 + idx * 6,
  system_failure_count: idx % 4,
  external_failure_count: idx % 3,
}));

interface StoryDatasetConfig {
  metrics: typeof metricsMock;
  logs: typeof logsMock;
  logsPageTwo: typeof logsMock;
  usageSeries: typeof usageSeriesMock;
  initialPerPage?: number;
}

const storyDatasets: Record<StoryDataset, StoryDatasetConfig> = {
  default: {
    metrics: metricsMock,
    logs: logsMock,
    logsPageTwo: logsPageTwoMock,
    usageSeries: usageSeriesMock,
  },
  dense: {
    metrics: denseMetricsMock,
    logs: denseLogsMock,
    logsPageTwo: denseLogsPageTwoMock,
    usageSeries: denseUsageSeriesMock,
    initialPerPage: 50,
  },
};

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

type MockEventSourceShape = EventSource & {
  dispatchEvent(event: Event): boolean;
};

const activeEventSources = new Set<MockEventSourceShape>();

function buildLogPage(
  source: typeof logsMock,
  page: number,
  requestedPerPage: number,
  responsePerPage = requestedPerPage,
): {
  items: typeof logsMock;
  page: number;
  per_page: number;
  total: number;
  request_kind_options: typeof requestKindOptionsMock;
  facets: {
    results: Array<{ value: string; count: number }>;
    key_effects: Array<{ value: string; count: number }>;
    keys: Array<{ value: string; count: number }>;
  };
} {
  const start = (page - 1) * responsePerPage;
  const requestKindOptions = requestKindOptionsMock.map((option) => ({
    ...option,
    count: source.filter((log) => log.request_kind_key === option.key).length,
  }));
  return {
    items: source.slice(start, start + responsePerPage),
    page,
    per_page: responsePerPage,
    total: source.length,
    request_kind_options: requestKindOptions,
    facets: {
      results: buildFacetOptions(source.map((log) => log.result_status)),
      key_effects: buildFacetOptions(source.map((log) => log.key_effect_code ?? "none")),
      keys: buildFacetOptions(source.map((log) => log.key_id)),
    },
  };
}

function buildFacetOptions(values: Array<string | null | undefined>): Array<{ value: string; count: number }> {
  const counts = new Map<string, number>();
  for (const raw of values) {
    const value = raw?.trim();
    if (!value) continue;
    counts.set(value, (counts.get(value) ?? 0) + 1);
  }
  return Array.from(counts.entries())
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .map(([value, count]) => ({ value, count }));
}

function installFetchMock(
  detailOverride = tokenDetailMock,
  mode: StoryMode = "default",
  dataset: StoryDataset = "default",
): () => void {
  const originalFetch = window.fetch.bind(window);
  const activeTokenId = detailOverride.id;
  let initialLogsResolved = false;
  const storyData = storyDatasets[dataset];

  window.fetch = async (
    input: RequestInfo | URL,
    init?: RequestInit,
  ): Promise<Response> => {
    const request = input instanceof Request ? input : new Request(input, init);
    const url = new URL(request.url, window.location.origin);

    if (url.pathname === `/api/tokens/${activeTokenId}`) {
      if (mode === "initial_loading") {
        await wait(4_000);
      }
      return jsonResponse(detailOverride);
    }

    if (url.pathname === `/api/tokens/${activeTokenId}/metrics`) {
      if (mode === "initial_loading") {
        await wait(4_000);
      }
      return jsonResponse(storyData.metrics);
    }

    if (url.pathname === `/api/tokens/${activeTokenId}/logs/page`) {
      const perPage = Number(url.searchParams.get("per_page") ?? "20");
      const page = Number(url.searchParams.get("page") ?? "1");
      const selectedRequestKinds = url.searchParams.getAll("request_kind");
      const selectedResult = url.searchParams.get("result")?.trim() ?? "";
      const selectedKeyEffect = url.searchParams.get("key_effect")?.trim() ?? "";
      const selectedKeyId = url.searchParams.get("key_id")?.trim() ?? "";
      const source = (page === 2 ? storyData.logsPageTwo : storyData.logs).map((log) => ({
        ...log,
        auth_token_id: activeTokenId,
      }));
      const facetScopedSource = source.filter((log) => {
        if (selectedRequestKinds.length > 0 && !selectedRequestKinds.includes(log.request_kind_key)) {
          return false;
        }
        if (selectedKeyId && log.key_id !== selectedKeyId) {
          return false;
        }
        return true;
      });
      const filteredSource = facetScopedSource.filter((log) => {
        if (selectedResult && log.result_status !== selectedResult) {
          return false;
        }
        if (selectedKeyEffect && (log.key_effect_code ?? "none") !== selectedKeyEffect) {
          return false;
        }
        return true;
      });
      if (mode === "initial_loading") {
        await wait(4_000);
      } else if (mode === "switch_loading" && page === 2) {
        await wait(4_000);
      } else if (mode === "refreshing" && page === 1 && initialLogsResolved) {
        await wait(4_000);
      }
      const responsePerPage =
        !initialLogsResolved && page === 1 && storyData.initialPerPage != null
          ? storyData.initialPerPage
          : perPage;
      initialLogsResolved = true;
      return jsonResponse(buildLogPage(filteredSource, page, perPage, responsePerPage));
    }

    if (url.pathname === `/api/tokens/${activeTokenId}/metrics/usage-series`) {
      return jsonResponse(storyData.usageSeries);
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
      activeEventSources.add(this as unknown as MockEventSourceShape);
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
      activeEventSources.delete(this as unknown as MockEventSourceShape);
    }
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (window as any).EventSource = MockEventSource;

  return () => {
    window.EventSource = OriginalEventSource;
  };
}

function emitSnapshotRefresh(dataset: StoryDataset): void {
  const storyData = storyDatasets[dataset];
  const event = new MessageEvent("snapshot", {
    data: JSON.stringify({
      summary: storyData.metrics,
      logs: storyData.logs.slice(0, storyData.initialPerPage ?? 20),
    }),
  });
  activeEventSources.forEach((source) => {
    source.dispatchEvent(event);
  });
}

function openStoryInManager(storyId: string): void {
  addons.getChannel().emit(SELECT_STORY, { storyId });
}

export function TokenDetailStoryCanvas({
  detail = tokenDetailMock,
  mode = "default",
  dataset = "default",
}: {
  detail?: StoryTokenDetail;
  mode?: StoryMode;
  dataset?: StoryDataset;
}): JSX.Element {
  const [ready, setReady] = useState(false);

  useLayoutEffect(() => {
    const cleanupFetch = installFetchMock(detail, mode, dataset);
    const cleanupEventSource = installEventSourceMock();
    setReady(true);

    return () => {
      cleanupFetch();
      cleanupEventSource();
      setReady(false);
    };
  }, [dataset, detail, mode]);

  useLayoutEffect(() => {
    if (!ready) return;

    if (mode === "switch_loading") {
      const timer = window.setTimeout(() => {
        const nextButton = document.querySelectorAll<HTMLButtonElement>(
          ".table-pagination-button",
        )[1];
        nextButton?.click();
      }, 600);
      return () => window.clearTimeout(timer);
    }

    if (mode === "refreshing") {
      const timer = window.setTimeout(() => {
        emitSnapshotRefresh(dataset);
      }, 600);
      return () => window.clearTimeout(timer);
    }
  }, [dataset, mode, ready]);

  if (!ready) {
    return <div style={{ minHeight: "100vh" }} />;
  }

  return (
    <TokenDetail
      id={detail.id}
      onBack={() => undefined}
      onOpenUser={(userId) => {
        if (!userId) return;
        openStoryInManager("admin-pages--user-detail");
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
  args: { detail: tokenDetailMock, mode: "default" },
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};

export const Unbound: Story = {
  args: { detail: tokenDetailUnboundMock, mode: "default" },
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};

export const InitialLoading: Story = {
  args: { detail: tokenDetailMock, mode: "initial_loading" },
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};

export const SwitchLoading: Story = {
  args: { detail: tokenDetailMock, mode: "switch_loading" },
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};

export const Refreshing: Story = {
  args: { detail: tokenDetailMock, mode: "refreshing" },
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};

export const DenseRequestRecords: Story = {
  args: { detail: tokenDetailDenseMock, mode: "default", dataset: "dense" },
  parameters: {
    viewport: { defaultViewport: "1440-device-desktop" },
  },
};
