"use client";

import Link from "next/link";
import { AiScan } from "pixelarticons/react/AiScan.js";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { AvatarCircle } from "pixelarticons/react/AvatarCircle.js";
import { Script } from "pixelarticons/react/Script.js";
import { Shield } from "pixelarticons/react/Shield.js";
import { Sliders } from "pixelarticons/react/Sliders.js";
import { Markdown } from "@/components/chat/markdown";
import type { Icon } from "@/components/icon";
import type { Agent, Iso } from "@/fixtures/types";
import { clock } from "@/lib/format";
import { type DeskRole, type DeskStep, deskFor, figureLine, stamp } from "@/lib/messages";
import { useRuntime } from "@/lib/mock-runtime";

const ROLE_ICON: Record<DeskRole, Icon> = {
  research: AiScan,
  trader: Sliders,
  risk: Shield,
  autonomy: Script,
  you: AvatarCircle,
};

const LINK_LABEL: Record<DeskRole, string> = {
  research: "See the model output",
  trader: "See the order",
  risk: "See the gate decision",
  autonomy: "See the rule in the mandate",
  you: "Open the request",
};

function Step({ step, now, last }: { step: DeskStep; now: Iso; last: boolean }) {
  const Glyph = ROLE_ICON[step.role];
  return (
    <li data-slot="desk-step" data-role={step.role} className="relative grid grid-cols-[3rem_minmax(0,1fr)] gap-x-3">
      <span className="relative grid justify-items-center">
        <span className="grid size-12 place-items-center rounded-2xl bg-background">
          <Glyph aria-hidden className="size-6" />
        </span>
        {last ? null : <span aria-hidden className="absolute top-12 bottom-0 w-0.5 bg-border" />}
      </span>
      <div className="grid content-start gap-2 pt-3 pb-8">
        <p className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
          <span className="font-semibold">{step.title}</span>
          {step.at ? (
            <time dateTime={step.at} className="font-mono text-caption text-muted-foreground tabular">
              {stamp(step.at, now)}
            </time>
          ) : (
            <span className="text-caption text-muted-foreground">Time not recorded</span>
          )}
        </p>
        {step.lines.map((line, i) => (
          <p key={i} className="text-pretty">
            {line}
          </p>
        ))}
        {step.quotes.map((q) => (
          <figure key={`${q.model_id}-${q.produced_at}`} data-slot="model-quote" className="grid gap-1.5 rounded-xl border border-dashed border-foreground/30 px-4 py-3">
            <figcaption className="flex flex-wrap items-center gap-x-2 gap-y-1 text-caption text-muted-foreground">
              <span className="inline-flex h-6 items-center rounded-md bg-background px-2 text-label text-foreground">Model output</span>
              <span translate="no" className="font-mono">
                {q.model_id} {q.version}
              </span>
              <time dateTime={q.produced_at} className="font-mono tabular">
                {clock(q.produced_at)}
              </time>
            </figcaption>
            <blockquote className="font-mono text-sm">
              <Markdown text={q.lines.join("\n")} links="show" />
            </blockquote>
          </figure>
        ))}
        {step.figures.length > 0 ? (
          <ul className="grid gap-0.5 text-sm">
            {step.figures.map((f) => (
              <li key={f.field} className="tabular">
                {figureLine(f)}
              </li>
            ))}
          </ul>
        ) : null}
        {step.href ? (
          <Link href={step.href} className="-mx-2 inline-flex min-h-9 w-fit items-center gap-1 rounded-md px-2 text-sm font-medium text-lapis outline-none hover:bg-lapis-soft focus-visible:ring-2 focus-visible:ring-ring max-lg:min-h-11">
            {LINK_LABEL[step.role]}
            <ArrowRight aria-hidden className="-my-1 size-6" />
          </Link>
        ) : null}
      </div>
    </li>
  );
}

/**
 * The desk (DEC-479): how the agent's latest request was made, from the models to the owner. Each
 * step is a journaled fact; model output is quoted in a dashed frame and named, its Markdown drawn
 * and its links shown but never opened (DEC-481), and nothing on the desk is in the agent's voice.
 */
export function ThreadDesk({ agent }: { agent: Agent }) {
  const { ws, now } = useRuntime();
  const desk = deskFor(ws, agent.agent_id, now);
  return (
    <div className="grid content-start gap-5" data-slot="thread-desk">
      {desk ? (
        <>
          <h3 className="text-h3 text-pretty">
            {desk.subject === "request" ? "Latest request: " : "Latest gate decision: "}
            {desk.title}
          </h3>
          <ol className="grid" aria-label="Steps">
            {desk.steps.map((step, i) => (
              <Step key={step.role} step={step} now={now} last={i === desk.steps.length - 1} />
            ))}
          </ol>
        </>
      ) : (
        <p className="text-muted-foreground">Nothing is on the desk yet: {agent.label} has made no request and the gate has decided nothing for it.</p>
      )}
    </div>
  );
}
