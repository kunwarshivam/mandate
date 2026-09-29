"use client";

import { Banner } from "@cloudflare/kumo/components/banner";
import { Question } from "@phosphor-icons/react";
import { useRuntime } from "@/lib/mock-runtime";

/**
 * Account-wide notices under the header. A command or response the deployment took without a
 * journal entry is never shown as done: the banner says the result is unknown until it answers.
 * Kumo's Banner has no role of its own, so the wrapper carries it.
 */
export function AccountBanners() {
  const { commands, responses } = useRuntime();
  const unknown = commands.some((c) => c.phase === "unknown") || Object.values(responses).some((r) => r.phase === "unknown");
  if (!unknown) return null;
  return (
    <div role="alert" data-slot="result-unknown" className="border-b px-(--page-x) py-2">
      <Banner
        variant="default"
        icon={<Question className="size-5" aria-hidden />}
        title="The result is unknown; we are checking."
        description="Your deployment took the request but has not journaled it. Nothing on screen changes until it does; if it matters now, act at the broker directly."
      />
    </div>
  );
}
