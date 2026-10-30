"use client";

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { FixtureTag } from "@/components/domain/placeholders";
import { ConfirmCard } from "./cards";
import { ChatThread } from "./thread";

/** A version as the API names it; anything else in `?version=` is ignored. */
const VERSION = /^sha256:[0-9a-f]{64}$/;

/**
 * A new agent, in words (DEC-2293): the setup thread. The agent asks what it needs, drafts a
 * mandate the server validates, and posts it as a card the owner confirms and deploys to paper.
 * A version made outside the chat, by the CLI or an owner-connected agent, opens here as
 * `/new?version={hash}` with the same card, so the owner confirms it with the same checks.
 */
export function SetupScreen() {
  const asked = useSearchParams().get("version");
  const version = asked && VERSION.test(asked) ? asked : null;
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-slot="setup-screen">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-border/70 px-(--page-x) py-3 lg:px-8">
        <Link href="/" className="text-sm text-muted-foreground underline-offset-4 hover:underline">
          Home
        </Link>
        <h1 className="text-h2">New agent</h1>
        <FixtureTag scope="workspace" />
      </header>
      {version ? (
        <div className="border-b border-border/70 px-(--page-x) py-4 lg:px-8" data-slot="version-to-confirm">
          <ConfirmCard card={{ kind: "confirm_mandate", version, name: null, lines: [] }} />
        </div>
      ) : null}
      <ChatThread
        target={{ setup: null }}
        label="Describe your agent"
        placeholder="What should it trade, with how much, and how careful should it be?"
        empty={
          <div className="grid max-w-measure gap-2">
            <p>Tell me what you want this agent to do, in your own words: what to trade, how much money it has, and how much it may lose.</p>
            <p>I&apos;ll ask about anything missing, then show you the whole agent to confirm. It runs on paper with simulated money.</p>
          </div>
        }
      />
    </div>
  );
}
