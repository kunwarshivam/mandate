"use client";

import { useState } from "react";
import { cn } from "@/lib/utils";
import { BOLD, MONO, PIXEL, PLAIN_BUTTON, RAISED } from "./letter";

type Entry = { time: string; kind: string; text: string };

export const TRACE: Entry[] = [
  { time: "09:41:02", kind: "Read", text: "XYZ's quarterly filing. It raised its spending plans for the second time" },
  { time: "09:41:09", kind: "Idea", text: "Buy XYZ and hold about six weeks. Wrong if the plans are cut" },
  { time: "09:41:09", kind: "Sized", text: "12 shares at a limit of $141.30, with a stop at $132.80" },
  { time: "09:41:09", kind: "Checked", text: "Inside your rules: $1,695.60 of the $2,000 allowed per order" },
  { time: "09:41:10", kind: "Asked you", text: "Your rule: ask before buying a stock it hasn't held" },
  { time: "09:43:27", kind: "Approved", text: "By you, with your passkey" },
  { time: "09:43:28", kind: "Checked again", text: "Price and limits still inside your rules. Sent" },
  { time: "09:43:29", kind: "Filled", text: "12 XYZ at $141.27" },
];

/** The line the demo edits and what it's changed to. */
export const EDITED = { index: 2, text: "40 shares at a limit of $141.30, with a stop at $132.80" };

const CELL = "border border-t-foreground/40 border-l-foreground/40 border-r-card border-b-card px-2 py-1 align-top";
const HEAD = `border border-foreground bg-foreground px-2 py-1 text-start text-[0.9375rem] font-medium text-card ${PIXEL}`;

/** On a phone the time moves under the step, so the table fits without scrolling sideways. */
const NARROW_HIDDEN = "max-sm:hidden";

/**
 * One example decision as the record keeps it, step by step, so a visitor sees why the order went
 * out and who said yes. Under it, why the record can be trusted: edit one line and the chain stops
 * matching from that line on. The seal column appears only while a line is edited.
 */
export function RecordTrace() {
  const [edited, setEdited] = useState(false);
  const line = EDITED.index + 1;

  return (
    <div className="grid gap-5" data-slot="record-trace" data-edited={edited || undefined}>
      <div className="overflow-x-auto">
        <table className={cn(RAISED, "w-full border-separate border-spacing-[2px] bg-card text-[1rem] leading-snug sm:min-w-[36rem]")}>
          <caption className="pb-2 text-start">Agent 2 buys XYZ. Example data.</caption>
          <thead>
            <tr>
              <th scope="col" className={HEAD}>
                #
              </th>
              <th scope="col" className={cn(HEAD, NARROW_HIDDEN)}>
                Time
              </th>
              <th scope="col" className={HEAD}>
                Step
              </th>
              <th scope="col" className={HEAD}>
                What happened
              </th>
              {edited && (
                <th scope="col" className={HEAD}>
                  Seal
                </th>
              )}
            </tr>
          </thead>
          <tbody>
            {TRACE.map((e, i) => {
              const changed = edited && i === EDITED.index;
              return (
                <tr key={`${e.time}-${e.kind}`} className={cn(changed && "bg-warning-soft")}>
                  <td className={cn(CELL, "tabular-nums text-muted-foreground")}>{i + 1}</td>
                  <td className={cn(CELL, NARROW_HIDDEN, MONO, "text-lg leading-tight whitespace-nowrap")}>{e.time}</td>
                  <td className={cn(CELL, "sm:whitespace-nowrap")}>
                    {e.kind}
                    <span className={cn(MONO, "block text-lg leading-tight text-muted-foreground sm:hidden")}>{e.time}</span>
                  </td>
                  <td className={CELL}>{changed ? EDITED.text : e.text}</td>
                  {edited && <td className={cn(CELL, MONO, "text-lg leading-tight whitespace-nowrap")}>{changed ? "edited" : i > EDITED.index ? "no match" : "matches"}</td>}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <section aria-labelledby="record-seal" className="grid gap-3">
        <h3 id="record-seal" className={cn(BOLD, "text-lg")}>
          Can anyone change it afterwards?
        </h3>
        <p>Not without it showing. Each line is sealed with a hash of the line before it, so a changed line breaks its own seal and every seal after it. Try it.</p>
        <p role="status" className={cn(MONO, "text-xl leading-snug")}>
          <strong className={cn(BOLD, "text-base")}>Chain check:</strong>{" "}
          {edited ? `fails at line ${line}. Lines ${line} to ${TRACE.length} no longer match the lines before them.` : `all ${TRACE.length} lines match.`}
        </p>
        <p>
          <button type="button" onClick={() => setEdited((v) => !v)} aria-pressed={edited} className={PLAIN_BUTTON}>
            {edited ? "Undo the edit" : `Edit line ${line}`}
          </button>
        </p>
      </section>
    </div>
  );
}
