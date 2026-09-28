import type { CommandKind } from "@/lib/mock-runtime";

/** Pause removes permissions and can be undone, so it alone skips step-up (PX-4 (b)). */
export function needsStepUp(kind: CommandKind): boolean {
  return kind !== "pause" && kind !== "pause_all";
}

export function commandTitle(kind: CommandKind, label: string): string {
  switch (kind) {
    case "pause":
      return `Pause ${label}`;
    case "pause_all":
      return "Pause all agents on this account";
    case "resume":
      return `Resume ${label}`;
    case "stop":
      return `Stop ${label}`;
    case "kill":
      return `Kill switch for ${label}`;
    case "release":
      return `Stop ${label} and release its positions`;
    case "stop_all":
      return "Stop all agents on this account";
    case "close_all":
      return "Close everything on this account";
    default: {
      const unhandled: never = kind;
      throw new Error(`unhandled command ${String(unhandled)}`);
    }
  }
}

export function stepUpLine(kind: CommandKind, label: string, positions: number): string {
  switch (kind) {
    case "pause":
      return `Pause ${label}: no new orders; resting protection stays.`;
    case "pause_all":
      return "Pause all agents on this account: no new orders; resting protection stays.";
    case "resume":
      return `Resume ${label}: it trades again within its mandate.`;
    case "stop":
      return `Stop ${label} for good. It holds nothing, so nothing is sold.`;
    case "kill":
      return `Kill switch for ${label}: cancel its orders, sell its positions, and end it.`;
    case "release":
      return `Stop ${label} and release ${positions} ${positions === 1 ? "position" : "positions"} to you, without protection.`;
    case "stop_all":
      return "Stop all agents on this account: each agent's kill switch. Your own holdings stay.";
    case "close_all":
      return "Close everything on this account: cancel every order and close every position, including ones no agent manages.";
    default: {
      const unhandled: never = kind;
      throw new Error(`unhandled command ${String(unhandled)}`);
    }
  }
}

export function recordedLine(kind: CommandKind, label: string): string {
  switch (kind) {
    case "pause":
      return `${label} is paused.`;
    case "pause_all":
      return "Every agent on this account is paused.";
    case "resume":
      return `${label} is trading again.`;
    case "stop":
      return `${label} is stopped.`;
    case "kill":
      return `${label}: orders canceled, positions sold, agent stopped.`;
    case "release":
      return `${label} is stopped; its positions are yours and unprotected.`;
    case "stop_all":
      return "Every agent on this account is stopped by its kill switch.";
    case "close_all":
      return "Every order on this account is canceled, every position closed, and every agent stopped.";
    default: {
      const unhandled: never = kind;
      throw new Error(`unhandled command ${String(unhandled)}`);
    }
  }
}
