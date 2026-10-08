/**
 * Each fixture agent's confirmed mandate versions, oldest first, as `MandateVersionCreated`,
 * `MandateConfirmed` and `MandateVersionApplied` record them (mandate spec §2.2, §9.2, §10). The
 * diffs end at the fixture mandates in `mandates.ts`, and every approval is bound to the version in
 * effect when it was requested; `versions.test.ts` checks both. Fixture values, not a record.
 */
import type { MandateVersionRecord } from "./types";

const t = (hms: string, date: string) => `${date}T${hms}-04:00`;

export const VERSION = {
  btc: "sha256:516967c342c510b904903fb8494951bbafa61c9ef0a16fc7b2e62426c0daedee",
  swingFirst: "sha256:1d0d825f4597c402ed460a2331b1cd7725138d9e5ce66c1a73072d3d181cbdb3",
  swing: "sha256:587390f50e024688e71d20f95912694b3c72eb20605e1a305bcb38d36bca5471",
  lmnFirst: "sha256:6db43ae1f375c241e4235e4067819a10d8e5f8af58b9ebf6911b35229653107e",
  lmn: "sha256:95120ef74b1d56aa7db4a8c7c607a374b1f9177d7768d7732483f66ecafd9f9f",
  btcRaise: "sha256:b98128f35e178c3041ab5e5fb517d51446ace1dcac5ac16edc7d35d6f0d8ec37",
} as const;

export const BTC_VERSIONS: MandateVersionRecord[] = [
  {
    mandate_version: VERSION.btc,
    previous: null,
    confirmed_at: t("09:24:10", "2026-09-21"),
    step_up: true,
    classification: null,
    changes: [],
    application: { result: "applied", at: t("09:30:00", "2026-09-21"), approvals_canceled: 0 },
  },
];

export const SWING_VERSIONS: MandateVersionRecord[] = [
  {
    mandate_version: VERSION.swingFirst,
    previous: null,
    confirmed_at: t("09:21:45", "2026-09-22"),
    step_up: true,
    classification: null,
    changes: [],
    application: { result: "applied", at: t("09:30:00", "2026-09-22"), approvals_canceled: 0 },
  },
  {
    mandate_version: VERSION.swing,
    previous: VERSION.swingFirst,
    confirmed_at: t("08:15:00", "2026-09-25"),
    step_up: true,
    classification: "risk_increasing",
    changes: [
      { path: "/notifications/quiet_hours/start", from: "22:30", to: "23:00", classification: "neutral" },
      { path: "/risk/max_gross_exposure_usd", from: "1500", to: "2000", classification: "risk_increasing" },
    ],
    application: { result: "applied", at: t("08:15:04", "2026-09-25"), approvals_canceled: 0 },
  },
];

export const LMN_VERSIONS: MandateVersionRecord[] = [
  {
    mandate_version: VERSION.lmnFirst,
    previous: null,
    confirmed_at: t("09:18:30", "2026-09-23"),
    step_up: true,
    classification: null,
    changes: [],
    application: { result: "applied", at: t("09:30:00", "2026-09-23"), approvals_canceled: 0 },
  },
  {
    mandate_version: VERSION.lmn,
    previous: VERSION.lmnFirst,
    confirmed_at: t("09:05:00", "2026-09-28"),
    step_up: false,
    classification: "risk_reducing",
    changes: [
      { path: "/autonomy/approval/two_approver_above_usd", from: null, to: "400", classification: "risk_reducing" },
      { path: "/risk/max_position_usd", from: "3000", to: "2500", classification: "risk_reducing" },
    ],
    application: { result: "applied", at: t("09:05:00", "2026-09-28"), approvals_canceled: 1 },
  },
];

/** The drawdown scenario's raise: confirmed, then refused at application because the ladder had latched (mandate spec §5.1). */
export const BTC_RAISE_REJECTED: MandateVersionRecord = {
  mandate_version: VERSION.btcRaise,
  previous: VERSION.btc,
  confirmed_at: t("14:03:30", "2026-09-28"),
  step_up: true,
  classification: "risk_increasing",
  changes: [{ path: "/capital/allocation_usd", from: "10000", to: "12000", classification: "risk_increasing" }],
  application: {
    result: "rejected",
    at: t("14:03:34", "2026-09-28"),
    reason: "An allocation increase is refused while a limit is latched, and the drawdown ladder holds this agent at selling only.",
  },
};
