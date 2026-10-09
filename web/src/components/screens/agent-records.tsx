"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { Shield } from "pixelarticons/react/Shield.js";
import { LinkButton } from "@cloudflare/kumo/components/button";
import { ChartSkeleton } from "@/components/charts/chart-parts";
import { PositionChart } from "@/components/charts/price-chart";
import { AsOf } from "@/components/domain/as-of";
import { SourceTag } from "@/components/domain/mode";
import { KEY } from "@/components/kumo/key";
import { Money, SignedMoney } from "@/components/domain/money";
import { FixtureTag, Placeholder } from "@/components/domain/placeholders";
import { OrdersTable, SendingStopped, protectionText } from "@/components/domain/positions";
import { Skeleton } from "@/components/domain/skeleton";
import type { Agent, Fill } from "@/fixtures/types";
import { dec, mul, toFixed } from "@/lib/decimal";
import { clock, dateLabel, price, quantity, usd, zoneLabel } from "@/lib/format";
import { CHECK_RESULT_LABEL, type GateCheck, gateChecks } from "@/lib/gate-checks";
import { actionSentence, isExit, verdictLabel } from "@/lib/gate-reasons";
import { ORDER_STATE_LABEL, PURPOSE_LABEL } from "@/lib/labels";
import { useRuntime } from "@/lib/mock-runtime";
import { type AnyOrder, allOrders, fillsOf, findOrder, isPast, lifecycle, orderSentence, realizedIn } from "@/lib/orders";
import { useCan } from "@/lib/roles";
import { type AgentRecord, agentHref, decisionHref, orderHref, positionHref } from "@/lib/screens";
import { cn } from "@/lib/utils";
import { AgentFrame, AgentNotFound, RecordNotFound, useAgent } from "./agent-frame";
import { ComingSoon } from "./coming-soon";
import { Panel, Section, WorkspaceGate } from "./common";

function stamp(iso: string): string {
  return `${dateLabel(iso)}, ${clock(iso)} ${zoneLabel(iso)}`;
}

function Facts({ children, className }: { children: ReactNode; className?: string }) {
  return <dl className={cn("grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-3", className)}>{children}</dl>;
}

function Fact({ term, children, wide }: { term: string; children: ReactNode; wide?: boolean }) {
  return (
    <div className={cn("grid content-start gap-0.5", wide && "col-span-2 sm:col-span-3")}>
      <dt className="field-label text-muted-foreground">{term}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function RelatedLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <li>
      <Link href={href} className="press -mx-3 flex min-h-12 items-center justify-between gap-3 rounded-xl px-3 py-3 font-medium outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset sm:px-4">
        {children}
        <ArrowRight className="size-6 shrink-0" aria-hidden />
      </Link>
    </li>
  );
}

const TH = "pt-1 pb-2 pr-3 text-label font-normal text-muted-foreground";

