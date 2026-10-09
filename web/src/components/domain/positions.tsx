import Link from "next/link";
import { cn } from "@/lib/utils";
import { Shield } from "pixelarticons/react/Shield.js";
import type { Position } from "@/fixtures/types";
import { clockShort, dateLabel, price, quantity, usd } from "@/lib/format";
import { ORDER_STATE_LABEL, PURPOSE_LABEL } from "@/lib/labels";
import { type AnyOrder, isPast } from "@/lib/orders";
import { AsOf } from "./as-of";
import { Placeholder } from "./placeholders";
import { SourceTag } from "./mode";
import { SignedMoney } from "./money";

export function protectionText(p: Position): string {
  const pr = p.protection;
  switch (pr.kind) {
    case "bracket":
      return `Bracket: stop ${price(pr.stop_price ?? "0")}${pr.take_profit_price ? `, take-profit ${price(pr.take_profit_price)}` : ""}`;
    case "oco":
      return `OCO: stop ${price(pr.stop_price ?? "0")}, limit ${price(pr.limit_price ?? "0")}`;
    case "crypto_stop_limit":
      return `Stop-limit: stop ${price(pr.stop_price ?? "0")}, limit ${price(pr.limit_price ?? "0")}. A stop-limit may not fill on a gap.`;
    case "none":
      return "No resting protection";
    default: {
      const unhandled: never = pr.kind;
      throw new Error(`unhandled protection ${String(unhandled)}`);
    }
  }
}

const TH = "pt-1 pb-2 pr-3 text-label font-normal text-muted-foreground";

/** A link over its whole row or card: the row is the target, the text is the accessible name. */
export const STRETCHED_LINK = "outline-none after:absolute after:inset-0 after:content-[''] focus-visible:after:ring-3 focus-visible:after:ring-ring focus-visible:after:ring-inset";

