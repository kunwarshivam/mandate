import type { MandateChange } from "@/fixtures/types";
import { percent, seconds, usd } from "./format";

type Format = "usd" | "fraction" | "count" | "seconds" | "text";

/** The owner's names for the mandate fields a version diff can name, matching the Mandate tab. */
const FIELDS: Record<string, { label: string; format: Format }> = {
  "/capital/allocation_usd": { label: "Capital", format: "usd" },
  "/capital/max_loss_from_allocation": { label: "Lifetime loss limit", format: "fraction" },
  "/risk/max_order_usd": { label: "Largest order", format: "usd" },
  "/risk/max_position_usd": { label: "Largest position", format: "usd" },
  "/risk/max_position_fraction": { label: "Largest position, share of equity", format: "fraction" },
  "/risk/max_gross_exposure_usd": { label: "Total holdings limit", format: "usd" },
  "/risk/max_orders_per_day": { label: "Orders a day", format: "count" },
  "/risk/max_daily_loss": { label: "Daily loss limit", format: "fraction" },
  "/risk/max_drawdown": { label: "Drawdown limit", format: "fraction" },
  "/protection/stop_distance": { label: "Protective stop", format: "fraction" },
  "/autonomy/approval/timeout_s": { label: "Approval window", format: "seconds" },
  "/autonomy/approval/two_approver_above_usd": { label: "Two approvers above", format: "usd" },
  "/notifications/quiet_hours/start": { label: "Quiet hours start", format: "text" },
  "/notifications/quiet_hours/end": { label: "Quiet hours end", format: "text" },
};

/** A path with no name here is shown as its JSON Pointer, so nothing in a diff is ever hidden. */
export function changeLabel(path: string): string {
  return FIELDS[path]?.label ?? path;
}

export function changeValue(path: string, value: MandateChange["from"]): string {
  if (value === null) return "Not set";
  const format = FIELDS[path]?.format ?? "text";
  switch (format) {
    case "usd":
      return usd(String(value));
    case "fraction":
      return percent(String(value));
    case "count":
      return String(value);
    case "seconds":
      return seconds(Number(value));
    case "text":
      return String(value);
    default: {
      const unhandled: never = format;
      throw new Error(`unhandled format ${String(unhandled)}`);
    }
  }
}