/** Executions as journaled, newest first, each linked to the order it filled. */
function FillsTable({ agent, fills, showOrder = true }: { agent: Agent; fills: Fill[]; showOrder?: boolean }) {
  if (fills.length === 0) return <p className="text-sm text-muted-foreground">No fills.</p>;
  const rows = [...fills].sort((a, b) => Date.parse(b.at) - Date.parse(a.at));
  return (
    <div className="relative -mx-3 overflow-x-auto px-3 sm:-mx-4 sm:px-4">
      <table className="w-full min-w-[30rem] text-sm" data-slot="fills">
        <caption className="sr-only">Fills</caption>
        <thead>
          <tr className="border-b border-border text-left">
            <th scope="col" className={TH}>Time</th>
            <th scope="col" className={TH}>Side</th>
            <th scope="col" className={cn(TH, "text-right")}>Quantity</th>
            <th scope="col" className={cn(TH, "text-right")}>Price</th>
            <th scope="col" className={cn(TH, showOrder ? "text-right" : "pr-0 text-right")}>Value</th>
            {showOrder ? <th scope="col" className={cn(TH, "pr-0 text-right")}>Order</th> : null}
          </tr>
        </thead>
        <tbody>
          {rows.map((f) => (
            <tr key={f.fill_id} className="border-b last:border-b-0">
              <td className="py-2.5 pr-3 font-mono text-caption tabular">
                <time dateTime={f.at}>{stamp(f.at)}</time>
              </td>
              <td className="py-2.5 pr-3 font-semibold">{f.side === "buy" ? "Buy" : "Sell"}</td>
              <td className="py-2.5 pr-3 text-right font-mono tabular">{quantity(f.qty)}</td>
              <td className="py-2.5 pr-3 text-right font-mono tabular">{price(f.price)}</td>
              <td className={cn("py-2.5 text-right font-mono tabular", showOrder && "pr-3")}>{usd(mul(dec(f.qty), dec(f.price)))}</td>
              {showOrder ? (
                <td className="py-2.5 text-right">
                  <Link href={orderHref(agent.agent_id, f.client_order_id)} className="font-semibold text-lapis underline decoration-lapis/30 underline-offset-4 hover:decoration-current">
                    View order
                  </Link>
                </td>
              ) : null}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function useMarketStale(agent: Agent, symbol: string): boolean {
  const { ws } = useRuntime();
  return ws.health.market_data.state !== "ok" || agent.restrictions.some((r) => r.code === "stale_mark" && r.symbol === symbol);
}

function PositionRecord({ agent, assetId }: { agent: Agent; assetId: string }) {
  const { now } = useRuntime();
  const canClose = useCan("stop.full");
  const position = agent.positions.find((p) => p.instrument.asset_id === assetId);
  const symbol = position?.instrument.symbol ?? "";
  const stale = useMarketStale(agent, symbol);
  if (!position) {
    return (
      <AgentFrame agent={agent} title="Position" description={agent.label}>
        <RecordNotFound
          title="No open position here"
          text="This agent holds nothing in that instrument now. If it sold, the fills and orders are still in its orders."
          back={{ href: agentHref(agent.agent_id, "positions"), label: "Back to positions" }}
        />
      </AgentFrame>
    );
  }
  const legs = allOrders(agent).filter((o) => o.instrument.asset_id === assetId && o.purpose === "protective" && !isPast(o));
  const fills = agent.fills.filter((f) => f.instrument.asset_id === assetId);
  const realized = realizedIn(agent, symbol);
  const unprotected = position.protection.kind === "none";

  return (
    <AgentFrame
      agent={agent}
      title={`${symbol} position`}
      description={
        <>
          {agent.label}, {agent.mandate.name}
        </>
      }
    >
      <section aria-label="Figures" className="reveal grid gap-3">
        <Facts>
          <Fact term="Quantity">
            <span className="font-mono text-lg font-semibold tabular">{quantity(position.qty)}</span>
          </Fact>
          <Fact term="Average cost">
            <span className="font-mono tabular">{price(position.avg_cost)}</span>
          </Fact>
          <Fact term="Mark">
            <span className="font-mono tabular">{price(position.mark)}</span>
            <span className="block">
              <AsOf at={position.mark_as_of} now={now} stale={stale} />
            </span>
          </Fact>
          <Fact term="Value">
            <Money value={position.market_value} />
          </Fact>
          <Fact term="Unrealized, simulated">
            <SignedMoney value={position.unrealized_pnl} className="font-semibold" />
          </Fact>
          <Fact term="Realized here, simulated">
            <SignedMoney value={toFixed(realized, 2)} className="font-semibold" />
          </Fact>
        </Facts>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <span className="flex flex-wrap items-center gap-2">
            <FixtureTag />
            <Placeholder name="performance" />
          </span>
          {canClose ? (
            <LinkButton href={`${positionHref(agent.agent_id, assetId)}/close`} variant="secondary" size="lg" className={KEY}>
              Close position…
            </LinkButton>
          ) : null}
        </div>
      </section>

      <Section title="Price">
        <Panel>
          <PositionChart agent={agent} position={position} />
        </Panel>
      </Section>

      <div className="grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
        <Section title="Protection">
          <p className={cn("flex items-start gap-2 rounded-xl px-4 py-3 text-sm", unprotected ? "bg-muted font-medium" : "bg-mandate-soft")} data-slot="protection">
            <Shield className="size-6 shrink-0" aria-hidden />
            {protectionText(position)}
          </p>
          <OrdersTable orders={legs} hrefFor={(o) => orderHref(agent.agent_id, o.client_order_id)} empty="No protective order is resting at the broker." />
        </Section>

        <Section title="Fills">
          <Panel>
            <FillsTable agent={agent} fills={fills} />
          </Panel>
        </Section>
      </div>
    </AgentFrame>
  );
}

function ClosePositionRecord({ agent, assetId }: { agent: Agent; assetId: string }) {
  const position = agent.positions.find((p) => p.instrument.asset_id === assetId);
  return (
    <AgentFrame agent={agent} title={position ? `Close ${position.instrument.symbol} position` : "Close position"} description={agent.label}>
      <ComingSoon
        purpose="Sell this position yourself, as an owner exit. It will say what is canceled and what is sold before anything is sent, and it will ask for your passkey."
        back={{ href: positionHref(agent.agent_id, assetId), label: "Back to the position" }}
      />
    </AgentFrame>
  );
}

function OrderRecord({ agent, id }: { agent: Agent; id: string }) {
  const { ws } = useRuntime();
  const order = findOrder(agent, id);
  if (!order) {
    return (
      <AgentFrame agent={agent} title="Order" description={agent.label}>
        <RecordNotFound title="No order with this ID" text="This agent has no order with that ID." back={{ href: agentHref(agent.agent_id, "orders"), label: "Back to orders" }} />
      </AgentFrame>
    );
  }
  const fills = fillsOf(agent, id);
  const steps = lifecycle(order, fills);
  const unknown = order.state === "Unknown";
  const decision = ws.decisions.find((d) => d.client_order_id === id);
  const held = agent.positions.find((p) => p.instrument.asset_id === order.instrument.asset_id);
  const filled = fills.reduce((sum, f) => sum + dec(f.qty), 0n);

  return (
    <AgentFrame
      agent={agent}
      title="Order"
      description={
        <>
          {orderSentence(order)}, {agent.label}
        </>
      }
    >
      <section aria-label="Order" data-state={order.state} className={cn("reveal grid gap-3 px-3 py-3 sm:px-4 sm:py-4", unknown ? "bg-lapis-soft" : "bg-card")}>
        <p className="flex flex-wrap items-center gap-2">
          <span data-slot="order-state" className={cn("inline-flex h-7 items-center rounded-md px-2.5 text-label", unknown ? "bg-card ring-1 ring-foreground ring-inset" : "bg-muted")}>
            {ORDER_STATE_LABEL[order.state]}
          </span>
          {unknown ? <SourceTag source="account" /> : null}
          {unknown ? <SendingStopped symbol={order.instrument.symbol} /> : null}
        </p>
        {unknown ? (
          <p className="max-w-measure text-sm" data-slot="unknown-order">
            The broker has not answered for this order, so its state is unknown. Nothing else is sent in {order.instrument.symbol}, exits included, until the broker answers;
            the order counts as filled against every limit meanwhile. The kill switch still works.
          </p>
        ) : null}
        <Facts>
          <Fact term="Side">{order.side === "buy" ? "Buy" : "Sell"}</Fact>
          <Fact term="Quantity">
            <span className="font-mono tabular">{quantity(order.qty)}</span>
          </Fact>
          <Fact term="Filled">
            <span className="font-mono tabular">{unknown ? "Unknown" : quantity(toFixed(filled, 8))}</span>
          </Fact>
          <Fact term="Limit">
            <span className="font-mono tabular">{order.limit_price ? price(order.limit_price) : "None"}</span>
          </Fact>
          <Fact term="Stop">
            <span className="font-mono tabular">{order.stop_price ? price(order.stop_price) : "None"}</span>
          </Fact>
          <Fact term="Purpose">{PURPOSE_LABEL[order.purpose]}</Fact>
          <Fact term="Time in force">{order.time_in_force === "gtc" ? "Good until canceled" : "Day"}</Fact>
          <Fact term="Sent">
            <time dateTime={order.submitted_at} className="font-mono text-sm tabular">
              {stamp(order.submitted_at)}
            </time>
          </Fact>
          {isPast(order) ? (
            <Fact term="Closed">
              <time dateTime={order.closed_at} className="font-mono text-sm tabular">
                {stamp(order.closed_at)}
              </time>
            </Fact>
          ) : null}
          <Fact term="Order ID" wide>
            <span className="font-mono text-caption break-all" translate="no">
              {order.client_order_id}
            </span>
          </Fact>
        </Facts>
      </section>

      <div className="grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[minmax(0,7fr)_minmax(0,5fr)]">
        <Section title="Lifecycle">
          <Panel>
            <ol className="grid gap-4 border-l border-border pl-5" data-slot="lifecycle">
              {steps.map((s, i) => (
                <li key={`${s.state}-${i}`} data-step={s.state} className="relative grid gap-0.5">
                  <span className="absolute top-1.5 -left-[1.5625rem] size-2 rounded-full bg-muted-foreground ring-4 ring-card" aria-hidden />
                  <p className="flex flex-wrap items-baseline gap-x-2 text-caption text-muted-foreground">
                    <time dateTime={s.at} className="font-mono tabular">
                      {stamp(s.at)}
                    </time>
                    <span className="text-label text-foreground">{ORDER_STATE_LABEL[s.state]}</span>
                  </p>
                  <p className="text-sm">{s.text}</p>
                </li>
              ))}
            </ol>
          </Panel>
        </Section>

        <div className="grid grid-cols-1 content-start gap-(--section-gap)">
          <Section title="Fills">
            <Panel>
              <FillsTable agent={agent} fills={fills} showOrder={false} />
            </Panel>
          </Section>
          {decision || held ? (
            <Section title="Related">
              <ul className="grid divide-y divide-border/70">
                {decision ? <RelatedLink href={decisionHref(agent.agent_id, decision.event_id)}>The gate decision that allowed it</RelatedLink> : null}
                {held ? <RelatedLink href={positionHref(agent.agent_id, held.instrument.asset_id)}>{`The ${held.instrument.symbol} position`}</RelatedLink> : null}
              </ul>
            </Section>
          ) : null}
        </div>
      </div>
    </AgentFrame>
  );
}

const CHECK_CHIP: Record<GateCheck["result"], string> = {
  passed: "bg-muted text-foreground",
  failed: "bg-card text-foreground ring-1 ring-foreground ring-inset",
  waiting: "bg-card text-foreground ring-1 ring-foreground ring-inset",
  not_run: "border border-dashed border-muted-foreground text-muted-foreground",
};

function DecisionRecord({ agent, id }: { agent: Agent; id: string }) {
  const { ws } = useRuntime();
  const decision = ws.decisions.find((d) => d.event_id === id && d.agent_id === agent.agent_id);
  if (!decision) {
    return (
      <AgentFrame agent={agent} title="Gate decision" description={agent.label}>
        <RecordNotFound
          title="No gate decision with this ID"
          text="This agent has no gate decision with that ID."
          back={{ href: agentHref(agent.agent_id, "decisions"), label: "Back to decisions" }}
        />
      </AgentFrame>
    );
  }
  const checks = gateChecks(decision, agent);
  const exit = isExit(decision.action.purpose);
  const order: AnyOrder | undefined = decision.client_order_id ? findOrder(agent, decision.client_order_id) : undefined;
  const resultLabel = (c: GateCheck) => (c.result === "failed" && exit ? "Held" : CHECK_RESULT_LABEL[c.result]);

  return (
    <AgentFrame
      agent={agent}
      title="Gate decision"
      description={
        <>
          {actionSentence(decision.action)}, {agent.label}
        </>
      }
    >
      <section aria-label="Verdict" data-verdict={decision.verdict} className="reveal grid gap-2">
        <p className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
          <span
            data-slot="verdict"
            className={cn("inline-flex h-7 items-center rounded-md px-2.5 text-label", decision.verdict === "allow" ? "bg-muted" : "bg-card ring-1 ring-foreground ring-inset")}
          >
            {verdictLabel(decision)}
          </span>
          <span className="font-semibold">{actionSentence(decision.action)}</span>
          <span className="text-sm text-muted-foreground">{PURPOSE_LABEL[decision.action.purpose]}</span>
        </p>
        <p className="text-caption text-muted-foreground">
          <time dateTime={decision.at} className="font-mono tabular">
            {stamp(decision.at)}
          </time>
        </p>
        {decision.then ? <p className="max-w-measure text-sm">{decision.then}</p> : null}
      </section>

      <div className="grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[minmax(0,7fr)_minmax(0,5fr)]">
        <Section title="Checks, in the order the gate ran them">
          <ol className="grid divide-y divide-border/70" data-slot="gate-checks">
            {checks.map((c) => (
              <li key={c.key} data-check={c.key} data-result={c.result} className="grid gap-1 py-3">
                <p className="flex flex-wrap items-center justify-between gap-2">
                  <span className={cn("font-semibold", c.result === "not_run" && "text-muted-foreground")}>{c.label}</span>
                  <span className={cn("inline-flex h-6 items-center rounded-md px-2.5 text-label", CHECK_CHIP[c.result])}>{resultLabel(c)}</span>
                </p>
                <p className="text-sm text-muted-foreground">{c.rule}</p>
              </li>
            ))}
          </ol>
          {checks.some((c) => c.result === "not_run") ? <p className="text-caption text-muted-foreground">The gate stops at the first check that does not pass; later checks did not run.</p> : null}
        </Section>

        {order || decision.approval_id ? (
          <Section title="What it led to">
            <ul className="grid divide-y divide-border/70">
              {order ? <RelatedLink href={orderHref(agent.agent_id, order.client_order_id)}>{`The order: ${orderSentence(order)}`}</RelatedLink> : null}
              {decision.approval_id ? <RelatedLink href={`/approvals/${decision.approval_id}`}>The request it sent you</RelatedLink> : null}
            </ul>
          </Section>
        ) : null}
      </div>
    </AgentFrame>
  );
}

function RecordSkeleton() {
  return (
    <div className="grid gap-(--section-gap)" aria-busy="true" aria-label="Loading" data-slot="skeleton">
      <Skeleton className="h-28" />
      <div>
        <ChartSkeleton height={320} label="Loading the price chart" />
      </div>
      <Skeleton className="h-40" />
      <span className="sr-only">Loading from your deployment. Values appear once they arrive; nothing from an earlier visit is shown.</span>
    </div>
  );
}

function AgentRecordBody({ agentId, record }: { agentId: string; record: AgentRecord }) {
  const agent = useAgent(agentId);
  if (!agent) return <AgentNotFound />;
  switch (record.kind) {
    case "position":
      return <PositionRecord agent={agent} assetId={record.id} />;
    case "close-position":
      return <ClosePositionRecord agent={agent} assetId={record.id} />;
    case "order":
      return <OrderRecord agent={agent} id={record.id} />;
    case "decision":
      return <DecisionRecord agent={agent} id={record.id} />;
    default: {
      const unhandled: never = record;
      throw new Error(`unhandled record ${JSON.stringify(unhandled)}`);
    }
  }
}

export function AgentRecordScreen({ agentId, record }: { agentId: string; record: AgentRecord }) {
  return (
    <WorkspaceGate skeleton={<RecordSkeleton />}>
      <AgentRecordBody agentId={agentId} record={record} />
    </WorkspaceGate>
  );
}
