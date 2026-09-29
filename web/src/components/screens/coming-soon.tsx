import Link from "next/link";
import { Empty } from "@cloudflare/kumo/components/empty";
import { Hourglass } from "@phosphor-icons/react/ssr";

/**
 * A screen the navigation reaches before it is built: what it is for, and a way back. Nothing on it
 * pretends to work.
 */
export function ComingSoon({ purpose, back }: { purpose: string; back?: { href: string; label: string } }) {
  return (
    <div data-slot="coming-soon" className="grid max-w-3xl gap-(--block-gap)">
      <Empty
        size="sm"
        icon={<Hourglass className="size-8 text-muted-foreground" aria-hidden />}
        title="Coming in the next slice"
        description={purpose}
        className="items-start rounded-2xl border border-dashed border-border bg-card text-left [&>div]:items-start [&_p]:text-left"
      />
      {back ? (
        <Link href={back.href} className="w-fit font-medium text-lapis underline decoration-lapis/30 underline-offset-4 hover:decoration-current">
          {back.label}
        </Link>
      ) : null}
    </div>
  );
}
