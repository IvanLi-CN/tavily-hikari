import React from "react";
import { createPortal } from "react-dom";
import { Icon } from "../lib/icons";

import { useTranslate } from "../i18n";
import { useAnchoredFloatingLayer } from "../lib/useAnchoredFloatingLayer";
import { useViewportMode } from "../lib/responsive";
import { StatusBadge, type StatusTone } from "./StatusBadge";
import { Button } from "./ui/button";
import { Badge } from "./ui/badge";
import {
  Dialog,
  DialogContent,
} from "./ui/dialog";
import { Drawer, DrawerContent } from "./ui/drawer";
import { Tooltip, TooltipContent, TooltipTrigger } from "./ui/tooltip";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "./ui/table";
import type { AddApiKeysBatchResponse, ValidateAssignedProxyMatchKind } from "../api";

export type KeyValidationStatus =
  | "pending"
  | "duplicate_in_input"
  | "ok"
  | "ok_exhausted"
  | "unauthorized"
  | "forbidden"
  | "invalid"
  | "error";

export type KeyValidationRow = {
  api_key: string;
  status: KeyValidationStatus;
  registration_ip?: string | null;
  registration_region?: string | null;
  assigned_proxy_key?: string | null;
  assigned_proxy_label?: string | null;
  assigned_proxy_match_kind?: ValidateAssignedProxyMatchKind | null;
  quota_limit?: number;
  quota_remaining?: number;
  detail?: string;
  attempts: number;
};

export type KeysValidationState = {
  group: string;
  input_lines: number;
  valid_lines: number;
  unique_in_input: number;
  duplicate_in_input: number;
  checking: boolean;
  importing: boolean;
  rows: KeyValidationRow[];
  importReport?: AddApiKeysBatchResponse;
  importWarning?: string;
  importError?: string;
};

export type KeysValidationCounts = {
  pending: number;
  duplicate: number;
  ok: number;
  exhausted: number;
  invalid: number;
  error: number;
  checked: number;
  totalToCheck: number;
};

type ValidationFilterKey = "pending" | "ok" | "exhausted" | "invalid" | "error" | "duplicate";
const numberFormatter = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });

function formatNumber(value: number | null | undefined): string {
  if (value == null) return "—";
  return numberFormatter.format(value);
}

export function computeValidationCounts(state: KeysValidationState | null): KeysValidationCounts {
  const rows = state?.rows ?? [];
  let pending = 0;
  let duplicate = 0;
  let ok = 0;
  let exhausted = 0;
  let invalid = 0;
  let error = 0;

  for (const row of rows) {
    switch (row.status) {
      case "pending":
        pending += 1;
        break;
      case "duplicate_in_input":
        duplicate += 1;
        break;
      case "ok":
        ok += 1;
        break;
      case "ok_exhausted":
        exhausted += 1;
        break;
      case "unauthorized":
      case "forbidden":
      case "invalid":
        invalid += 1;
        break;
      case "error":
        error += 1;
        break;
    }
  }

  const checked = ok + exhausted + invalid + error;
  const totalToCheck = state?.unique_in_input ?? 0;
  return { pending, duplicate, ok, exhausted, invalid, error, checked, totalToCheck };
}

export function computeValidKeys(state: KeysValidationState | null): string[] {
  const set = new Set<string>();
  for (const row of state?.rows ?? []) {
    if (row.status === "ok" || row.status === "ok_exhausted") set.add(row.api_key);
  }
  return Array.from(set);
}

export function computeExhaustedKeys(state: KeysValidationState | null): string[] {
  const set = new Set<string>();
  for (const row of state?.rows ?? []) {
    if (row.status === "ok_exhausted") set.add(row.api_key);
  }
  return Array.from(set);
}

function statusTone(status: KeyValidationStatus): StatusTone {
  switch (status) {
    case "ok":
      return "success";
    case "ok_exhausted":
      return "warning";
    case "pending":
      return "info";
    case "duplicate_in_input":
      return "neutral";
    case "unauthorized":
    case "forbidden":
    case "invalid":
      return "error";
    case "error":
      return "error";
  }
}

