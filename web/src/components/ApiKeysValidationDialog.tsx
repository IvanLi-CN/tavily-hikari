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

  return (
    <dialog
      id="keys_validation_modal"
      ref={props.dialogRef}
      className="modal modal-bottom sm:modal-middle"
      onClose={props.onClose}
      {...(props.forceOpen ? { open: true } : {})}
    >
      <div
        className="modal-box w-11/12 max-w-6xl p-0"
        style={{
          maxHeight: "min(calc(100dvh - 6rem), calc(100vh - 6rem))",
          display: "flex",
          flexDirection: "column",
        }}
      >
        {/* Header */}
        <div className="p-5 sm:p-6 border-b border-base-200/70 bg-base-100">
          <div className="flex items-start justify-between gap-3">
            <div>
              <div className="flex items-center gap-2">
                <span className="inline-flex h-9 w-9 items-center justify-center rounded-2xl bg-primary/10 text-primary">
                  <Icon icon="mdi:key-chain-variant" width={20} height={20} />
                </span>
                <h3 className="font-extrabold text-xl tracking-tight m-0">
                  {validationStrings.title ?? "Verify API Keys"}
                </h3>
              </div>
              <div className="mt-2 flex flex-wrap items-center gap-2 text-sm">
                <span className="badge badge-outline">{groupText}</span>
                {props.state && (
                  <>
                    <span className="badge badge-ghost">
                      {(summaryStrings.inputLines ?? "Input lines").replace("{count}", String(props.state.input_lines))}
                      :&nbsp;{formatNumber(props.state.input_lines)}
                    </span>
                    <span className="badge badge-ghost">
                      {(summaryStrings.uniqueInInput ?? "Unique").replace("{count}", String(props.state.unique_in_input))}
                      :&nbsp;{formatNumber(props.state.unique_in_input)}
                    </span>
                    {props.state.duplicate_in_input > 0 && (
                      <span className="badge badge-ghost">
                        {(summaryStrings.duplicateInInput ?? "Duplicates").replace(
                          "{count}",
                          String(props.state.duplicate_in_input),
                        )}
                        :&nbsp;{formatNumber(props.state.duplicate_in_input)}
                      </span>
                    )}
                  </>
                )}
              </div>
            </div>
            <button type="button" className="btn btn-sm btn-ghost btn-circle" onClick={props.onClose}>
              <Icon icon="mdi:close" width={18} height={18} />
            </button>
          </div>
        </div>

        {/* Body */}
        <div className="px-5 sm:px-6 py-4" style={{ overflowY: "auto", minHeight: 0 }}>
          {props.state ? (
            <>
              {/* Progress + headline stats */}
              <div className="rounded-2xl border border-base-200 bg-base-100 p-4">
                <div className="flex items-center gap-3">
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center justify-between gap-3 text-sm opacity-70">
                      <div className="truncate">
                        {(summaryStrings.checked ?? "Checked {checked} / {total}")
                          .replace("{checked}", String(props.counts.checked))
                          .replace("{total}", String(props.counts.totalToCheck))}
                      </div>
                      <div className="font-mono tabular-nums">
                        {formatNumber(props.counts.checked)} / {formatNumber(props.counts.totalToCheck)}
                      </div>
                    </div>
                    <progress
                      className="progress progress-primary w-full mt-2"
                      value={props.counts.checked}
                      max={Math.max(1, props.counts.totalToCheck)}
                    />
                  </div>
                  <div className="hidden sm:flex">
                    <div
                      className="radial-progress text-primary"
                      style={
                        {
                          "--value": Math.round((props.counts.checked / Math.max(1, props.counts.totalToCheck)) * 100),
                          "--size": "3.25rem",
                          "--thickness": "6px",
                        } as React.CSSProperties
                      }
                      aria-label="Progress percent"
                    >
                      <span className="text-sm font-mono tabular-nums">
                        {Math.round((props.counts.checked / Math.max(1, props.counts.totalToCheck)) * 100)}%
                      </span>
                    </div>
                  </div>
                </div>

                <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mt-4">
                  <div className="rounded-xl bg-base-200/60 p-3">
                    <div className="text-xs uppercase tracking-wide opacity-70">{summaryStrings.ok ?? "Valid"}</div>
                    <div className="text-lg font-bold tabular-nums">{formatNumber(props.counts.ok)}</div>
                  </div>
                  <div className="rounded-xl bg-base-200/60 p-3">
                    <div className="text-xs uppercase tracking-wide opacity-70">{summaryStrings.exhausted ?? "Exhausted"}</div>
                    <div className="text-lg font-bold tabular-nums">{formatNumber(props.counts.exhausted)}</div>
                  </div>
                  <div className="rounded-xl bg-base-200/60 p-3">
                    <div className="text-xs uppercase tracking-wide opacity-70">{summaryStrings.invalid ?? "Invalid"}</div>
                    <div className="text-lg font-bold tabular-nums">{formatNumber(props.counts.invalid)}</div>
                  </div>
                  <div className="rounded-xl bg-base-200/60 p-3">
                    <div className="text-xs uppercase tracking-wide opacity-70">{summaryStrings.error ?? "Error"}</div>
                    <div className="text-lg font-bold tabular-nums">{formatNumber(props.counts.error)}</div>
                  </div>
                </div>
              </div>

              {props.state.importError && (
                <div className="alert alert-error mt-4">
                  {props.state.importError}
                </div>
              )}

              {props.state.importReport && (
                <div className="mt-4">
                  <div className="flex items-center gap-2">
                    <h4 className="font-bold m-0">{importStrings.title ?? "Import Result"}</h4>
                    <span className="badge badge-success badge-outline">{actions.imported ?? "Imported"}</span>
                  </div>
                  <div className="mt-2 grid grid-cols-2 sm:grid-cols-4 gap-2">
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

              <div className="mt-4 rounded-2xl border border-base-200 bg-base-100">
                <div className="px-4 py-3 border-b border-base-200/70 flex items-center justify-between gap-2">
                  <div className="font-semibold">{tableStrings.title ?? "Keys"}</div>
                  <div className="text-sm opacity-70">
                    {formatNumber(props.state.rows.length)}{" "}
                    {(tableStrings.rows ?? "rows").replace("{count}", String(props.state.rows.length))}
                  </div>
                </div>
                <div className="overflow-x-auto">
                  <table className="table table-sm table-zebra">
                  <thead>
                    <tr>
                      <th>{tableStrings.apiKey ?? "API Key"}</th>
                      <th>{tableStrings.result ?? "Result"}</th>
                      <th>{tableStrings.quota ?? "Quota"}</th>
                      <th>{tableStrings.actions ?? "Actions"}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {props.state.rows.map((row, index) => {
                      const canRetry =
                        !props.state?.checking &&
                        !props.state?.importing &&
                        (row.status === "unauthorized" || row.status === "forbidden" || row.status === "invalid" || row.status === "error");
                      const quotaLabel =
                        row.quota_remaining != null && row.quota_limit != null
                          ? `${formatNumber(row.quota_remaining)} / ${formatNumber(row.quota_limit)}`
                          : "—";
                      const label = statuses[row.status] ?? row.status;
                      return (
                        <tr key={`${row.api_key}-${index}`}>
                          <td style={{ wordBreak: "break-all" }} className="font-mono text-xs">
                            <code className="bg-base-200/50 px-2 py-1 rounded-lg">{row.api_key}</code>
                          </td>
                          <td>
                            <div className="key-validation-detail">
                              <StatusBadge tone={statusTone(row.status)}>{label}</StatusBadge>
                              {row.detail && (
                                <>
                                  <button
                                    type="button"
                                    className="btn btn-ghost btn-xs key-validation-detail-trigger"
                                    aria-label={actions.details ?? "Details"}
                                  >
                                    <Icon icon="mdi:information-outline" width={16} height={16} />
                                  </button>
                                  <div className="key-validation-bubble">{row.detail}</div>
                                </>
                              )}
                            </div>
                          </td>
                          <td className="font-mono text-xs tabular-nums">{quotaLabel}</td>
                          <td>
                            <button
                              type="button"
                              className="btn btn-xs btn-outline"
                              onClick={() => props.onRetryOne(row.api_key)}
                              disabled={!canRetry}
                            >
                              <Icon icon="mdi:refresh" width={16} height={16} />
                              &nbsp;{actions.retry ?? "Retry"}
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
        <div className="px-5 sm:px-6 py-4 border-t border-base-200/70 bg-base-100">
          <form method="dialog" onSubmit={(e) => e.preventDefault()} className="flex flex-wrap gap-2 items-center justify-between">
            <div className="flex flex-wrap gap-2 items-center">
              <button
                type="button"
                className="btn btn-outline"
                onClick={props.onRetryFailed}
                disabled={!props.state || props.state.checking || props.state.importing || props.counts.invalid + props.counts.error === 0}
              >
                <Icon icon="mdi:refresh" width={18} height={18} />
                &nbsp;{actions.retryFailed ?? "Retry failed"}
              </button>
              {props.exhaustedKeys.length > 0 && (
                <span className="text-sm opacity-70">
                  <Icon icon="mdi:alert-circle-outline" width={16} height={16} />{" "}
                  {(summaryStrings.exhaustedNote ?? "{count} keys will be imported as exhausted")
                    .replace("{count}", String(props.exhaustedKeys.length))}
                </span>
              )}
            </div>

            <div className="flex flex-wrap gap-2 items-center justify-end">
              <button type="button" className="btn" onClick={props.onClose}>
                {actions.close ?? keyStrings.batch.report.close}
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={props.onImportValid}
                disabled={!props.state || props.state.checking || props.state.importing || props.counts.pending > 0 || props.validKeys.length === 0}
              >
                <Icon icon={props.state?.importing ? "mdi:progress-helper" : "mdi:tray-arrow-down"} width={18} height={18} />
                &nbsp;
                {(actions.importValid ?? "Import {count} valid keys").replace("{count}", String(props.validKeys.length))}
              </button>
            </div>
          </form>
        </div>
      </div>
    </dialog>
  );
}
