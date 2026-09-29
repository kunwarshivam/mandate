"use client";

import { useState } from "react";
import { cn } from "@/lib/utils";
import { BOLD, MONO, PIXEL, PLAIN_BUTTON, RAISED } from "./letter";

type Entry = { time: string; kind: string; text: string; hash: string };

export const TRACE: Entry[] = [
  { time: "09:41:02", kind: "Read", text: "XYZ quarterly filing: capex guidance raised a second time", hash: "9f2c41ab" },
  { time: "09:41:09", kind: "Idea", text: "Buy XYZ and hold about six weeks. Wrong if guidance is cut", hash: "3e07d9c5" },
  { time: "09:41:09", kind: "Sized", text: "12 shares at a limit of $141.30, stop at $132.80", hash: "b81a60f2" },
  { time: "09:41:09", kind: "Checked", text: "Inside your mandate. $1,695.60 of $2,000 per order", hash: "52cd0e97" },
  { time: "09:41:10", kind: "Asked you", text: "Your rule: ask before a stock it hasn't held", hash: "e4f3a118" },
  { time: "09:43:27", kind: "Approved", text: "By you, with your passkey", hash: "07b95dce" },
  { time: "09:43:28", kind: "Checked again", text: "Price and limits still inside. Sent", hash: "a4d1c93e" },
  { time: "09:43:29", kind: "Filled", text: "12 XYZ at $141.27", hash: "c6a2f40d" },
];

/** The line the demo edits and what it's changed to. */
export const EDITED = { index: 2, text: "40 shares at a limit of $141.30, stop at $132.80" };

const CELL = "border border-t-foreground/40 border-l-foreground/40 border-r-card border-b-card px-2 py-1 align-top";
const HEAD = `border border-foreground bg-foreground px-2 py-1 text-start text-[0.9375rem] font-medium text-card ${PIXEL}`;

/**
 * An example decision as the record keeps it, and a way to see why it can be trusted: edit one line
 * and its hash stops matching, and so does every line chained after it.
 */
export function RecordTrace() {
  const [edited, setEdited] = useState(false);
  const line = EDITED.index + 1;

  return (
    <div className="grid gap-4" data-slot="record-trace" data-edited={edited || undefined}>
      <div className="overflow-x-auto">
        <table className={cn(RAISED, "w-full min-w-[36rem] border-separate border-spacing-[2px] bg-card text-[1rem] leading-snug")}>
          <caption className="pb-2 text-start">Agent 2, one decision. Example data.</caption>
          <thead>
            <tr>
              <th scope="col" className={HEAD}>
                #
              </th>
              <th scope="col" className={HEAD}>
                Time
              </th>
              <th scope="col" className={HEAD}>
                Step
              </th>
              <th scope="col" className={HEAD}>
                What happened
              </th>
              <th scope="col" className={HEAD}>
                Hash
              </th>
            </tr>
          </thead>
          <tbody>
            {TRACE.map((e, i) => {
              const changed = edited && i === EDITED.index;
              const broken = edited && i >= EDITED.index;
              return (
                <tr key={e.hash} className={cn(changed && "bg-warning-soft")}>
                  <td className={cn(CELL, "tabular-nums text-muted-foreground")}>{i + 1}</td>
                  <td className={cn(CELL, MONO, "text-lg leading-tight whitespace-nowrap")}>{e.time}</td>
                  <td className={cn(CELL, "whitespace-nowrap")}>{e.kind}</td>
                  <td className={CELL}>{changed ? EDITED.text : e.text}</td>
                  <td className={cn(CELL, MONO, "text-lg leading-tight whitespace-nowrap")}>
                    <span className={cn(broken && "line-through")}>{e.hash}</span>
                    {broken && <span className="block">{changed ? "edited" : "no match"}</span>}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <p role="status" className={cn(MONO, "text-xl leading-snug")}>
        <strong className={cn(BOLD, "text-base")}>Chain check:</strong>{" "}
        {edited ? `fails at line ${line}. Lines ${line} to ${TRACE.length} no longer match the lines before them.` : `all ${TRACE.length} lines match.`}
      </p>
      <p>
        <button type="button" onClick={() => setEdited((v) => !v)} aria-pressed={edited} className={PLAIN_BUTTON}>
          {edited ? "Undo the edit" : `Edit line ${line}`}
        </button>
      </p>
    </div>
  );
}
