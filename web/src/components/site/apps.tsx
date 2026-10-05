"use client";

import { useState } from "react";
import Image from "next/image";
import { cn } from "@/lib/utils";
import { OWL_SCROLL } from "./art";
import { BetaForm } from "./beta-form";
import { BODY, BOLD, MONO, PIXEL, PLAIN_BUTTON, SUNKEN } from "./letter";
import { NOTE, PixelIcon } from "./pixel-icons";
import { RecordTrace } from "./record-trace";
import { ArtCredit } from "./wallpaper";

function Menu({ items }: { items: string[] }) {
  return (
    <div aria-hidden className={cn("flex shrink-0 gap-4 px-2 py-0.5 text-[0.9375rem]", PIXEL)}>
      {items.map((m) => (
        <span key={m}>
          <span className="underline">{m[0]}</span>
          {m.slice(1)}
        </span>
      ))}
    </div>
  );
}

export const README = `Welcome to Owlhead.

This is our homepage, set out as a desktop from the late 1990s.

Click an icon to open it. Drag a window by its title bar. The three buttons at its top right hide it, fill the screen with it, and close it. The taskbar brings back anything hidden, and Start has everything.

Owlhead is a trading agent for your own brokerage account. It works inside rules you write, and it writes down every decision it makes. It is in private beta, trading on paper.

The wallpapers are paintings and prints from The Metropolitan Museum of Art, which shares them as public domain. Pick one in Display.

The record shows one decision the way Owlhead writes it down; try editing a line. Questions answers what people ask first, and the Guestbook asks for a place in the beta.

Tour.mp4 is a minute on what Owlhead does. The Recycle Bin holds what Owlhead won't do.

The owl in the corner has tips. Right-click the desktop and pick Ask the owl to bring it back.

Winamp is Webamp, the open source Winamp 2 for the browser. Its playlist is public domain recordings from Wikimedia Commons: records made from 1916 to 1925 by Bessie Smith and Louis Armstrong, George Gershwin with Paul Whiteman, Scott Joplin, Mamie Smith, Al Jolson, the Original Dixieland Jass Band and Enrico Caruso, and classics played by the US Air Force and Marine bands.`;

/** Notepad, open on the readme. */
export function Notepad() {
  return (
    <>
      <Menu items={["File", "Edit", "Search", "Help"]} />
      <div className={cn(SUNKEN, "min-h-0 flex-1 overflow-y-auto bg-card px-3 py-2")}>
        <p className={cn(MONO, "text-[1.25rem] leading-[1.35] whitespace-pre-line")}>{README}</p>
      </div>
    </>
  );
}

/** A picture viewer, open on Soga Nichokuan's owl from the Met. */
export function PictureViewer() {
  return (
    <>
      <Menu items={["File", "View", "Help"]} />
      <figure className="flex min-h-0 flex-1 flex-col gap-2">
        <div className={cn(SUNKEN, "relative min-h-0 flex-1 bg-muted")}>
          <Image src={OWL_SCROLL.src} alt="An owl in ink, perched on a pine branch beneath a pale full moon, on a hanging scroll." fill sizes="(min-width: 640px) 22rem, 100vw" className="object-contain p-2" />
        </div>
        <figcaption className="px-1 pb-1">
          <ArtCredit art={OWL_SCROLL} />
        </figcaption>
      </figure>
    </>
  );
}

/** The page's reading text, in a sunken well that scrolls inside its window. */
const PAGE = cn(SUNKEN, BODY, "min-h-0 flex-1 overflow-y-auto bg-card px-4 py-3 text-[1.0625rem] leading-[1.6] sm:px-5 sm:py-4");

/** The record, open on one decision from start to finish, with the tamper demo. */
export function RecordViewer() {
  return (
    <>
      <Menu items={["File", "Edit", "View", "Help"]} />
      <div className={PAGE}>
        <div className="grid gap-4 text-pretty">
          <p>
            Before any order goes out, Owlhead writes down what the agent read, what it concluded, which checks passed and who approved it. Each line carries a hash of the line before it, so if anyone edits a line, the chain
            stops matching from there on. You can export it for an auditor or your investors.
          </p>
          <p>Here is one decision from start to finish. Try editing a line.</p>
          <RecordTrace />
        </div>
      </div>
    </>
  );
}

export const QUESTIONS: { q: string; a: string }[] = [
  { q: "Can it take money out of my account?", a: "No. Owlhead only asks your broker for permission to trade. It can't withdraw or transfer money, and it can't change its own rules." },
  {
    q: "Is it trading real money?",
    a: "Not yet. In the beta, agents trade on paper with simulated money. Live trading comes later, once it has legal sign-off, and only when you switch it on with your passkey.",
  },
  { q: "Which brokers does it work with?", a: "Alpaca first. Robinhood and Kraken Derivatives US are planned." },
  { q: "How do I stop it?", a: "Press Stop. It halts every agent and cancels their open orders. Protective stops rest at your broker, so they hold even if Owlhead goes down." },
  { q: "What does it cost?", a: "Nothing during the private beta. We'll tell you the price well before we charge anything." },
  {
    q: "Is this investment advice?",
    a: "No. Owlhead is software that carries out rules you write. It doesn't know your finances and doesn't recommend what to buy or sell. Whether trading suits you is your decision, ideally with an adviser.",
  },
  { q: "Who gets in?", a: "We let people in a few at a time, roughly in the order they ask, with room for each kind of user so we hear from all of them." },
];

