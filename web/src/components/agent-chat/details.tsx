"use client";

import { type ReactNode, useEffect, useState } from "react";
import { Dialog } from "@cloudflare/kumo/primitives/dialog";
import { Close } from "pixelarticons/react/Close.js";
import { type Goals, summaryLines } from "@/api/chat";
import type { AgentHolding } from "@/api/activity";
import type { Mandate } from "@/fixtures/types";
import { clockShort, price, quantity, usd } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";
import { OwnerExit } from "./owner-exit";

type Line = { label: string; value: string };
type Row = { key: string; text: string; at?: string; holding?: AgentHolding };

/** What the drawer reads for one agent, each list as served or why not; a list not read is never shown as empty. */
interface Read {
  positions: Row[] | string;
  activity: Row[] | string;
  mandate: Line[] | string;
  goals: Goals | string;
}

/** The fixture mandate's envelope, in the lines a confirm card shows. */
function fixtureMandate(mandate: Mandate): Line[] {
  return summaryLines({
    allocation_usd: mandate.capital.allocation_usd,
    goal: mandate.goal.type,
    max_position_usd: mandate.risk.max_position_usd,
    max_order_usd: mandate.risk.max_order_usd,
    max_orders_per_day: mandate.risk.max_orders_per_day,
    max_daily_loss: mandate.risk.max_daily_loss,
    max_drawdown: mandate.risk.max_drawdown,
    max_loss_from_allocation: mandate.capital.max_loss_from_allocation,
    asset_classes: mandate.universe.asset_classes,
  });
}

const NOT_READ = "Your deployment didn't report this. It is read again when you reopen Details.";

/** The drawer's reads: the fixture agent's own lists, or the API's activity, agent and goals reads. */
function useDetails(agentId: string, open: boolean): Read | null {
  const { source, ws, view, activity, chat } = useRuntime();
  const [read, setRead] = useState<Read | null>(null);
  useEffect(() => {
    if (!open) return;
    let live = true;
    void (async () => {
      const goals = await chat.goals(agentId);
      const goalsRead: Goals | string = goals.ok ? goals.value : NOT_READ;
      if (source === "fixtures" || !activity) {
        const agent = ws.agents.find((a) => a.agent_id === agentId);
        if (!live) return;
        setRead({
          positions: agent ? agent.positions.map((p) => ({ key: p.instrument.symbol, text: `${quantity(p.qty)} ${p.instrument.symbol} · ${usd(p.market_value)} at ${price(p.mark)}`, at: p.mark_as_of })) : NOT_READ,
          activity: agent ? [...agent.orders, ...agent.past_orders].map((o) => ({ key: o.client_order_id, text: `${o.side === "sell" ? "Sell" : "Buy"} ${quantity(o.qty)} ${o.instrument.symbol} · ${o.state}`, at: o.submitted_at })).reverse() : NOT_READ,
          mandate: agent ? fixtureMandate(agent.mandate) : NOT_READ,
          goals: goalsRead,
        });
        return;
      }
      const answer = await activity(agentId);
      if (!live) return;
      const detail = view?.agents.find((a) => a.agent_id === agentId);
      setRead({
        positions: answer.holdings.ok
          ? answer.holdings.items.map((h) => ({ key: h.instrument_id, text: `${quantity(h.filled_qty)} of ${h.instrument_id}${h.market_value ? ` · ${usd(h.market_value)}` : ""}`, at: h.risk_mark?.marked_at, holding: h }))
          : NOT_READ,
        activity: answer.orders.ok ? [...answer.orders.items].reverse().map((o) => ({ key: o.client_order_id, text: `${o.side === "sell" ? "Sell" : "Buy"} ${quantity(o.qty)} of ${o.instrument_id} · ${o.state ?? "sent"}`, at: o.submitted_at })) : NOT_READ,
        mandate: detail?.mandate ? summaryLines(detail.mandate as unknown as Record<string, unknown>) : NOT_READ,
        goals: goalsRead,
      });
    })();
    return () => {
      live = false;
    };
  }, [agentId, open, source, ws, view, activity, chat]);
  return read;
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section aria-label={title} className="grid gap-2 border-b border-border/70 px-5 py-4 last:border-b-0">
      <h3 className="text-label text-muted-foreground">{title}</h3>
      {children}
    </section>
  );
}