function filterKeyForStatus(status: KeyValidationStatus): ValidationFilterKey {
  switch (status) {
    case "pending":
      return "pending";
    case "ok":
      return "ok";
    case "ok_exhausted":
      return "exhausted";
    case "duplicate_in_input":
      return "duplicate";
    case "unauthorized":
    case "forbidden":
    case "invalid":
      return "invalid";
    case "error":
      return "error";
  }
}

export function assignedProxyMatchToneClass(
  matchKind?: ValidateAssignedProxyMatchKind | null,
): string {
  switch (matchKind) {
    case "registration_ip":
      return "text-success";
    case "same_region":
      return "text-info";
    case "other":
      return "text-warning";
    default:
      return "";
  }
}

function RegistrationIpIndicator(props: {
  label: string;
  ip: string;
  region?: string | null;
  proxyLabel?: string | null;
  proxyKey?: string | null;
  proxyMatchKind?: ValidateAssignedProxyMatchKind | null;
  ipLabel: string;
  regionLabel: string;
  proxyLabelText: string;
}): JSX.Element {
  const triggerRef = React.useRef<HTMLSpanElement | null>(null);
  const [open, setOpen] = React.useState(false);
  const region = props.region?.trim() ?? null;
  const proxyValue = props.proxyLabel?.trim() || props.proxyKey?.trim() || null;
  const proxyValueToneClass = assignedProxyMatchToneClass(props.proxyMatchKind);
  const accessibleLabel = [
    `${props.ipLabel}: ${props.ip}`,
    region ? `${props.regionLabel}: ${region}` : null,
    proxyValue ? `${props.proxyLabelText}: ${proxyValue}` : null,
  ]
    .filter(Boolean)
    .join("; ");
  const { layerRef: bubbleRef, position } = useAnchoredFloatingLayer<HTMLSpanElement>({
    open,
    anchorEl: triggerRef.current,
    placement: "bottom",
    align: "center",
    offset: 10,
    viewportMargin: 12,
    arrowPadding: 18,
  });

  return (
    <span className="key-validation-detail">
      <span
        ref={triggerRef}
        className="key-validation-detail-trigger inline-flex"
        tabIndex={0}
        aria-label={accessibleLabel}
        data-registration-ip-trigger="true"
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => setOpen(false)}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
      >
        <Badge
          variant="success"
          className="gap-1 rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.08em]"
        >
          <Icon icon="mdi:check-bold" width={12} height={12} aria-hidden="true" />
          <span>{props.label}</span>
        </Badge>
      </span>
      {open && typeof document !== "undefined"
        ? createPortal(
            <span
              ref={bubbleRef}
              className="key-validation-bubble layer-popover"
              role="tooltip"
              data-placement={position?.placement ?? "bottom"}
              style={{
                top: `${position?.top ?? 0}px`,
                left: `${position?.left ?? 0}px`,
                visibility: position ? "visible" : "hidden",
                pointerEvents: "none",
                ["--key-validation-bubble-arrow-left" as string]: `${position?.arrowOffset ?? 40}px`,
              }}
            >
              <span className="key-validation-bubble-line">
                <span className="key-validation-bubble-label">{props.ipLabel}</span>
                <span className="key-validation-bubble-value">{props.ip}</span>
              </span>
              {region ? (
                <span className="key-validation-bubble-line">
                  <span className="key-validation-bubble-label">{props.regionLabel}</span>
                  <span className="key-validation-bubble-value">{region}</span>
                </span>
              ) : null}
              {proxyValue ? (
                <span className="key-validation-bubble-line">
                  <span className="key-validation-bubble-label">{props.proxyLabelText}</span>
                  <span
                    className={`key-validation-bubble-value${proxyValueToneClass ? ` ${proxyValueToneClass}` : ""}`}
                  >
                    {proxyValue}
                  </span>
                </span>
              ) : null}
            </span>,
            document.body,
          )
        : null}
    </span>
  );
}

