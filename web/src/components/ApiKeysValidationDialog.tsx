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

  return (
    <dialog
      id="keys_validation_modal"
      ref={props.dialogRef}
      className="modal"
      onClose={props.onClose}
      {...(props.forceOpen ? { open: true } : {})}
    >
      <div
        className="modal-box"
        style={{
          maxHeight: "min(calc(100dvh - 6rem), calc(100vh - 6rem))",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>
          {validationStrings.title ?? "Verify API Keys"}
        </h3>
        <div style={{ overflowY: "auto", minHeight: 0, paddingTop: 12 }}>
          {props.state ? (
            <>
              <div className="grid gap-3" style={{ gridTemplateColumns: "repeat(2, minmax(0, 1fr))" }}>
                <div>
                  <span className="opacity-70">{summaryStrings.inputLines ?? "Input lines"}</span>{" "}
                  {formatNumber(props.state.input_lines)}
                </div>
                <div>
                  <span className="opacity-70">{summaryStrings.validLines ?? "Valid lines"}</span>{" "}
                  {formatNumber(props.state.valid_lines)}
                </div>
                <div>
                  <span className="opacity-70">{summaryStrings.uniqueInInput ?? "Unique"}</span>{" "}
                  {formatNumber(props.state.unique_in_input)}
                </div>
                <div>
                  <span className="opacity-70">{summaryStrings.duplicateInInput ?? "Duplicates"}</span>{" "}
                  {formatNumber(props.state.duplicate_in_input)}
                </div>
                <div style={{ gridColumn: "1 / -1" }}>
                  <div className="opacity-70" style={{ marginBottom: 6 }}>
                    {(summaryStrings.checked ?? "Checked {checked} / {total}")
                      .replace("{checked}", String(props.counts.checked))
                      .replace("{total}", String(props.counts.totalToCheck))}
                  </div>
                  <progress
                    className="progress progress-primary w-full"
                    value={props.counts.checked}
                    max={Math.max(1, props.counts.totalToCheck)}
                  />
                </div>
                <div>
                  <span className="opacity-70">{summaryStrings.ok ?? "Valid"}</span>{" "}
                  {formatNumber(props.counts.ok)}
                </div>
                <div>
                  <span className="opacity-70">{summaryStrings.exhausted ?? "Exhausted"}</span>{" "}
                  {formatNumber(props.counts.exhausted)}
                </div>
                <div>
                  <span className="opacity-70">{summaryStrings.invalid ?? "Invalid"}</span>{" "}
                  {formatNumber(props.counts.invalid)}
                </div>
                <div>
                  <span className="opacity-70">{summaryStrings.error ?? "Error"}</span>{" "}
                  {formatNumber(props.counts.error)}
                </div>
              </div>

              {props.state.importError && (
                <div className="alert alert-error" style={{ marginTop: 12 }}>
                  {props.state.importError}
                </div>
              )}

              {props.state.importReport && (
                <div style={{ marginTop: 12 }}>
                  <h4 className="font-bold">{importStrings.title ?? "Import Result"}</h4>
                  <div className="py-2" style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 8 }}>
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

              <div className="overflow-x-auto" style={{ marginTop: 12 }}>
                <table className="table table-zebra">
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
                          <td style={{ wordBreak: "break-all" }}>
                            <code>{row.api_key}</code>
                          </td>
                          <td>
                            <div className="key-validation-detail">
                              <button type="button" className="key-validation-detail-trigger" aria-label={label}>
                                <StatusBadge tone={statusTone(row.status)}>{label}</StatusBadge>
                              </button>
                              {row.detail && <div className="key-validation-bubble">{row.detail}</div>}
                            </div>
                          </td>
                          <td>{quotaLabel}</td>
                          <td>
                            <button
                              type="button"
                              className="btn btn-sm"
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
            </>
          ) : (
            <div className="py-2">{validationStrings.hint ?? keyStrings.batch.hint}</div>
          )}
        </div>

        <div className="modal-action" style={{ marginTop: 12 }}>
          <form
            method="dialog"
            onSubmit={(e) => e.preventDefault()}
            style={{ display: "flex", gap: 8, flexWrap: "wrap", justifyContent: "flex-end" }}
          >
            <button type="button" className="btn" onClick={props.onClose}>
              {actions.close ?? keyStrings.batch.report.close}
            </button>
            <button
              type="button"
              className="btn btn-outline"
              onClick={props.onRetryFailed}
              disabled={!props.state || props.state.checking || props.state.importing || props.counts.invalid + props.counts.error === 0}
            >
              <Icon icon="mdi:refresh" width={18} height={18} />
              &nbsp;{actions.retryFailed ?? "Retry failed"}
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
          </form>
        </div>
      </div>
    </dialog>
  );
}
