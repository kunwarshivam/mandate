import { cn } from "@/lib/utils";
import { CircleHelp, ShieldCheck } from "lucide-react";
import type { Position, WorkingOrder } from "@/fixtures/types";
import { price, quantity, usd } from "@/lib/format";
import { ORDER_STATE_LABEL, PURPOSE_LABEL } from "@/lib/labels";
import { AsOf } from "./as-of";
import { SignedMoney } from "./money";

function protectionText(p: Position): string {
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

export function PositionsTable({ positions, now, staleSymbols }: { positions: Position[]; now: string; staleSymbols: ReadonlySet<string> }) {
  if (positions.length === 0) return <p className="text-sm text-muted-foreground">No positions. The agent is flat.</p>;
  return (
    <div className="-mx-4 overflow-x-auto px-4">
      <table className="w-full min-w-[34rem] text-sm">
        <caption className="sr-only">Positions</caption>
        <thead>
          <tr className="border-b text-left text-caption text-muted-foreground">
            <th scope="col" className="py-2 pr-3 font-medium">Instrument</th>
            <th scope="col" className="py-2 pr-3 text-right font-medium">Quantity</th>
            <th scope="col" className="py-2 pr-3 text-right font-medium">Mark</th>
            <th scope="col" className="py-2 pr-3 text-right font-medium">Value</th>
            <th scope="col" className="py-2 text-right font-medium">Unrealized</th>
          </tr>
        </thead>
        <tbody>
          {positions.map((p) => {
            const stale = staleSymbols.has(p.instrument.symbol);
            return (
              <tr key={p.instrument.asset_id} className="border-b border-border/60 align-top last:border-b-0">
                <th scope="row" className="py-3 pr-3 text-left font-medium">
                  {p.instrument.symbol}
                  <span className="mt-1 flex items-start gap-1 text-caption font-normal text-muted-foreground">
                    <ShieldCheck className="mt-0.5 size-3.5 shrink-0" aria-hidden />
                    {protectionText(p)}
                  </span>
                </th>
                <td className="py-3 pr-3 text-right font-mono tabular">{quantity(p.qty)}</td>
                <td className="py-3 pr-3 text-right">
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
    </div>
  );
}

export function OrdersTable({ orders }: { orders: WorkingOrder[] }) {
  if (orders.length === 0) return <p className="text-sm text-muted-foreground">No working orders.</p>;
  return (
    <ul className="grid divide-y divide-border/60">
      {orders.map((o) => {
        const unknown = o.state === "Unknown";
        const priceText = o.stop_price && o.limit_price ? `stop ${price(o.stop_price)}, limit ${price(o.limit_price)}` : o.stop_price ? `stop ${price(o.stop_price)}` : o.limit_price ? `limit ${price(o.limit_price)}` : "";
        return (
          <li key={o.client_order_id} data-state={o.state} className={cn("grid gap-1 py-3", unknown && "-mx-2 rounded-md bg-notice px-2 ring-1 ring-notice-border")}>
            <p className="flex flex-wrap items-baseline justify-between gap-x-3">
              <span className="font-medium">
                {o.side === "buy" ? "Buy" : "Sell"} <span className="font-mono tabular">{quantity(o.qty)}</span> {o.instrument.symbol}
                <span className="font-normal text-muted-foreground"> {priceText}</span>
              </span>
              <span className={cn("inline-flex items-center gap-1 text-caption", unknown ? "font-semibold text-foreground" : "text-muted-foreground")}>
                {unknown ? <CircleHelp className="size-3.5" aria-hidden /> : null}
                {ORDER_STATE_LABEL[o.state]}
              </span>
            </p>
            <p className="text-caption text-muted-foreground">
              {PURPOSE_LABEL[o.purpose]}, {o.time_in_force === "gtc" ? "good until canceled" : "day"}
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
