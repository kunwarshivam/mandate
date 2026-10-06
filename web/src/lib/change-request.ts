import { type EditPath, type Edits, fieldFor, readInput } from "./mandate-change";
import { changeLabel } from "./mandate-paths";

/**
 * A request to change a mandate's limits, read from the owner's words by fixed phrases (DEC-483),
 * so the same words always read the same way. It reads only what the Edit form edits, only as the
 * new value in figures, and leaves every check to the review it hands over to.
 */
export type ChangeReading =
  | { kind: "edits"; edits: Edits }
  /** What could not be read, in the owner's words: nothing is proposed until it can be. */
  | { kind: "unclear"; lines: string[] }
  | { kind: "not_here" };

type Target = EditPath | "position" | "quiet";

/** Most specific first: "max loss" is the lifetime limit, "daily loss" the day's. */
const PHRASES: ReadonlyArray<[RegExp, Target]> = [
  [/\btwo[- ]approvers?\b|\bsecond approver\b/, "/autonomy/approval/two_approver_above_usd"],
  [/\bquiet hours? (start|begin)s?\b|\bstart of (the )?quiet hours\b|\bquiet start\b/, "/notifications/quiet_hours/start"],
  [/\bquiet hours? ends?\b|\bend of (the )?quiet hours\b|\bquiet end\b/, "/notifications/quiet_hours/end"],
  [/\bquiet hours\b/, "quiet"],
  [/\bdaily loss\b|\bloss (a|per) day\b|\bday'?s loss\b/, "/risk/max_daily_loss"],
  [/\b(lifetime|total) loss\b|\bmax(imum)? loss\b/, "/capital/max_loss_from_allocation"],
  [/\borders (a|per) day\b|\bdaily orders\b|\border count\b|\bnumber of orders\b/, "/risk/max_orders_per_day"],
  [/\b(largest|biggest|max(imum)?) order\b|\border size\b|\border limit\b/, "/risk/max_order_usd"],
  [/\bshare of equity\b|\bposition share\b/, "/risk/max_position_fraction"],
  [/\b(largest|biggest|max(imum)?) position\b|\bposition (size|limit)\b/, "position"],
  [/\btotal holdings\b|\bgross( exposure)?\b|\bexposure\b/, "/risk/max_gross_exposure_usd"],
  [/\bprotective stop\b|\bstop[- ]?(distance|loss)\b|\bthe stop\b/, "/protection/stop_distance"],
  [/\bapproval (window|time(out)?)\b|\btimeout\b|\btime to (answer|approve)\b/, "/autonomy/approval/timeout_s"],
  [/\b(capital|allocation)\b/, "/capital/allocation_usd"],
];

/** Parts of a mandate a message or the Edit form does not change. */
const NOT_HERE = /\b(instruments?|universe|symbols?|tickers?|signal models?|models?|rules?|goal|environment|drawdown|ladder|connection|asset class(es)?)\b/;

const CHANGE_VERB = /\b(set|change|raise|lower|increase|decrease|reduce|cut|lift|drop|remove|clear|turn off)\b/;

const REMOVE = /\b(remove|drop|clear|turn off|no)\b/;

/** One request per clause: "and", "also", a semicolon, or a comma before words (not one inside 1,200). */
const CLAUSE = /;\s*|,\s+(?=\D)|\s+and\s+|\s+also\s+/;

const TRAILING = /[\s.!?]+$|\s+(please|thanks|thank you|now|instead|again|for me|(for|on|of) (agent\s*\d+|this agent|the agent|it))$/g;

/** What a value's own words add after the figure: the base is the field's, not the message's. */
const QUALIFIER = /\s+(of (its |the |my )?(equity|capital|allocation|the day'?s? (starting )?equity)|a day|per day|each day|below (the )?cost|orders?( a day| per day)?)$/;

export const CHANGE_EXAMPLE = "“set the largest order to $800”";

function targetOf(clause: string): Target | null {
  for (const [phrase, target] of PHRASES) if (phrase.test(clause)) return target;
  return null;
}

/** Whether the words name one of the limits a message can change. */
export function namesLimit(text: string): boolean {
  return PHRASES.some(([phrase]) => phrase.test(text));
}

/** Whether the words ask to change a limit at all: a limit named, with a verb or a new value. */
export function asksForChange(text: string): boolean {
  if (!namesLimit(text) && !(CHANGE_VERB.test(text) && NOT_HERE.test(text))) return false;
  return CHANGE_VERB.test(text) || /\bto\s+[$\d]/.test(text);
}

function strip(value: string): string {
  let v = value.trim();
  for (let before = ""; before !== v; ) {
    before = v;
    v = v.replace(TRAILING, "");
  }
  return v.trim();
}

/** The value after "to", in the field's unit as `readInput` reads it, or why it can't be. */
function valueFor(path: EditPath, raw: string): { ok: true; text: string } | { ok: false; error: string } {
  const v = raw
    .replace(QUALIFIER, "")
    .replace(/\s*(dollars?|usd)\b/, "")
    .replace(/\s*(percent|per cent)\b/, "%")
    .trim();
  if (!/\d/.test(v)) return { ok: false, error: "write the new value in figures, like $800 or 1.5%." };
  if (/^\$?\s?[\d,.]+\s?k$/.test(v)) return { ok: false, error: "write the whole amount, like 2,000." };
  if (fieldFor(path).unit !== "minutes") return { ok: true, text: v };
  const hours = /^(\d+)\s*(hours?|hrs?|h)$/.exec(v);
  if (hours) return { ok: true, text: String(Number(hours[1]) * 60) };
  const minutes = /^(\d+)\s*(minutes?|mins?|m)?$/.exec(v);
  if (minutes) return { ok: true, text: minutes[1] };
  return { ok: false, error: "write whole minutes, like 15 minutes." };
}

const TIME = /\b(\d{1,2}:\d{2})\b/g;

/**
 * The edits a message asks for, read clause by clause. `text` is the owner's words in lower case.
 * Each clause names one limit and its new value after "to"; a difference ("by $200"), a value in
 * words, or a limit named twice is asked about rather than guessed.
 */
export function readChange(text: string): ChangeReading {
  const clauses = text
    .replace(/(\d{1,2}:\d{2})\s+(and|until|-)\s+(\d{1,2}:\d{2})/g, "$1 to $3")
    .split(CLAUSE)
    .map(strip)
    .filter((c) => c.length > 0);
  const edits: Edits = {};
  const lines: string[] = [];
  const named = new Set<EditPath>();
  const put = (path: EditPath, raw: string) => {
    if (named.has(path)) {
      lines.push(`${changeLabel(path)} is named twice. Say it once, with the value you want.`);
      return;
    }
    named.add(path);
    const value = valueFor(path, raw);
    if (!value.ok) {
      lines.push(`${changeLabel(path)}: ${value.error}`);
      return;
    }
    const parsed = readInput(fieldFor(path), value.text);
    if (parsed.ok) edits[path] = parsed.value;
    else lines.push(`${changeLabel(path)}: ${parsed.error.charAt(0).toLowerCase()}${parsed.error.slice(1)}`);
  };

  for (const clause of clauses) {
    const target = targetOf(clause);
    if (target === null) {
      if (NOT_HERE.test(clause)) return { kind: "not_here" };
      lines.push(`I couldn't tell which limit “${clause}” is about.`);
      continue;
    }
    if (target === "/autonomy/approval/two_approver_above_usd" && REMOVE.test(clause) && !/\bto\s+[$\d]/.test(clause)) {
      if (named.has(target)) lines.push(`${changeLabel(target)} is named twice. Say it once, with the value you want.`);
      named.add(target);
      edits[target] = null;
      continue;
    }
    if (target === "quiet") {
      const times = clause.match(TIME) ?? [];
      if (times.length !== 2) {
        lines.push("Quiet hours: say both times on a 24-hour clock, like “quiet hours 22:00 to 07:00”.");
        continue;
      }
      put("/notifications/quiet_hours/start", times[0]);
      put("/notifications/quiet_hours/end", times[1]);
      continue;
    }
    const to = /^.*(?:\bto|\bat|=)\s+(.+)$/.exec(clause);
    if (!to) {
      const label = target === "position" ? "Largest position" : changeLabel(target);
      lines.push(
        /\bby\s+[$\d]/.test(clause)
          ? `${label}: say the new value rather than the difference, like “to $800”, not “by $200”.`
          : `${label}: say the new value after “to”, like ${CHANGE_EXAMPLE}.`,
      );
      continue;
    }
    const raw = to[1];
    if (target === "position") put(/%|percent/.test(raw) ? "/risk/max_position_fraction" : "/risk/max_position_usd", raw);
    else put(target, raw);
  }
  return lines.length > 0 ? { kind: "unclear", lines } : { kind: "edits", edits };
}