export function PositionsTable({
  positions,
  now,
  staleSymbols,
  hrefFor,
}: {
  positions: Position[];
  now: string;
  staleSymbols: ReadonlySet<string>;
  hrefFor?: (p: Position) => string;
}) {
  if (positions.length === 0) return <p className="text-sm text-muted-foreground">No positions. The agent is flat.</p>;
  return (
    <div className="relative -mx-(--page-x) overflow-x-auto px-(--page-x) lg:mx-0 lg:px-0">
      <PositionRows positions={positions} now={now} staleSymbols={staleSymbols} hrefFor={hrefFor} />
      <table className="w-full min-w-[34rem] text-sm max-lg:hidden">
        <caption className="sr-only">Positions</caption>
        <thead>
          <tr className="border-b border-border text-left">
            <th scope="col" className={TH}>Instrument</th>
            <th scope="col" className={cn(TH, "text-right")}>Quantity</th>
            <th scope="col" className={cn(TH, "text-right")}>Mark</th>
            <th scope="col" className={cn(TH, "text-right")}>Value</th>
            <th scope="col" className={cn(TH, "pr-0 text-right")}>Unrealized</th>
          </tr>
        </thead>
        <tbody>
          {positions.map((p) => {
            const stale = staleSymbols.has(p.instrument.symbol);
            return (
              <tr key={p.instrument.asset_id} className={cn("border-b border-border/70 align-top last:border-b-0", hrefFor && "group relative transition-colors duration-(--duration-hover) hover:bg-background")}>
                <th scope="row" className="py-3 pr-3 text-left font-medium">
                  {hrefFor ? (
                    <Link href={hrefFor(p)} className={cn("underline-offset-4 group-hover:underline", STRETCHED_LINK)}>
                      {p.instrument.symbol}
                    </Link>
                  ) : (
                    p.instrument.symbol
                  )}
                  <span className="mt-1 flex items-start gap-1 text-caption font-normal text-muted-foreground">
                    <Shield className="size-6 shrink-0" aria-hidden />
                    {protectionText(p)}
                  </span>
                </th>
                <td className="py-3 pr-3 text-right font-mono tabular">{quantity(p.qty)}</td>
                <td className="py-3 pr-3 text-right whitespace-nowrap">
                  <span className="font-mono tabular">{price(p.mark)}</span>
                  <span className="block">
                    <AsOf at={p.mark_as_of} now={now} stale={stale} />
                  </span>
                </td>
                <td className="py-3 pr-3 text-right font-mono tabular">{usd(p.market_value)}</td>
                <td className="py-3 text-right">
                  <SignedMoney value={p.unrealized_pnl} showWord={false} />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <p className="mt-3 flex flex-wrap items-center gap-2 text-caption text-muted-foreground">
        Unrealized paper P&amp;L, simulated. <Placeholder name="performance" />
      </p>
    </div>
  );
}

/**
 * Below 64rem each position is a two-line row (DEC-207, critique C-14), so a phone shows what it is
 * worth without scrolling sideways: the instrument and its value, then the quantity, the mark and the
 * protection, with the unrealized P&L at the end of the second line and its disclosure symbol beside
 * it (DEC-210). From 64rem CSS hides these rows and the table shows instead, so a screen reader meets
 * one of the two. Each figure carries its label for assistive technology.
 */
function PositionRows({
  positions,
  now,
  staleSymbols,
  hrefFor,
}: {
  positions: Position[];
  now: string;
  staleSymbols: ReadonlySet<string>;
  hrefFor?: (p: Position) => string;
}) {
  return (
    <ul aria-label="Positions" data-slot="position-rows" className="grid lg:hidden">
      {positions.map((p) => (
        <li
          key={p.instrument.asset_id}
          data-slot="position-row"
          className={cn(
            "grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-x-3 gap-y-1 border-b border-border/70 py-3 text-sm last:border-b-0",
            hrefFor && "group relative transition-colors duration-(--duration-hover) hover:bg-background",
          )}
        >
          <p className="font-medium">
            {hrefFor ? (
              <Link href={hrefFor(p)} className={cn("underline-offset-4 group-hover:underline", STRETCHED_LINK)}>
                {p.instrument.symbol}
              </Link>
            ) : (
              p.instrument.symbol
            )}
          </p>
          <p className="text-right font-mono font-medium tabular">
            <span className="sr-only">Value </span>
            {usd(p.market_value)}
          </p>
          <p className="flex min-w-0 flex-wrap items-baseline gap-x-1.5 text-caption text-muted-foreground">
            <span className="whitespace-nowrap">
              <span className="sr-only">Quantity </span>
              <span className="font-mono text-foreground tabular">{quantity(p.qty)}</span> at <span className="sr-only">a mark of </span>
              <span className="font-mono text-foreground tabular">{price(p.mark)}</span>
            </span>
            <AsOf at={p.mark_as_of} now={now} stale={staleSymbols.has(p.instrument.symbol)} />
          </p>
          <p className="flex items-center justify-end gap-1 text-right">
            <span className="sr-only">Unrealized </span>
            <SignedMoney value={p.unrealized_pnl} showWord={false} />
            <Placeholder name="performance" />
          </p>
          <p className="col-span-2 flex items-start gap-1 text-caption text-pretty text-muted-foreground">
            <Shield className="size-6 shrink-0" aria-hidden />
            <span>
              <span className="sr-only">Protection: </span>
              {protectionText(p)}
            </span>
          </p>
        </li>
      ))}
    </ul>
  );
}

export function orderPriceText(o: AnyOrder): string {
  if (o.stop_price && o.limit_price) return `stop ${price(o.stop_price)}, limit ${price(o.limit_price)}`;
  if (o.stop_price) return `stop ${price(o.stop_price)}`;
  if (o.limit_price) return `limit ${price(o.limit_price)}`;
  return "at the market";
}

/** Nothing more is sent in the instrument while an order's state is unknown: the stopped treatment. */
export function SendingStopped({ symbol }: { symbol: string }) {
  return (
    <span data-slot="sending-stopped" className="inline-flex h-6 items-center rounded-md bg-ink px-2.5 text-label text-ink-foreground">
      Sending stopped in {symbol}
    </span>
  );
}

/**
 * Orders in every state. One whose state the broker has not confirmed is the account's problem, so it
 * wears lapis, and it stops sending in its instrument, so it carries the stopped tag too.
 */
export function OrdersTable({ orders, hrefFor, empty = "No working orders." }: { orders: AnyOrder[]; hrefFor?: (o: AnyOrder) => string; empty?: string }) {
  if (orders.length === 0) return <p className="text-sm text-muted-foreground">{empty}</p>;
  return (
    <ul className="grid">
      {orders.map((o) => {
        const unknown = o.state === "Unknown";
        const past = isPast(o);
        const title = (
          <>
            {o.side === "buy" ? "Buy" : "Sell"} <span className="font-mono tabular">{quantity(o.qty)}</span> {o.instrument.symbol}
          </>
        );
        return (
          <li
            key={o.client_order_id}
            data-state={o.state}
            className={cn(
              "-mx-3 grid gap-1 px-3 py-3",
              unknown ? "my-1 rounded-xl bg-lapis-soft px-4 py-4 max-sm:mx-0" : "border-b border-border/70 last:border-b-0",
              hrefFor && "group relative",
              hrefFor && !unknown && "transition-colors duration-(--duration-hover) hover:bg-background",
            )}
          >
            <p className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
              <span className="font-medium">
                {hrefFor ? (
                  <Link href={hrefFor(o)} className={cn("underline-offset-4 group-hover:underline", STRETCHED_LINK)}>
                    {title}
                  </Link>
                ) : (
                  title
                )}
                <span className="font-normal text-muted-foreground tabular"> {orderPriceText(o)}</span>
              </span>
              <span className="flex flex-wrap items-center gap-1.5">
                {unknown ? <SourceTag source="account" /> : null}
                {unknown ? <SendingStopped symbol={o.instrument.symbol} /> : null}
                <span className={cn("inline-flex h-6 items-center rounded-md px-2.5 text-label", unknown ? "bg-card ring-1 ring-foreground ring-inset" : past ? "bg-background text-muted-foreground" : "bg-background text-foreground")}>
                  {ORDER_STATE_LABEL[o.state]}
                </span>
              </span>
            </p>
            <p className="text-caption text-muted-foreground">
              {PURPOSE_LABEL[o.purpose]}, {o.time_in_force === "gtc" ? "good until canceled" : "day"}
              {past ? `. ${o.note} ${dateLabel(o.closed_at)}, ${clockShort(o.closed_at)}.` : ""}
            </p>
            {unknown ? (
              <p className="text-sm" data-slot="unknown-order">
                The broker has not answered for this order, so its state is unknown. Nothing else is sent in {o.instrument.symbol}, exits included, until the broker
                answers; the order counts as filled against every limit meanwhile. The kill switch still works.
              </p>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}
