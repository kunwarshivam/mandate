import Link from "next/link";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { Deadline } from "@/components/approvals/deadline";
import { KEY, KEY_SM } from "@/components/kumo/key";
import type { Approval, Iso } from "@/fixtures/types";
import { clock, price, quantity, usd, zoneLabel } from "@/lib/format";
import { APPROVAL_STATUS_LABEL, PURPOSE_LABEL } from "@/lib/labels";
import { stamp } from "@/lib/messages";
import { cn } from "@/lib/utils";

/**
 * The waiting request, pinned above a thread while it scrolls, so it is never lost under newer
 * entries. Static time only, like a list row; the request itself counts down (DEC-207).
 */
export function PinnedRequest({ approval }: { approval: Approval }) {
  const { bound } = approval;
  return (
    <Link
      href={`/approvals/${approval.approval_id}`}
      data-slot="pinned-request"
      className="press sticky top-16 z-10 -mx-(--page-x) flex items-center gap-3 bg-lapis-soft px-(--page-x) py-2.5 outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset lg:mx-0 lg:rounded-xl lg:px-4"
    >
      <span className="grid min-w-0 flex-1 gap-0.5">
        <span className="truncate font-semibold">
          Buy <span className="font-mono tabular">{quantity(bound.qty)}</span> {bound.symbol} at <span className="font-mono tabular">{price(bound.limit)}</span>
        </span>
        <span className="text-caption text-muted-foreground">
          Skipped at{" "}
          <time dateTime={approval.deadline} className="font-mono tabular">
            {clock(approval.deadline)} {zoneLabel(approval.deadline)}
          </time>{" "}
          if you do nothing
        </span>
      </span>
      <span className="inline-flex shrink-0 items-center gap-0.5 text-sm font-medium text-lapis">
        Review
        <ArrowRight aria-hidden className="-my-1 size-6" />
      </span>
    </Link>
  );
}

/**
 * A request in a thread. It says what was asked and links to the request itself, the record screen
 * where the owner approves or skips with every line shown (D6); a message never approves (DEC-477).
 */
export function RequestCard({ approval, now }: { approval: Approval; now: Iso }) {
  const open = approval.status === "delivered";
  const { bound } = approval;
  const order = approval.risk_impact.find((f) => f.field === "order_usd");
  return (
    <article
      data-slot="request-card"
      data-status={approval.status}
      aria-label={`Request: buy ${quantity(bound.qty)} ${bound.symbol}`}
      className={cn("grid max-w-lg gap-3 rounded-2xl p-4", open ? "bg-lapis-soft" : "border border-border")}
    >
      <p className="flex flex-wrap items-center justify-between gap-2 text-caption text-muted-foreground">
        <span className={cn("inline-flex h-6 items-center rounded-md px-2.5 text-label", open ? "bg-card text-lapis" : "bg-background text-foreground")}>
          {open ? "Asks you" : APPROVAL_STATUS_LABEL[approval.status]}
        </span>
        <time dateTime={approval.requested_at} className="font-mono tabular">
          {stamp(approval.requested_at, now)}
        </time>
      </p>
      <p className={cn("text-lg font-semibold text-pretty", !open && "text-muted-foreground")}>
        Buy <span className="font-mono tabular">{quantity(bound.qty)}</span> {bound.symbol} at a limit of <span className="font-mono tabular">{price(bound.limit)}</span>
      </p>
      <dl className="grid gap-1 text-sm">
        {order ? (
          <div className="flex flex-wrap gap-x-2">
            <dt className="text-muted-foreground">Order value</dt>
            <dd className="tabular">{usd(order.value)}
              {order.cap ? ` of the ${usd(order.cap)} order limit` : ""}</dd>
          </div>
        ) : null}
        <div className="flex flex-wrap gap-x-2">
          <dt className="text-muted-foreground">Purpose</dt>
          <dd>{PURPOSE_LABEL[bound.purpose]}</dd>
        </div>
        <div className="grid gap-0.5">
          <dt className="text-muted-foreground">Why it asked</dt>
          <dd className="text-pretty">{approval.trigger}</dd>
        </div>
      </dl>
      {open ? (
        <>
          <Deadline deadline={approval.deadline} now={now} />
          <Link href={`/approvals/${approval.approval_id}`} className={cn(KEY, "w-fit")}>
            Open the request
            <ArrowRight aria-hidden className="size-6" />
          </Link>
          <p className="text-caption text-pretty text-muted-foreground">You approve or skip on the request itself, where every line is shown.</p>
        </>
      ) : (
        <>
          {approval.resolution ? (
            <p className="text-sm text-pretty">
              <time dateTime={approval.resolution.at} className="font-mono tabular">
                {stamp(approval.resolution.at, now)}
              </time>
              : {approval.resolution.text}
            </p>
          ) : null}
          <Link href={`/approvals/${approval.approval_id}`} className={cn(KEY_SM, "w-fit")}>
            See the request
          </Link>
        </>
      )}
    </article>
  );
}
