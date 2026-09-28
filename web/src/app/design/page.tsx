import type { Metadata } from "next";
import type { CSSProperties, ReactNode } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { Skeleton } from "@/components/domain/skeleton";
import { Deadline } from "@/components/approvals/deadline";
import { BrandSpecimen } from "@/components/brand/brand-specimen";
import { ChartCredit, LevelLegend } from "@/components/charts/chart-parts";
import type { ChartLevel } from "@/components/charts/options";
import { Sparkline } from "@/components/charts/sparkline";
import { KumoSurfaces } from "@/components/design/kumo-surfaces";
import { MotionSamples } from "@/components/design/motion-samples";
import { AsOf } from "@/components/domain/as-of";
import { LimitRail } from "@/components/domain/envelope";
import { ModeBadge, ModeBanner, SOURCE_FIELD, SourceTag } from "@/components/domain/mode";
import { Money, SignedMoney } from "@/components/domain/money";
import { FixtureTag, Placeholder } from "@/components/domain/placeholders";
import { ProvenanceBadge } from "@/components/domain/provenance-badge";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { StopControl } from "@/components/shell/stop-control";
import type { AgentMode, Provenance } from "@/fixtures/types";
import { contrastRatio, toHex } from "@/lib/color";
import { dec } from "@/lib/decimal";
import { MODE_MEANING } from "@/lib/labels";
import { type RestrictionSource, SOURCE_LABEL } from "@/lib/restrictions";
import { type Meaning, colorTokens, markPairs, motionTokens, radiusTokens, spacingTokens, textPairs, tokenValue, typeScale } from "@/lib/tokens";

export const metadata: Metadata = { title: "Design system" };

const PROVENANCES: Provenance[] = ["user_stated", "user_entered", "template_structure", "platform_proposed", "platform_default"];
const MODES: AgentMode[] = ["normal", "exits_only", "paused", "stopped"];
const SOURCES: Array<[RestrictionSource, string]> = [
  ["mandate", "A limit in your mandate acted: drawdown, daily loss, the lifetime floor, a goal."],
  ["account", "The account needs a look: reconciliation, an unknown order, activity at the broker."],
  ["owner", "You paused or stopped the agent."],
  ["market", "Market data is stale for one instrument."],
];

const MEANINGS: Array<{ name: string; meaning: Meaning; means: string; detail: string; className: string }> = [
  {
    name: "Brass",
    meaning: "mandate",
    means: "Your mandate",
    detail: "Limits, rails, the envelope and the mandate's price lines, on a pale brass field. Where the agent must stay.",
    className: "bg-mandate text-mandate-strong",
  },
  { name: "Navy", meaning: "account", means: "The account", detail: "The account's equity line, links, primary actions and the paper hatch.", className: "bg-lapis text-lapis-foreground" },
  { name: "Ink", meaning: "stopped", means: "Stopped", detail: "A paused or stopped agent, and the Stop control.", className: "bg-ink text-ink-foreground" },
  { name: "Crimson", meaning: "kill", means: "Kill switch", detail: "Nothing else in the product is this colour.", className: "bg-crimson text-crimson-foreground" },
];

const CHART_RULES: Array<[string, string, string]> = [
  ["bg-lapis", "The account", "Account equity: a 2 px navy line over a pale navy fill. The hero of Home."],
  ["bg-foreground", "An agent", "One agent's equity: a 2 px ink line over the page colour. The hero of an agent."],
  ["bg-mandate-marker", "Your mandate", "Loss limits, the lifetime floor, a stop and a take-profit: dashed brass price lines, labelled on the axis in pale brass."],
  ["bg-ink", "A proposal", "The limit an agent asks you to approve, dashed, on a small neutral chart."],
  ["bg-gain", "Up candle", "A candle that closed above its open, with the sign in the readout. Blue when colour-blind friendly is on."],
  ["bg-loss", "Down candle", "A candle that closed below its open. Orange when colour-blind friendly is on."],
];