function Rows({ rows, none, agentId }: { rows: Row[] | string; none: string; agentId?: string }) {
  if (typeof rows === "string") return <p className="text-sm text-muted-foreground">{rows}</p>;
  if (rows.length === 0) return <p className="text-sm text-muted-foreground">{none}</p>;
  return (
    <ul className="grid gap-1.5">
      {rows.map((r) => (
        <li key={r.key} className="grid gap-1.5">
          <span className="flex flex-wrap justify-between gap-x-3 text-sm tabular">
            <span className="min-w-0 wrap-anywhere">{r.text}</span>
            {r.at ? (
              <time dateTime={r.at} className="text-muted-foreground">
                {clockShort(r.at)}
              </time>
            ) : null}
          </span>
          {r.holding && agentId ? <OwnerExit agentId={agentId} holding={r.holding} /> : null}
        </li>
      ))}
    </ul>
  );
}

/**
 * Details, read only (DEC-2293): what the agent holds, what it is working on, its mandate in plain
 * words, and its orders. The mandate changes only by asking the agent, which drafts a version the
 * owner confirms.
 */
export function DetailsDrawer({ agentId, open, onOpenChange }: { agentId: string; open: boolean; onOpenChange: (open: boolean) => void }) {
  const read = useDetails(agentId, open);
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-ink/40" />
        <Dialog.Popup data-slot="details-drawer" className="fixed inset-y-0 right-0 z-50 flex w-full flex-col overflow-y-auto overscroll-contain border-l border-border bg-card text-foreground shadow-2xl outline-none sm:max-w-md sm:rounded-l-3xl">
          <div className="flex items-center justify-between gap-3 border-b border-border/70 px-5 pt-6 pb-4">
            <Dialog.Title className="text-h2">Details</Dialog.Title>
            <Dialog.Close aria-label="Close details" className="press inline-flex size-11 items-center justify-center rounded-full outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring">
              <Close className="size-6" aria-hidden />
            </Dialog.Close>
          </div>
          {read === null ? (
            <p className="px-5 py-4 text-muted-foreground">Reading…</p>
          ) : (
            <>
              <Section title="Positions">
                <Rows rows={read.positions} none="Holds nothing." agentId={agentId} />
              </Section>
              <Section title="Goals">
                {typeof read.goals === "string" ? (
                  <p className="text-sm text-muted-foreground">{read.goals}</p>
                ) : (
                  <div className="grid gap-2 text-sm">
                    {read.goals.goals.length === 0 ? <p className="text-muted-foreground">No goals yet. Tell your agent what you want it to work on.</p> : null}
                    <ul className="grid gap-1">
                      {read.goals.goals.map((g) => (
                        <li key={g.text} data-status={g.status} className={g.status === "done" ? "text-muted-foreground line-through" : undefined}>
                          {g.text}
                        </li>
                      ))}
                    </ul>
                    {read.goals.plan ? <p className="text-pretty">{read.goals.plan}</p> : null}
                    {read.goals.watching.length > 0 ? <p className="text-muted-foreground">Watching {read.goals.watching.join(", ")}</p> : null}
                  </div>
                )}
              </Section>
              <Section title="Mandate">
                {typeof read.mandate === "string" ? (
                  <p className="text-sm text-muted-foreground">{read.mandate}</p>
                ) : (
                  <dl className="grid gap-1">
                    {read.mandate.map((l) => (
                      <div key={l.label} className="flex flex-wrap justify-between gap-x-4 text-sm">
                        <dt className="text-muted-foreground">{l.label}</dt>
                        <dd className="tabular">{l.value}</dd>
                      </div>
                    ))}
                  </dl>
                )}
                <p className="text-caption text-muted-foreground">To change it, ask your agent. It drafts a new version for you to confirm.</p>
              </Section>
              <Section title="Activity">
                <Rows rows={read.activity} none="No orders yet." />
              </Section>
            </>
          )}
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