/** Help, open on the questions people ask first, each a term and its answer. */
export function Help() {
  return (
    <>
      <Menu items={["File", "Edit", "Bookmark", "Options", "Help"]} />
      <div className={PAGE}>
        <dl className="grid gap-5 text-pretty">
          {QUESTIONS.map(({ q, a }) => (
            <div key={q}>
              <dt className={BOLD}>{q}</dt>
              <dd>{a}</dd>
            </div>
          ))}
        </dl>
      </div>
    </>
  );
}

/** The guestbook, where a visitor asks for a place in the private beta. */
export function Guestbook() {
  return (
    <div className="grid min-h-0 flex-1 content-start gap-4 overflow-y-auto p-4 sm:p-5">
      <p className="text-base leading-snug">Sign the guestbook to ask for a place. Leave your email and we&apos;ll write once, when your place opens.</p>
      <BetaForm />
    </div>
  );
}

export type Discarded = { name: string; from: string; why: string };

/** What Owlhead threw out, and why. Restore explains instead of restoring. */
export const DISCARDED: Discarded[] = [
  { name: "Withdraw money.exe", from: "C:\\Owlhead\\Permissions", why: "Owlhead only asks your broker for permission to trade. It can't withdraw or transfer money." },
  { name: "Change my own rules.bat", from: "C:\\Owlhead\\Agent", why: "The agent works inside the rules you write, and it can't change them." },
  { name: "Approve my own order.exe", from: "C:\\Owlhead\\Agent", why: "An agent can't approve its own orders. Above the size you choose, they wait for you." },
  { name: "Black box fund.lnk", from: "C:\\Desktop", why: "Every decision is written down before the agent acts, so you can always see why an order went out and who said yes." },
  { name: "Edited record.log", from: "C:\\Owlhead\\Record", why: "Each line of the record carries a hash of the line before it. Edit one and the chain stops matching from there on." },
  { name: "Real money, right now.exe", from: "C:\\Owlhead\\Beta", why: "Not yet. In the beta, agents trade on paper. Live trading comes once it has legal sign-off, and only when you switch it on with your passkey." },
];

/** The Recycle Bin, in a detail view of its time. */
export function RecycleBin({ items, onEmpty }: { items: Discarded[]; onEmpty: () => void }) {
  const [picked, setPicked] = useState<string | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  const item = items.find((d) => d.name === picked) ?? null;
  return (
    <>
      <Menu items={["File", "Edit", "View", "Help"]} />
      <div className="flex shrink-0 flex-wrap gap-1.5 px-1.5 pb-1.5">
        <button type="button" disabled={!item} onClick={() => item && setSaid(`${item.name} can't be restored. ${item.why}`)} className={cn(PLAIN_BUTTON, "disabled:cursor-default disabled:opacity-50")}>
          Restore
        </button>
        <button
          type="button"
          disabled={items.length === 0}
          onClick={() => {
            onEmpty();
            setPicked(null);
            setSaid("The Recycle Bin is empty. None of it is coming back.");
          }}
          className={cn(PLAIN_BUTTON, "disabled:cursor-default disabled:opacity-50")}
        >
          Empty Recycle Bin
        </button>
      </div>
      <div className={cn(SUNKEN, "min-h-0 flex-1 overflow-auto bg-card")}>
        <table className={cn(PIXEL, "w-full border-collapse text-start text-[0.875rem]")}>
          <thead className="sticky top-0 bg-muted">
            <tr>
              {["Name", "Original location"].map((h) => (
                <th key={h} scope="col" className="border-r border-b border-r-foreground/40 border-b-foreground/40 px-2 py-0.5 text-start font-normal">
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {items.map((d) => (
              <tr key={d.name} aria-selected={picked === d.name} className={cn(picked === d.name && "bg-foreground text-card")}>
                <td className="px-1 py-0.5">
                  <button
                    type="button"
                    onClick={() => {
                      setPicked(d.name);
                      setSaid(null);
                    }}
                    className="flex w-full cursor-pointer items-center gap-1.5 text-start outline-none focus-visible:outline-1 focus-visible:outline-dotted focus-visible:outline-current"
                  >
                    <PixelIcon sprite={NOTE} className="size-4" />
                    {d.name}
                  </button>
                </td>
                <td className="px-2 py-0.5 whitespace-nowrap">{d.from}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {items.length === 0 && <p className={cn(PIXEL, "p-3 text-[0.875rem] text-muted-foreground")}>This folder is empty.</p>}
      </div>
      <p role="status" className={cn(PIXEL, SUNKEN, "mt-0.5 min-h-7 shrink-0 px-2 py-1 text-[0.875rem] leading-snug")}>
        {said ?? (item ? item.why : `${items.length} object${items.length === 1 ? "" : "s"}`)}
      </p>
    </>
  );
}