const SAMPLE_LEVELS: ChartLevel[] = [
  { key: "daily", label: "Daily loss limit", price: 9701.5, tone: "mandate", meaning: "Exits only until a new risk day" },
  { key: "avg-cost", label: "Average cost", price: 9850, tone: "account", meaning: "What you paid per unit" },
  { key: "proposed", label: "Proposed limit", price: 9912.25, tone: "proposal" },
];

const SAMPLE_POINTS = Array.from({ length: 48 }, (_, i) => ({ time: i * 600, value: 9800 + 60 * Math.sin(i / 6) + i * 1.5 }));

const DENSITY_ROWS: Array<[string, string]> = [
  ["Order placed", "10123.45"],
  ["Order filled", "10131.02"],
  ["Limit checked", "10118.77"],
];

const DO = [
  "One hero number per screen, set in the hero size, with its change, the word for it and its disclosure on the next line.",
  "Let space separate things. Reach for a hairline before a box, and for a box only when it carries meaning: brass, navy, a well.",
  "Sentence case everywhere. Weight 600 at most in the product.",
  "Tabular figures wherever numbers line up or change.",
  "Motion that answers the owner: a press, a sheet, the line drawing in once.",
  "Stop in the header at every width, never disabled, never behind a menu.",
];

const DONT = [
  "Gradients, glass, glows, or a shadow on anything that does not float.",
  "Capitals-only labels, heavy rules, or bands of colour as signage.",
  "Confetti, streaks, badges for trading, or any cue that rewards activity.",
  "Crimson anywhere but the kill switch, or brass for anything but the mandate.",
  "Optimistic state: nothing is shown as done before the deployment says so.",
  "Motion on a deadline, a figure the owner is deciding on, or a Stop control.",
];