export interface ApiKeysValidationDialogProps {
  open: boolean;
  state: KeysValidationState | null;
  counts: KeysValidationCounts;
  validKeys: string[];
  exhaustedKeys: string[];
  onClose: () => void;
  onRetryFailed: () => void;
  onRetryOne: (apiKey: string) => void;
  onImportValid: () => void;
}

export function ApiKeysValidationDialog(props: ApiKeysValidationDialogProps): JSX.Element {
  const adminStrings = useTranslate().admin;
  const viewportMode = useViewportMode();
  const keyStrings = adminStrings.keys;

  const validationStrings = keyStrings.validation;
  const statuses = validationStrings.statuses;
  const actions = validationStrings.actions;
  const summaryStrings = validationStrings.summary;
  const tableStrings = validationStrings.table;
  const importStrings = validationStrings.import;
  const ipBadgeLabel = validationStrings.registrationIpBadge ?? "IP";
  const registrationIpLabel = keyStrings.table.registrationIp ?? "Registration IP";
  const registrationRegionLabel = keyStrings.table.registrationRegion ?? "Region";
  const assignedProxyLabel = keyStrings.table.assignedProxy ?? "Assigned Proxy";
  const [activeFilter, setActiveFilter] = React.useState<ValidationFilterKey | null>(null);

  const groupLabel = props.state?.group?.trim() || "default";
  const groupText = summaryStrings.group.replace("{group}", groupLabel);

  const checkedText = (summaryStrings.checked ?? "Checked {checked} / {total}")
    .replace("{checked}", String(props.counts.checked))
    .replace("{total}", String(props.counts.totalToCheck));

  const segmentTotal = props.counts.totalToCheck;
  const segmentDivisor = Math.max(1, segmentTotal);
  const statusSegments: Array<{
    key: ValidationFilterKey;
    label: string;
    count: number;
    toneClass: string;
  }> = [
    {
      key: "pending",
      label: statuses.pending ?? "Pending",
      count: props.counts.pending,
      toneClass: "is-pending",
    },
    {
      key: "ok",
      label: summaryStrings.ok ?? "Valid",
      count: props.counts.ok,
      toneClass: "is-ok",
    },
    {
      key: "exhausted",
      label: summaryStrings.exhausted ?? "Exhausted",
      count: props.counts.exhausted,
      toneClass: "is-exhausted",
    },
    {
      key: "invalid",
      label: summaryStrings.invalid ?? "Invalid",
      count: props.counts.invalid,
      toneClass: "is-invalid",
    },
    {
      key: "error",
      label: summaryStrings.error ?? "Error",
      count: props.counts.error,
      toneClass: "is-error",
    },
  ];

  const isBusy = !!props.state?.checking || !!props.state?.importing;
  const hasFailures = props.counts.invalid + props.counts.error > 0;
  const canRetryFailed = !!props.state && !isBusy && hasFailures;
  const canImport = !!props.state && !isBusy && props.counts.pending === 0 && props.validKeys.length > 0;
  const filteredRows = React.useMemo(() => {
    const rows = props.state?.rows ?? [];
    if (!activeFilter) return rows;
    return rows.filter((row) => filterKeyForStatus(row.status) === activeFilter);
  }, [props.state?.rows, activeFilter]);
  const isSmallViewport = viewportMode === "small";
  const importVerboseLabel = (actions.importValid ?? "Import {count} valid keys").replace(
    "{count}",
    String(props.validKeys.length),
  );
  const importButtonLabel = isSmallViewport ? (actions.import ?? "Import") : importVerboseLabel;
  const retryFailedLabel = isSmallViewport ? (actions.retry ?? "Retry") : (actions.retryFailed ?? "Retry failed");
  const handleOpenChange = React.useCallback((open: boolean) => {
    if (!open) props.onClose();
  }, [props.onClose]);

  React.useEffect(() => {
    if (!props.state) return;
    // Prevent "mystery" horizontal scrollbars on the page while the modal is open.
    const prevHtml = document.documentElement.style.overflowX;
    const prevBody = document.body.style.overflowX;
    document.documentElement.style.overflowX = "hidden";
    document.body.style.overflowX = "hidden";
    return () => {
      document.documentElement.style.overflowX = prevHtml;
      document.body.style.overflowX = prevBody;
    };
  }, [props.state]);

  React.useEffect(() => {
    if (!props.state) {
      setActiveFilter(null);
    }
  }, [props.state]);

  const content = (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="key-validation-header px-4 md:px-5 pt-4 pb-3 border-b border-base-200/70">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <h3 className="m-0 font-extrabold text-lg md:text-xl tracking-tight">
              {validationStrings.title}
            </h3>
            <div className="mt-1 text-sm opacity-70 truncate">
              {groupText}
              {props.state ? (
                <>
                  {" "}
                  · {checkedText}
                </>
              ) : null}
            </div>
          </div>
          {!isSmallViewport ? (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="key-validation-close-button h-9 w-9 rounded-full"
              onClick={props.onClose}
              title={actions.close}
            >
              <Icon icon="mdi:close" width={18} height={18} />
            </Button>
          ) : null}
        </div>

        {props.state ? (
          <div className="mt-3">
            <div className="flex items-start justify-between gap-3">
              <div
                className="key-validation-segmented-bar"
                role="progressbar"
                aria-label={checkedText}
                aria-valuemin={0}
                aria-valuemax={segmentDivisor}
                aria-valuenow={props.counts.checked}
              >
                {statusSegments.map((segment) => {
                  if (segment.count <= 0) return null;
                  const width = (segment.count / segmentDivisor) * 100;
                  return (
                    <span
                      key={segment.key}
                      className={`key-validation-segment ${segment.toneClass}`}
                      style={{ width: `${width}%` }}
                      title={`${segment.label}: ${segment.count}`}
                    />
                  );
                })}
              </div>
            </div>
            <div className="mt-2 key-validation-segment-stats">
              {statusSegments.map((segment) => {
                const hasRate = segmentTotal > 0;
                const rate = hasRate ? Math.round((segment.count / segmentTotal) * 100) : 0;
                const isActive = activeFilter === segment.key;
                const isDimmed = !!activeFilter && activeFilter !== segment.key;
                const segmentButton = (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    key={segment.key}
                    className={`key-validation-segment-stat ${segment.toneClass}${hasRate ? " has-rate" : ""}${
                      isActive ? " is-active" : ""
                    }${isDimmed ? " is-dimmed" : ""}`}
                    onClick={() => setActiveFilter((prev) => (prev === segment.key ? null : segment.key))}
                    aria-pressed={isActive}
                  >
                    <span className="key-validation-segment-dot" />
                    {segment.label}:{" "}
                    <span className="font-mono tabular-nums">{formatNumber(segment.count)}</span>
                  </Button>
                );

                if (!hasRate) {
                  return segmentButton;
                }

                return (
                  <Tooltip key={segment.key}>
                    <TooltipTrigger asChild>{segmentButton}</TooltipTrigger>
                    <TooltipContent side="bottom" className="text-center">
                      {rate}%
                    </TooltipContent>
                  </Tooltip>
                );
              })}
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className={`key-validation-segment-stat is-duplicate${
                  activeFilter === "duplicate" ? " is-active" : ""
                }${activeFilter && activeFilter !== "duplicate" ? " is-dimmed" : ""}`}
                onClick={() => setActiveFilter((prev) => (prev === "duplicate" ? null : "duplicate"))}
                aria-pressed={activeFilter === "duplicate"}
              >
                <span className="key-validation-segment-dot" />
                {statuses.duplicate_in_input ?? "Duplicate"}:{" "}
                <span className="font-mono tabular-nums">{formatNumber(props.counts.duplicate)}</span>
              </Button>
            </div>
          </div>
        ) : null}
      </div>

      {/* Body */}
      <div className="key-validation-modal-body flex-1 min-h-0 overflow-y-auto overflow-x-hidden px-4 md:px-5 py-3">
        {props.state ? (
          <>
            {props.state.importError && (
              <div className="alert alert-error mb-3">
                {props.state.importError}
              </div>
            )}

            {props.state.importReport && (
              <div className="mb-3 rounded-xl border border-base-200 bg-base-100 p-3">
                <div className="flex items-center gap-2">
                  <h4 className="font-bold m-0">{importStrings.title}</h4>
                  <span className="badge badge-success badge-outline">{actions.imported}</span>
                </div>
                <div className="mt-2 grid grid-cols-2 md:grid-cols-4 gap-2 text-sm">
                  <div>
                    <span className="opacity-70">{keyStrings.batch.report.summary.created}</span>{" "}
                    {formatNumber(props.state.importReport.summary.created)}
                  </div>
                  <div>
                    <span className="opacity-70">{keyStrings.batch.report.summary.undeleted}</span>{" "}
                    {formatNumber(props.state.importReport.summary.undeleted)}
                  </div>
                  <div>
                    <span className="opacity-70">{keyStrings.batch.report.summary.existed}</span>{" "}
                    {formatNumber(props.state.importReport.summary.existed)}
                  </div>
                  <div>
                    <span className="opacity-70">{keyStrings.batch.report.summary.failed}</span>{" "}
                    {formatNumber(props.state.importReport.summary.failed)}
                  </div>
                </div>
              </div>
            )}

            {/* < md: stacked cards */}
            <div className="key-validation-mobile-list md:hidden rounded-xl border border-base-200 bg-base-100 overflow-hidden">
              {filteredRows.length === 0 ? (
                <div className="p-4 text-sm opacity-70">
                  {validationStrings.emptyFiltered}
                </div>
              ) : (
                <div className="divide-y divide-base-200/70">
                  {filteredRows.map((row, index) => {
                    const canRetry =
                      !isBusy &&
                      (row.status === "unauthorized" ||
                        row.status === "forbidden" ||
                        row.status === "invalid" ||
                        row.status === "error");
                    const quotaLabel =
                      row.quota_remaining != null && row.quota_limit != null
                        ? `${formatNumber(row.quota_remaining)}/${formatNumber(row.quota_limit)}`
                        : "—";
                    const label = statuses[row.status] ?? row.status;
                    const registrationIp = row.registration_ip?.trim();
                    const registrationRegion = row.registration_region?.trim() ?? null;
                    const assignedProxyKey = row.assigned_proxy_key?.trim() ?? null;
                    const assignedProxyLabelValue = row.assigned_proxy_label?.trim() ?? null;
                    const assignedProxyMatchKind = row.assigned_proxy_match_kind ?? null;
                    return (
                      <div key={`${row.api_key}-${index}`} className="p-3">
                        <div className="flex items-start justify-between gap-3">
                          <code className="block font-mono text-xs break-all whitespace-normal bg-base-200/50 px-2 py-1 rounded-lg max-w-full">
                            {row.api_key}
                          </code>
                          <Button
                            type="button"
                            variant="ghost"
                            size="xs"
                            className="key-validation-row-retry-button h-7 w-7 px-0"
                            onClick={() => props.onRetryOne(row.api_key)}
                            disabled={!canRetry}
                            aria-label={actions.retry ?? "Retry"}
                          >
                            <Icon icon="mdi:refresh" width={16} height={16} />
                          </Button>
                        </div>

                        <div className="mt-2 flex flex-wrap items-center gap-2">
                          <StatusBadge
                            tone={statusTone(row.status)}
                            className="max-w-full flex-wrap whitespace-normal break-words"
                          >
                            {label}
                          </StatusBadge>
                          {registrationIp ? (
                            <RegistrationIpIndicator
                              label={ipBadgeLabel}
                              ip={registrationIp}
                              region={registrationRegion}
                              proxyKey={assignedProxyKey}
                              proxyLabel={assignedProxyLabelValue}
                              proxyMatchKind={assignedProxyMatchKind}
                              ipLabel={registrationIpLabel}
                              regionLabel={registrationRegionLabel}
                              proxyLabelText={assignedProxyLabel}
                            />
                          ) : null}
                          <span className="text-xs font-mono tabular-nums opacity-70 whitespace-nowrap">{quotaLabel}</span>
                        </div>

                        {row.detail && (
                          <div className="mt-2 text-sm whitespace-pre-wrap break-all opacity-80 max-w-full">
                            {row.detail}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>

            {/* >= md: table layout (fixed columns) */}
            <div className="key-validation-table-shell hidden md:block rounded-xl border border-base-200 bg-base-100 overflow-hidden">
              <div className="key-validation-table-scroll">
                <Table className="table-fixed w-full key-validation-table text-sm">
                  <colgroup>
                    <col style={{ width: "52%" }} />
                    <col style={{ width: "26%" }} />
                    <col style={{ width: "14%" }} />
                    <col style={{ width: "8%" }} />
                  </colgroup>
                  <TableHeader>
                    <TableRow>
                      <TableHead className="whitespace-nowrap">{tableStrings.apiKey ?? "API Key"}</TableHead>
                      <TableHead className="whitespace-nowrap">{tableStrings.result ?? "Result"}</TableHead>
                      <TableHead className="whitespace-nowrap text-right">{tableStrings.quota ?? "Quota"}</TableHead>
                      <TableHead className="whitespace-nowrap px-2 text-right">
                        {tableStrings.actions ?? "Actions"}
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {filteredRows.length === 0 ? (
                      <TableRow>
                        <TableCell colSpan={4} className="py-4 text-center opacity-70">
                          {validationStrings.emptyFiltered}
                        </TableCell>
                      </TableRow>
                    ) : (
                      filteredRows.map((row, index) => {
                        const canRetry =
                          !isBusy &&
                          (row.status === "unauthorized" ||
                            row.status === "forbidden" ||
                            row.status === "invalid" ||
                            row.status === "error");
                        const quotaLabel =
                          row.quota_remaining != null && row.quota_limit != null
                            ? `${formatNumber(row.quota_remaining)}/${formatNumber(row.quota_limit)}`
                            : "—";
                        const label = statuses[row.status] ?? row.status;
                        const registrationIp = row.registration_ip?.trim();
                        const registrationRegion = row.registration_region?.trim() ?? null;
                        const assignedProxyKey = row.assigned_proxy_key?.trim() ?? null;
                        const assignedProxyLabelValue = row.assigned_proxy_label?.trim() ?? null;
                        const assignedProxyMatchKind = row.assigned_proxy_match_kind ?? null;
                        return (
                          <TableRow key={`${row.api_key}-${index}`}>
                            <TableCell className="max-w-0">
                              <code className="block font-mono text-xs break-all whitespace-normal bg-base-200/50 px-2 py-1 rounded-lg max-w-full">
                                {row.api_key}
                              </code>
                            </TableCell>
                            <TableCell className="max-w-0">
                              {row.detail ? (
                                <details className="key-validation-detail-disclosure min-w-0 max-w-full">
                                  <summary className="cursor-pointer list-none inline-flex items-center gap-2 flex-wrap">
                                    <StatusBadge
                                      tone={statusTone(row.status)}
                                      className="max-w-full flex-wrap whitespace-normal break-words"
                                    >
                                      {label}
                                    </StatusBadge>
                                    {registrationIp ? (
                                      <RegistrationIpIndicator
                                        label={ipBadgeLabel}
                                        ip={registrationIp}
                                        region={registrationRegion}
                                        proxyKey={assignedProxyKey}
                                        proxyLabel={assignedProxyLabelValue}
                                        proxyMatchKind={assignedProxyMatchKind}
                                        ipLabel={registrationIpLabel}
                                        regionLabel={registrationRegionLabel}
                                        proxyLabelText={assignedProxyLabel}
                                      />
                                    ) : null}
                                    <span className="opacity-60">
                                      <Icon icon="mdi:information-outline" width={16} height={16} />
                                    </span>
                                  </summary>
                                  <div className="mt-2 text-sm whitespace-pre-wrap break-all opacity-80 max-w-full">
                                    {row.detail}
                                  </div>
                                </details>
                              ) : (
                                <div className="inline-flex max-w-full flex-wrap items-center gap-2">
                                  <StatusBadge
                                    tone={statusTone(row.status)}
                                    className="max-w-full flex-wrap whitespace-normal break-words"
                                  >
                                    {label}
                                  </StatusBadge>
                                  {registrationIp ? (
                                    <RegistrationIpIndicator
                                      label={ipBadgeLabel}
                                      ip={registrationIp}
                                      region={registrationRegion}
                                      proxyKey={assignedProxyKey}
                                      proxyLabel={assignedProxyLabelValue}
                                      proxyMatchKind={assignedProxyMatchKind}
                                      ipLabel={registrationIpLabel}
                                      regionLabel={registrationRegionLabel}
                                      proxyLabelText={assignedProxyLabel}
                                    />
                                  ) : null}
                                </div>
                              )}
                            </TableCell>
                            <TableCell className="text-right font-mono text-xs tabular-nums opacity-70 whitespace-nowrap">
                              {quotaLabel}
                            </TableCell>
                            <TableCell className="px-2 text-right">
                              <Button
                                type="button"
                                variant="ghost"
                                size="xs"
                                className="key-validation-row-retry-button h-7 w-7 px-0"
                                onClick={() => props.onRetryOne(row.api_key)}
                                disabled={!canRetry}
                                aria-label={actions.retry ?? "Retry"}
                              >
                                <Icon icon="mdi:refresh" width={16} height={16} />
                              </Button>
                            </TableCell>
                          </TableRow>
                        );
                      })
                    )}
                  </TableBody>
                </Table>
              </div>
            </div>
          </>
        ) : (
          <div className="py-2">{validationStrings.hint ?? keyStrings.batch.hint}</div>
        )}
      </div>

      {/* Footer */}
      <div className="key-validation-footer px-4 md:px-5 py-3 border-t border-base-200/70 bg-base-100">
        {props.exhaustedKeys.length > 0 && (
          <div className="mb-2 text-sm opacity-70 flex items-start gap-2 min-w-0">
            <span className="flex-shrink-0 mt-0.5">
              <Icon icon="mdi:alert-circle-outline" width={16} height={16} />
            </span>
            <span className="min-w-0 whitespace-normal break-words">
              {summaryStrings.exhaustedNote.replace(
                "{count}",
                String(props.exhaustedKeys.length),
              )}
            </span>
          </div>
        )}
        {props.state?.importWarning && (
          <div className="alert alert-warning mb-2 text-sm">
            {props.state.importWarning}
          </div>
        )}

        <div className="key-validation-footer-actions flex flex-wrap items-center justify-between gap-2">
          <Button
            type="button"
            variant="outline"
            onClick={props.onRetryFailed}
            disabled={!canRetryFailed}
          >
            <Icon icon="mdi:refresh" width={18} height={18} />
            &nbsp;{retryFailedLabel}
          </Button>

          <div className="key-validation-footer-primary flex items-center gap-2 justify-end flex-wrap md:flex-nowrap flex-shrink-0">
            <Button type="button" variant="secondary" onClick={props.onClose}>
              {actions.close ?? keyStrings.batch.report.close}
            </Button>
            <Button
              type="button"
              className="key-validation-import-button"
              onClick={props.onImportValid}
              disabled={!canImport}
              aria-label={importVerboseLabel}
            >
              <Icon
                icon={props.state?.importing ? "mdi:progress-helper" : "mdi:tray-arrow-down"}
                width={18}
                height={18}
              />
              &nbsp;<span>{importButtonLabel}</span>
              {isSmallViewport && props.validKeys.length > 0 ? (
                <span className="key-validation-import-count-badge" aria-hidden="true">
                  {props.validKeys.length}
                </span>
              ) : null}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );

  if (isSmallViewport) {
    return (
      <Drawer open={props.open} onOpenChange={handleOpenChange} shouldScaleBackground={false}>
        <DrawerContent className="key-validation-drawer-content">
          {content}
        </DrawerContent>
      </Drawer>
    );
  }

  return (
    <Dialog open={props.open} onOpenChange={handleOpenChange}>
      <DialogContent className="key-validation-modal key-validation-modal-box max-w-5xl gap-0 p-0 sm:max-h-[min(calc(100dvh-4rem),calc(100vh-4rem))] [&>button]:hidden">
        {content}
      </DialogContent>
    </Dialog>
  );
}
