"use client";

import Link from "next/link";
import { FixtureTag } from "@/components/domain/placeholders";
import { ChatThread } from "./thread";

/**
 * A new agent, in words (DEC-2293): the setup thread. The agent asks what it needs, drafts a
 * mandate the server validates, and posts it as a card the owner confirms and deploys to paper.
 */
export function SetupScreen() {
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-slot="setup-screen">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-border/70 px-(--page-x) py-3 lg:px-8">
        <Link href="/" className="text-sm text-muted-foreground underline-offset-4 hover:underline">
          Home
        </Link>
        <h1 className="text-h2">New agent</h1>
        <FixtureTag scope="workspace" />
      </header>
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