function Block({ title, lead, children }: { title: string; lead?: ReactNode; children: ReactNode }) {
  const id = `design-${title.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
  return (
    <section className="grid gap-(--block-gap)" aria-labelledby={id}>
      <div className="grid gap-1.5">
        <h2 id={id} className="text-h2">
          {title}
        </h2>
        {lead ? <p className="max-w-prose text-muted-foreground">{lead}</p> : null}
      </div>
      {children}
    </section>
  );
}

function Rows({ caption, head, children, min = "36rem" }: { caption: string; head: string[]; children: ReactNode; min?: string }) {
  return (
    <div className="-mx-(--page-x) overflow-x-auto px-(--page-x)">
      <table className="w-full text-sm" style={{ minWidth: min }}>
        <caption className="sr-only">{caption}</caption>
        <thead>
          <tr className="border-b border-border/70 text-left">
            {head.map((h, i) => (
              <th key={h} scope="col" className={i === head.length - 1 && head.length > 3 ? "py-2 text-right text-label font-medium text-muted-foreground" : "py-2 pr-3 text-label font-medium text-muted-foreground"}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>{children}</tbody>
      </table>
    </div>
  );
}

const ROW = "border-b border-border/70 last:border-b-0";
const WELL = "grid content-start gap-3 rounded-2xl bg-background p-5";

function DensitySample({ density }: { density: "calm" | "dense" }) {
  return (
    <div data-density={density} className={WELL}>
      <p className="text-label font-medium text-muted-foreground">{density === "calm" ? "Calm: Home, agents, approvals" : "Dense: audit, settings, connections"}</p>
      <ul>
        {DENSITY_ROWS.map(([what, value]) => (
          <li key={what} className={`flex items-baseline justify-between gap-3 py-(--row-y) ${ROW}`}>
            <span>{what}</span>
            <Money value={value} className="tabular" />
          </li>
        ))}
      </ul>
    </div>
  );
}

export default function DesignPage() {
  return (
    <div className="grid gap-(--section-gap)">
      <header className="grid gap-2">
        <h1 className="text-h1">Design system</h1>
        <p className="max-w-prose text-muted-foreground">
          Owlhead&apos;s calm system (DEC-204): one hero number per screen, a chart at the centre, generous space and few boxes, in the navy and brass palette. Colour values live in{" "}
          <code>src/lib/palette.ts</code> and <code>globals.css</code>; a test fails if they drift or a reading pair drops below WCAG AA or APCA. The written rules are in <code>web/DESIGN.md</code>{" "}
          and the palette&apos;s in <code>web/COLOR.md</code>.
        </p>
      </header>

      <Block title="Brand" lead="The founder's Owlhead mark as a flat silhouette and the lowercase wordmark in outlines (DEC-203), navy on a light surface, in the navy and brass palette (DEC-202).">
        <BrandSpecimen />
      </Block>

      <Block title="Four colours, four meanings" lead="Flat colour only (DEC-200). Each colour means one thing everywhere, so the owner knows what binds an agent before reading a number. Most of a screen is none of them.">
        <ul className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          {MEANINGS.map((m, i) => (
            <li key={m.name} data-meaning={m.meaning} className={`reveal grid min-h-40 content-between gap-6 rounded-2xl p-5 ${m.className}`} style={{ "--i": i } as CSSProperties}>
              <span className="text-label font-medium">{m.name}</span>
              <span className="grid gap-1">
                <span className="text-h2">{m.means}</span>
                <span className="text-sm">{m.detail}</span>
              </span>
            </li>
          ))}
        </ul>
        <p className="max-w-prose text-sm text-muted-foreground">
          Gains and losses are the only other hues, and only as text beside a sign and the word: <SignedMoney value="123.45" /> <SignedMoney value="-67.89" /> <Placeholder name="performance" />
        </p>
      </Block>

      <Block title="Paper hatch" lead="Paper is the account's state, so it wears navy, hatched. The badge is in the header of every screen and never scrolls away.">
        <div className="flex flex-wrap items-center gap-3">
          <EnvironmentBadge environment="paper" />
          <div className="hatch h-16 w-40 rounded-2xl bg-card ring-1 ring-lapis" aria-hidden />
        </div>
      </Block>

      <Block title="Colour" lead="OKLCH ramps on one lightness curve; every token is a ramp step (web/COLOR.md). Neutrals are slate, tinted toward navy; nothing is pure grey, black, or white. Light only: dark mode is follow-up work.">
        <ul className="grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
          {colorTokens.map((t) => (
            <li key={t.name} data-meaning={t.meaning} className="grid grid-cols-[3rem_1fr] gap-3 rounded-xl bg-background p-2.5">
              <span className="size-12 rounded-lg ring-1 ring-border" style={{ background: t.value }} aria-hidden />
              <span className="grid min-w-0 content-center gap-0.5">
                <span className="font-mono text-caption font-semibold">--{t.name}</span>
                <span className="truncate font-mono text-label font-normal text-muted-foreground">
                  {t.value} {toHex(t.value)}
                </span>
                <span className="text-label font-normal text-muted-foreground">{t.role}</span>
              </span>
            </li>
          ))}
        </ul>
      </Block>

      <Block title="Contrast" lead="WCAG 2.2 ratios, computed from the OKLCH values (sRGB, gamut clipped, WCAG 2.2 relative luminance). Reading pairs reach 4.5:1 and marks 3:1. APCA (Lc 75 for reading pairs, Lc 45 for marks) is checked in the tests only, with apca-w3 as a dev dependency, so it is not computed here. The Kumo role pairs are checked in each surface scope by the tests.">
        <Rows caption="Contrast ratios" head={["Pair", "Use", "Ratio"]}>
          {[...textPairs.map((p) => ({ ...p, min: 4.5 })), ...markPairs.map((p) => ({ ...p, min: 3 }))].map((p) => (
            <tr key={`${p.fg}-${p.bg}-${p.kind}-${p.use}`} className={ROW}>
              <th scope="row" className="py-2 pr-3 text-left font-mono text-caption font-normal">
                {p.fg} / {p.bg}
              </th>
              <td className="py-2 pr-3 text-muted-foreground">{p.use}</td>
              <td className="py-2 text-right font-mono tabular">
                {contrastRatio(tokenValue(p.fg), tokenValue(p.bg)).toFixed(2)}:1 <span className="text-muted-foreground">(≥ {p.min})</span>
              </td>
            </tr>
          ))}
        </Rows>
      </Block>

      <Block
        title="Type"
        lead="Mona Sans, one variable family for everything: a grotesque with a humanist touch that stays warm at 16 px and crisp at the hero size. Tabular figures and a plain zero (0 10 100), a true minus sign, sentence case throughout, and weight 600 at most."
      >
        <ul className="grid">
          {typeScale.map((t) => (
            <li key={t.role} className={`grid gap-1 py-3 sm:grid-cols-[7rem_1fr] sm:items-baseline ${ROW}`}>
              <span className="text-label font-medium text-muted-foreground">{t.role}</span>
              <span className="grid min-w-0 gap-1">
                <span className={t.className}>{t.sample}</span>
                <span className="text-caption text-muted-foreground">{t.spec}</span>
              </span>
            </li>
          ))}
        </ul>
      </Block>

      <Block title="Space and density" lead="Space does the work boxes used to. Two densities share every token: calm for the screens an owner lives in, dense for audit and admin, where more rows on screen matter more than air. Touch targets stay at least 44 px on phones either way.">
        <Rows caption="Spacing tokens in each density" head={["Token", "Use", "Calm", "Dense"]} min="40rem">
          {spacingTokens.map((s) => (
            <tr key={s.name} className={ROW}>
              <th scope="row" className="py-2 pr-3 text-left font-mono text-caption font-normal">
                {s.name}
              </th>
              <td className="py-2 pr-3 text-muted-foreground">{s.use}</td>
              <td className="py-2 pr-3 font-mono tabular">{s.calm}</td>
              <td className="py-2 text-right font-mono tabular">{s.dense}</td>
            </tr>
          ))}
        </Rows>
        <div className="grid gap-3 lg:grid-cols-2">
          <DensitySample density="calm" />
          <DensitySample density="dense" />
        </div>
      </Block>

      <Block title="Shape" lead="Soft, consistent corners: rounder the larger the surface. Shadows only on what floats above the page: menus, the Stop sheet, dialogs.">
        <Rows caption="Radius tokens" head={["Token", "Value", "Use"]} min="32rem">
          {radiusTokens.map((r) => (
            <tr key={r.name} className={ROW}>
              <th scope="row" className="py-2 pr-3 text-left font-mono text-caption font-normal">
                {r.name}
              </th>
              <td className="py-2 pr-3 font-mono text-caption">{r.value}</td>
              <td className="py-2 text-muted-foreground">{r.use}</td>
            </tr>
          ))}
        </Rows>
      </Block>

      <Block title="States" lead="Every state has one flat treatment. An agent's mode is a pill; a restriction wears the colour of whoever imposed it; system states carry no meaning colour.">
        <div className="grid gap-(--section-gap)">
          <div className="grid gap-(--block-gap)">
            <h3 className="text-h3">Agent modes</h3>
            <ul>
              {MODES.map((m) => (
                <li key={m} className={`grid gap-2 py-(--row-y) sm:grid-cols-[9rem_1fr] sm:items-center ${ROW}`}>
                  <span>
                    <ModeBadge mode={m} />
                  </span>
                  <span className="text-sm text-muted-foreground">{MODE_MEANING[m]}</span>
                </li>
              ))}
            </ul>
          </div>
          <div className="grid gap-(--block-gap)">
            <h3 className="text-h3">Restrictions, by who imposed them</h3>
            <ul className="grid gap-2 sm:grid-cols-2">
              {SOURCES.map(([s, text]) => (
                <li key={s} className={`grid gap-2 rounded-xl px-4 py-3 ${SOURCE_FIELD[s]}`}>
                  <SourceTag source={s} />
                  <span className="text-sm">
                    <span className="font-semibold">{SOURCE_LABEL[s]}.</span> {text}
                  </span>
                </li>
              ))}
            </ul>
          </div>
          <div className="grid gap-(--block-gap)">
            <h3 className="text-h3">System states</h3>
            <ul className="grid gap-3 lg:grid-cols-2">
              <li className={WELL}>
                <span className="text-label font-medium text-muted-foreground">Stale</span>
                <AsOf at="2026-09-28T14:02:11-04:00" now="2026-09-28T14:05:20-04:00" stale />
                <p className="text-sm text-muted-foreground">The figure stays, labelled with its age, and the chart says how old its last point is.</p>
              </li>
              <li className={WELL}>
                <span className="text-label font-medium text-muted-foreground">Unreachable and error</span>
                <p className="text-h3">Cannot reach your deployment</p>
                <p className="text-sm text-muted-foreground">A quiet well with the one thing to try. No agent data is shown, and none is kept on this device.</p>
              </li>
              <li className={WELL}>
                <span className="text-label font-medium text-muted-foreground">Loading</span>
                <div className="grid gap-2" aria-hidden>
                  <Skeleton className="h-10 w-40 rounded-lg" />
                  <Skeleton className="h-24 rounded-xl" />
                </div>
                <p className="text-sm text-muted-foreground">Skeletons in the shape of the screen, never a value from an earlier visit.</p>
              </li>
              <li className={WELL}>
                <span className="text-label font-medium text-muted-foreground">Empty</span>
                <p className="text-h3">No agents yet</p>
                <p className="text-sm text-muted-foreground">One sentence and the one next step: describe your first agent.</p>
              </li>
            </ul>
          </div>
          <div className="grid gap-(--block-gap)">
            <h3 className="text-h3">Mode banner</h3>
            <ModeBanner
              mode="exits_only"
              restrictions={[
                { code: "drawdown_exits_only", since: "2026-09-28T14:01:12-04:00" },
                { code: "reconciliation_mismatch", since: "2026-09-28T13:40:00-04:00" },
              ]}
            />
          </div>
        </div>
      </Block>

      <Block title="Components" lead="Kumo components in the calm tokens: pill-shaped actions, flat fills. Stop and kill-switch actions are never disabled, so no disabled state is shown for them.">
        <div className="grid gap-(--section-gap)">
          <div className="grid gap-(--block-gap)">
            <h3 className="text-h3">Actions</h3>
            <div className="flex flex-wrap items-center gap-2">
              <Button size="lg" variant="primary" className="h-11 rounded-full px-5">
                Primary
              </Button>
              <Button size="lg" variant="secondary" className="h-11 rounded-full px-5">
                Secondary
              </Button>
              <Button size="lg" variant="outline" className="h-11 rounded-full px-5">
                Outline
              </Button>
              <Button size="lg" variant="ghost" className="h-11 rounded-full px-5">
                Ghost
              </Button>
              <Button size="lg" variant="outline" className="h-11 rounded-full px-5" disabled>
                Disabled
              </Button>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <StopControl />
              <span data-meaning="kill" className="inline-flex h-11 items-center rounded-xl bg-crimson px-4 font-semibold text-crimson-foreground">
                Kill switch (crimson, only here)
              </span>
            </div>
            <p className="text-sm text-muted-foreground">Approve and Skip are the same size, weight and variant, side by side, with nothing preselected.</p>
          </div>
          <div className="grid gap-(--block-gap)">
            <h3 className="text-h3">Labels</h3>
            <div className="flex flex-wrap items-center gap-2">
              {MODES.map((m) => (
                <ModeBadge key={m} mode={m} />
              ))}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              {PROVENANCES.map((p) => (
                <ProvenanceBadge key={p} provenance={{ path: "/", provenance: p }} />
              ))}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <FixtureTag />
              <Placeholder name="performance" />
              <Placeholder name="hypothetical" />
              <Placeholder name="retailAutoLive" />
            </div>
          </div>
          <div className="grid gap-3 lg:grid-cols-2">
            <div className={WELL}>
              <h3 className="text-h3">Figures</h3>
              <Money value="24987.50" className="text-hero tabular" />
              <SignedMoney value="123.45" />
              <SignedMoney value="-67.89" />
              <SignedMoney value="0" />
              <span className="font-mono tabular">0 1 2 3 4 5 6 7 8 9 · 10.00 · 100.00</span>
              <AsOf at="2026-09-28T14:05:18-04:00" now="2026-09-28T14:05:20-04:00" />
              <Deadline deadline="2026-09-28T14:14:58-04:00" now="2026-09-28T14:05:20-04:00" />
            </div>
            <div className="grid content-start gap-4 rounded-2xl bg-mandate p-5 text-mandate-foreground">
              <h3 className="text-h3 text-mandate-strong">Your mandate</h3>
              <LimitRail rail={{ key: "a", label: "Total holdings", used: dec("1618.09"), cap: dec("2000"), atCap: "No new buys" }} />
              <LimitRail rail={{ key: "b", label: "Loss today", used: dec("0"), cap: dec("201"), atCap: "Exits only until a new risk day" }} />
              <LimitRail rail={{ key: "c", label: "Loss today", used: dec("214.5"), cap: dec("201"), atCap: "Exits only until a new risk day" }} />
            </div>
          </div>
        </div>
      </Block>

      <Block
        title="Charts"
        lead="TradingView Lightweight Charts, one per screen as the centrepiece. Scrub to read any moment: the hero number and its date follow the finger and return to now on release. Ranges run from 1D to All; the change is measured from the start of the range. Plain-zero figures, times in ET, and every level also listed in words."
      >
        <div className="grid gap-(--block-gap) lg:grid-cols-2">
          <ul className="grid content-start gap-2.5 text-sm">
            {CHART_RULES.map(([swatch, name, use]) => (
              <li key={name} className="grid grid-cols-[0.75rem_minmax(0,1fr)] items-baseline gap-2.5">
                <span aria-hidden className={`size-3 self-center rounded-full ${swatch}`} />
                <span>
                  <span className="font-semibold">{name}</span>: {use}
                </span>
              </li>
            ))}
          </ul>
          <div className="@container grid content-start gap-3 rounded-2xl bg-background p-5">
            <Sparkline points={SAMPLE_POINTS} limit={9760} label="Sample sparkline: equity rising over the day, above the daily loss limit" className="h-14 w-full" />
            <LevelLegend levels={SAMPLE_LEVELS} />
            <p className="text-caption text-muted-foreground">
              Loading shows the chart&apos;s outline and no line; stale data shows its age beside the chart; if the canvas cannot be drawn, the chart says so and the figures stay.
            </p>
            <ChartCredit />
          </div>
        </div>
      </Block>

      <Block
        title="Kumo surfaces, flattened"
        lead="Kumo paints an overlay on emphasis buttons, fades on sticky table cells and tab scroll buttons, masks on scrolling regions, and a shimmer on skeletons. The theme flattens every one (DEC-200); a browser test reads the computed styles of these specimens."
      >
        <KumoSurfaces />
      </Block>

      <Block title="Motion" lead="Motion answers an action or shows what changed. Most of it is under 300 ms; the equity line's draw-in, once on load, is the one longer moment. Deadlines and anything that could nudge a decision never move. Reduced motion keeps colour changes and drops movement.">
        <Rows caption="Motion tokens" head={["Token", "Value", "Use"]} min="32rem">
          {motionTokens.map((m) => (
            <tr key={m.name} className={ROW}>
              <th scope="row" className="py-2 pr-3 text-left font-mono text-caption font-normal">
                {m.name}
              </th>
              <td className="py-2 pr-3 font-mono text-caption">{m.value}</td>
              <td className="py-2 text-muted-foreground">{m.use}</td>
            </tr>
          ))}
        </Rows>
        <MotionSamples />
      </Block>

      <Block title="Do and don't">
        <div className="grid gap-3 lg:grid-cols-2">
          <div className={WELL}>
            <h3 className="text-h3">Do</h3>
            <ul className="grid gap-2 text-sm">
              {DO.map((d) => (
                <li key={d}>{d}</li>
              ))}
            </ul>
          </div>
          <div className={WELL}>
            <h3 className="text-h3">Don&apos;t</h3>
            <ul className="grid gap-2 text-sm">
              {DONT.map((d) => (
                <li key={d}>{d}</li>
              ))}
            </ul>
          </div>
        </div>
      </Block>
    </div>
  );
}
