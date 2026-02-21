import React from "react";
import { Icon } from "@iconify/react";

import { useTranslate } from "../i18n";
import { StatusBadge, type StatusTone } from "./StatusBadge";
import type { AddApiKeysBatchResponse } from "../api";

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

export interface ApiKeysValidationDialogProps {
  dialogRef: React.RefObject<HTMLDialogElement>;
  state: KeysValidationState | null;
  counts: KeysValidationCounts;
  validKeys: string[];
  exhaustedKeys: string[];
  forceOpen?: boolean;
  onClose: () => void;
  onRetryFailed: () => void;
  onRetryOne: (apiKey: string) => void;
  onImportValid: () => void;
}

export function ApiKeysValidationDialog(props: ApiKeysValidationDialogProps): JSX.Element {
  const adminStrings = useTranslate().admin;
  const keyStrings = adminStrings.keys;

  const validationStrings = (keyStrings as any).validation ?? {};
  const statuses = validationStrings.statuses ?? {};
  const actions = validationStrings.actions ?? {};
  const summaryStrings = validationStrings.summary ?? {};
  const tableStrings = validationStrings.table ?? {};
  const importStrings = validationStrings.import ?? {};

  const groupLabel = props.state?.group?.trim() || "default";
  const groupText = (summaryStrings.group ?? "Group: {group}").replace("{group}", groupLabel);

  const checkedText = (summaryStrings.checked ?? "Checked {checked} / {total}")
    .replace("{checked}", String(props.counts.checked))
    .replace("{total}", String(props.counts.totalToCheck));

  const isBusy = !!props.state?.checking || !!props.state?.importing;
  const hasFailures = props.counts.invalid + props.counts.error > 0;
  const canRetryFailed = !!props.state && !isBusy && hasFailures;
  const canImport = !!props.state && !isBusy && props.counts.pending === 0 && props.validKeys.length > 0;

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

  return (
    <dialog
      id="keys_validation_modal"
      ref={props.dialogRef}
      className="modal modal-bottom sm:modal-middle key-validation-modal"
      onClose={props.onClose}
      {...(props.forceOpen ? { open: true } : {})}
      style={{ overflowX: "hidden" }}
    >
      <div
        className="modal-box w-11/12 max-w-7xl p-0 overflow-hidden"
        style={{
          height: "min(calc(100dvh - 6rem), 760px)",
          overflowX: "hidden",
        }}
      >
        <div className="flex flex-col h-full">
          {/* Header */}
          <div className="px-4 sm:px-5 pt-4 pb-3 border-b border-base-200/70">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <h3 className="m-0 font-extrabold text-lg sm:text-xl tracking-tight">
                  {validationStrings.title ?? "Verify API Keys"}
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
              <button
                type="button"
                className="btn btn-sm btn-ghost btn-circle"
                onClick={props.onClose}
                title={actions.close ?? "Close"}
              >
                <Icon icon="mdi:close" width={18} height={18} />
              </button>
            </div>

            {props.state ? (
              <div className="mt-3">
                <progress
                  className="progress progress-primary w-full"
                  value={props.counts.checked}
                  max={Math.max(1, props.counts.totalToCheck)}
                />
                <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-sm">
                  <span className="opacity-70">
                    {summaryStrings.ok ?? "Valid"}:{" "}
                    <span className="font-mono tabular-nums">{formatNumber(props.counts.ok)}</span>
                  </span>
                  <span className="opacity-70">
                    {summaryStrings.exhausted ?? "Exhausted"}:{" "}
                    <span className="font-mono tabular-nums">{formatNumber(props.counts.exhausted)}</span>
                  </span>
                  <span className="opacity-70">
                    {summaryStrings.invalid ?? "Invalid"}:{" "}
                    <span className="font-mono tabular-nums">{formatNumber(props.counts.invalid)}</span>
                  </span>
                  <span className="opacity-70">
                    {summaryStrings.error ?? "Error"}:{" "}
                    <span className="font-mono tabular-nums">{formatNumber(props.counts.error)}</span>
                  </span>
                </div>
              </div>
            ) : null}
          </div>

          {/* Body */}
          <div className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden px-4 sm:px-5 py-3">
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
                      <h4 className="font-bold m-0">{importStrings.title ?? "Import Result"}</h4>
                      <span className="badge badge-success badge-outline">{actions.imported ?? "Imported"}</span>
                    </div>
                    <div className="mt-2 grid grid-cols-2 sm:grid-cols-4 gap-2 text-sm">
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
                <div className="md:hidden rounded-xl border border-base-200 bg-base-100 overflow-hidden">
                  <div className="divide-y divide-base-200/70">
                    {props.state.rows.map((row, index) => {
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
                      return (
                        <div key={`${row.api_key}-${index}`} className="p-3">
                          <div className="flex items-start justify-between gap-3">
                            <code className="block font-mono text-xs break-all whitespace-normal bg-base-200/50 px-2 py-1 rounded-lg max-w-full">
                              {row.api_key}
                            </code>
                            <button
                              type="button"
                              className="btn btn-ghost btn-xs btn-square"
                              onClick={() => props.onRetryOne(row.api_key)}
                              disabled={!canRetry}
                              aria-label={actions.retry ?? "Retry"}
                            >
                              <Icon icon="mdi:refresh" width={16} height={16} />
                            </button>
                          </div>

                          <div className="mt-2 flex flex-wrap items-center gap-2">
                            <StatusBadge
                              tone={statusTone(row.status)}
                              className="max-w-full flex-wrap whitespace-normal break-words"
                            >
                              {label}
                            </StatusBadge>
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
                </div>

                {/* >= md: table layout (fixed columns) */}
                <div className="hidden md:block rounded-xl border border-base-200 bg-base-100 overflow-hidden">
                  <div style={{ overflowX: "hidden" }}>
                    <table className="table table-sm table-zebra table-fixed w-full">
                      <colgroup>
                        <col style={{ width: "52%" }} />
                        <col style={{ width: "26%" }} />
                        <col style={{ width: "14%" }} />
                        <col style={{ width: "8%" }} />
                      </colgroup>
                      <thead>
                        <tr>
                          <th className="whitespace-nowrap">{tableStrings.apiKey ?? "API Key"}</th>
                          <th className="whitespace-nowrap">{tableStrings.result ?? "Result"}</th>
                          <th className="whitespace-nowrap text-right">{tableStrings.quota ?? "Quota"}</th>
                          <th className="whitespace-nowrap text-right px-2">
                            {tableStrings.actions ?? "Actions"}
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        {props.state.rows.map((row, index) => {
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
                          return (
                            <tr key={`${row.api_key}-${index}`}>
                              <td className="max-w-0">
                                <code className="block font-mono text-xs break-all whitespace-normal bg-base-200/50 px-2 py-1 rounded-lg max-w-full">
                                  {row.api_key}
                                </code>
                              </td>
                              <td className="max-w-0">
                                {row.detail ? (
                                  <details className="min-w-0 max-w-full">
                                    <summary className="cursor-pointer list-none inline-flex items-center gap-2 flex-wrap">
                                      <StatusBadge
                                        tone={statusTone(row.status)}
                                        className="max-w-full flex-wrap whitespace-normal break-words"
                                      >
                                        {label}
                                      </StatusBadge>
                                      <span className="opacity-60">
                                        <Icon icon="mdi:information-outline" width={16} height={16} />
                                      </span>
                                    </summary>
                                    <div className="mt-2 text-sm whitespace-pre-wrap break-all opacity-80 max-w-full">
                                      {row.detail}
                                    </div>
                                  </details>
                                ) : (
                                  <StatusBadge
                                    tone={statusTone(row.status)}
                                    className="max-w-full flex-wrap whitespace-normal break-words"
                                  >
                                    {label}
                                  </StatusBadge>
                                )}
                              </td>
                              <td className="text-right font-mono text-xs tabular-nums opacity-70 whitespace-nowrap">
                                {quotaLabel}
                              </td>
                              <td className="text-right px-2">
                                <button
                                  type="button"
                                  className="btn btn-ghost btn-xs btn-square"
                                  onClick={() => props.onRetryOne(row.api_key)}
                                  disabled={!canRetry}
                                  aria-label={actions.retry ?? "Retry"}
                                >
                                  <Icon icon="mdi:refresh" width={16} height={16} />
                                </button>
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                </div>
              </>
            ) : (
              <div className="py-2">{validationStrings.hint ?? keyStrings.batch.hint}</div>
            )}
          </div>

          {/* Footer */}
          <div className="px-4 sm:px-5 py-3 border-t border-base-200/70 bg-base-100">
            {props.exhaustedKeys.length > 0 && (
              <div className="mb-2 text-sm opacity-70 flex items-start gap-2 min-w-0">
                <span className="flex-shrink-0 mt-0.5">
                  <Icon icon="mdi:alert-circle-outline" width={16} height={16} />
                </span>
                <span className="min-w-0 whitespace-normal break-words">
                  {(summaryStrings.exhaustedNote ?? "{count} keys will be imported as exhausted").replace(
                    "{count}",
                    String(props.exhaustedKeys.length),
                  )}
                </span>
              </div>
            )}

            <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
              <button
                type="button"
                className="btn btn-outline"
                onClick={props.onRetryFailed}
                disabled={!canRetryFailed}
              >
                <Icon icon="mdi:refresh" width={18} height={18} />
                &nbsp;{actions.retryFailed ?? "Retry failed"}
              </button>

              <div className="flex items-center gap-2 justify-end flex-wrap sm:flex-nowrap flex-shrink-0">
                <button type="button" className="btn" onClick={props.onClose}>
                  {actions.close ?? keyStrings.batch.report.close}
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={props.onImportValid}
                  disabled={!canImport}
                >
                  <Icon
                    icon={props.state?.importing ? "mdi:progress-helper" : "mdi:tray-arrow-down"}
                    width={18}
                    height={18}
                  />
                  &nbsp;
                  {(actions.importValid ?? "Import {count} valid keys").replace(
                    "{count}",
                    String(props.validKeys.length),
                  )}
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </dialog>
  );
}
