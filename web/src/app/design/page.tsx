import type { Metadata } from "next";
import type { CSSProperties, ReactNode } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { Skeleton } from "@/components/domain/skeleton";
import { Deadline } from "@/components/approvals/deadline";
import { ChartCredit, LevelLegend } from "@/components/charts/chart-parts";
import type { ChartLevel } from "@/components/charts/options";
import { Sparkline } from "@/components/charts/sparkline";
import { KumoSurfaces } from "@/components/design/kumo-surfaces";
import { MotionSamples } from "@/components/design/motion-samples";
import { AsOf } from "@/components/domain/as-of";
import { LimitRail } from "@/components/domain/envelope";
import { MODE_FIELD, ModeBadge, ModeBanner, SOURCE_FIELD, SourceTag } from "@/components/domain/mode";
import { SignedMoney } from "@/components/domain/money";
import { FixtureTag, Placeholder } from "@/components/domain/placeholders";
import { ProvenanceBadge } from "@/components/domain/provenance-badge";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { StopControl } from "@/components/shell/stop-control";
import type { AgentMode, Provenance } from "@/fixtures/types";
import { contrastRatio, toHex } from "@/lib/color";
import { dec } from "@/lib/decimal";
import { MODE_LABEL, MODE_MEANING } from "@/lib/labels";
import { type RestrictionSource, SOURCE_LABEL } from "@/lib/restrictions";
import { type Meaning, colorTokens, markPairs, motionTokens, spacingTokens, textPairs, tokenValue, typeScale } from "@/lib/tokens";

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
  { name: "Marigold", meaning: "mandate", means: "Your mandate", detail: "Limits, rails, and the envelope. Where the agent must stay.", className: "bg-marigold text-marigold-foreground" },
  { name: "Lapis", meaning: "account", means: "The account", detail: "Its board, the paper hatch, the current page, primary actions.", className: "bg-lapis text-lapis-foreground" },
  { name: "Ink", meaning: "stopped", means: "Stopped", detail: "A paused or stopped agent, and the Stop control.", className: "bg-ink text-ink-foreground" },
  { name: "Crimson", meaning: "kill", means: "Kill switch", detail: "Nothing else in the product is this colour.", className: "bg-crimson text-crimson-foreground" },
];

const CHART_RULES: Array<[string, string, string]> = [
  ["bg-lapis", "The account", "Account equity, and your average cost on a position."],
  ["bg-foreground", "An agent", "One agent's equity, a flat line over a muted fill."],
  ["bg-marigold", "Your mandate", "Loss limits, the lifetime floor, a stop and a take-profit, each a labelled line."],
  ["bg-ink", "A proposal", "The limit an agent asks you to approve, dashed, on a small neutral chart."],
  ["bg-gain", "Up candle", "A candle that closed above its open, with the sign in the readout."],
  ["bg-loss", "Down candle", "A candle that closed below its open."],
];

const SAMPLE_LEVELS: ChartLevel[] = [
  { key: "daily", label: "Daily loss limit", price: 9701.5, tone: "mandate", meaning: "Exits only until a new risk day" },
  { key: "avg-cost", label: "Average cost", price: 9850, tone: "account", meaning: "What you paid per unit" },
  { key: "proposed", label: "Proposed limit", price: 9912.25, tone: "proposal" },
];

const SAMPLE_POINTS = Array.from({ length: 48 }, (_, i) => ({ time: i * 600, value: 9800 + 60 * Math.sin(i / 6) + i * 1.5 }));

function Block({ title, lead, children }: { title: string; lead?: string; children: ReactNode }) {
  const id = `design-${title.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
  return (
    <section className="grid gap-(--block-gap)" aria-labelledby={id}>
      <div className="grid gap-1 border-b-2 border-foreground pb-2">
        <h2 id={id} className="text-title">
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
          <tr className="border-b-2 border-foreground text-left">
            {head.map((h, i) => (
              <th key={h} scope="col" className={i === head.length - 1 && head.length > 3 ? "py-2 text-right label-caps" : "py-2 pr-3 label-caps"}>
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

export default function DesignPage() {
  return (
    <div className="grid gap-(--section-gap)">
      <header className="grid gap-1.5">
        <h1 className="text-title sm:text-display">Design system</h1>
        <p className="max-w-prose text-muted-foreground">
          Placard, Owlhead&apos;s visual system: transit signage read at a glance by someone in a hurry. Colour values live in <code>src/lib/tokens.ts</code> and{" "}
          <code>globals.css</code>; a test fails if they drift or a reading pair drops below WCAG AA. The written rules are in <code>web/DESIGN.md</code>.
        </p>
      </header>

      <Block title="Four colours, four meanings" lead="Flat colour only (DEC-200). Each colour means one thing everywhere, so the owner knows what binds an agent before reading a number.">
        <ul className="grid gap-(--seam) sm:grid-cols-2 xl:grid-cols-4">
          {MEANINGS.map((m, i) => (
            <li key={m.name} data-meaning={m.meaning} className={`reveal grid min-h-40 content-between gap-6 p-4 ${m.className}`} style={{ "--i": i } as CSSProperties}>
              <span className="label-caps">{m.name}</span>
              <span className="grid gap-1">
                <span className="font-display text-title uppercase">{m.means}</span>
                <span className="text-sm">{m.detail}</span>
              </span>
            </li>
          ))}
        </ul>
        <p className="max-w-prose text-sm text-muted-foreground">
          Gains and losses are the only other hues, and only as text beside a sign and the word: <SignedMoney value="123.45" /> <SignedMoney value="-67.89" />
        </p>
      </Block>

      <Block title="Paper hatch" lead="Paper is the account's state, so it wears lapis, hatched. The badge is in the header of every screen and never scrolls away.">
        <div className="flex flex-wrap items-center gap-3">
          <EnvironmentBadge environment="paper" />
          <div className="hatch h-16 w-40 border-2 border-lapis bg-card" aria-hidden />
        </div>
      </Block>

      <Block title="Colour" lead="OKLCH. Every neutral is tinted toward lapis; nothing is pure grey, black, or white. Light only: dark mode is follow-up work.">
        <ul className="grid gap-(--seam) sm:grid-cols-2 xl:grid-cols-3">
          {colorTokens.map((t) => (
            <li key={t.name} data-meaning={t.meaning} className="grid grid-cols-[3.5rem_1fr] gap-3 bg-card p-2.5">
              <span className="h-14 border" style={{ background: t.value }} aria-hidden />
              <span className="grid min-w-0 content-center gap-0.5">
                <span className="font-mono text-caption font-bold">--{t.name}</span>
                <span className="truncate font-mono text-label font-normal text-muted-foreground">
                  {t.value} {toHex(t.value)}
                </span>
                <span className="text-label font-normal text-muted-foreground">{t.role}</span>
              </span>
            </li>
          ))}
        </ul>
      </Block>

      <Block title="Contrast" lead="Computed from the OKLCH values (sRGB, gamut clipped, WCAG 2.2 relative luminance). Reading pairs reach 4.5:1; marks reach 3:1.">
        <Rows caption="Contrast ratios" head={["Pair", "Use", "Ratio"]}>
          {[...textPairs.map((p) => ({ ...p, min: 4.5 })), ...markPairs.map((p) => ({ ...p, min: 3 }))].map((p) => (
            <tr key={`${p.fg}-${p.bg}`} className="border-b">
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

      <Block title="Type" lead="Big Shoulders Display for headings and big figures, in capitals. Atkinson Hyperlegible Next for everything read, in sentence case; capitals elsewhere only for field labels.">
        <ul className="grid">
          {typeScale.map((t) => (
            <li key={t.role} className="grid gap-1 border-b py-3 sm:grid-cols-[7rem_1fr] sm:items-baseline">
              <span className="label-caps text-muted-foreground">{t.role}</span>
              <span className="grid gap-1">
                <span className={t.className}>{t.sample}</span>
                <span className="text-caption text-muted-foreground">{t.spec}</span>
              </span>
            </li>
          ))}
        </ul>
      </Block>

      <Block title="Space" lead="Tighter gutters and a wider column (the founder, 2026-09-28), with touch targets of at least 44 px on phones and reading text held to about 65 characters.">
        <Rows caption="Spacing tokens before and after" head={["Token", "Use", "Before", "After"]} min="40rem">
          {spacingTokens.map((s) => (
            <tr key={s.name} className="border-b">
              <th scope="row" className="py-2 pr-3 text-left font-mono text-caption font-normal">
                {s.name}
              </th>
              <td className="py-2 pr-3 text-muted-foreground">{s.use}</td>
              <td className="py-2 pr-3 font-mono tabular text-muted-foreground line-through decoration-1">{s.before}</td>
              <td className="py-2 text-right font-mono font-bold tabular">{s.after}</td>
            </tr>
          ))}
        </Rows>
        <p className="text-sm text-muted-foreground">Corners are square everywhere: a sign has no radius. Adjacent colour fields sit on a 6 px seam, never inside one another.</p>
      </Block>

      <Block title="States" lead="Every state has one flat treatment. Agent modes are fields; a restriction wears the colour of whoever imposed it; system states carry no meaning colour.">
        <div className="grid gap-(--section-gap)">
          <div className="grid gap-(--block-gap)">
            <h3 className="text-heading">Agent modes</h3>
            <ul className="grid gap-(--seam) sm:grid-cols-2 xl:grid-cols-4">
              {MODES.map((m) => (
                <li key={m} className={`grid min-h-28 content-between gap-3 p-4 transition-colors duration-(--duration-hover) ${MODE_FIELD[m]}`}>
                  <span className="font-display text-title uppercase">{MODE_LABEL[m]}</span>
                  <span className="text-sm">{MODE_MEANING[m]}</span>
                </li>
              ))}
            </ul>
          </div>
          <div className="grid gap-(--block-gap)">
            <h3 className="text-heading">Restrictions, by who imposed them</h3>
            <ul className="grid gap-(--seam) sm:grid-cols-2">
              {SOURCES.map(([s, text]) => (
                <li key={s} className={`grid gap-2 px-4 py-3 ${SOURCE_FIELD[s]}`}>
                  <SourceTag source={s} />
                  <span className="text-sm">
                    <span className="font-bold">{SOURCE_LABEL[s]}.</span> {text}
                  </span>
                </li>
              ))}
            </ul>
          </div>
          <div className="grid gap-(--block-gap)">
            <h3 className="text-heading">System states</h3>
            <ul className="grid gap-(--seam) lg:grid-cols-2">
              <li className="grid gap-2 bg-card p-4">
                <span className="label-caps text-muted-foreground">Stale</span>
                <AsOf at="2026-09-28T14:02:11-04:00" now="2026-09-28T14:05:20-04:00" stale />
                <p className="text-sm text-muted-foreground">The figure stays, labelled with its age. The status strip turns muted and counts what is degraded.</p>
              </li>
              <li className="grid gap-2 border-t-4 border-foreground bg-muted p-4">
                <span className="label-caps text-muted-foreground">Unreachable and error</span>
                <p className="font-display text-heading uppercase">Cannot reach your deployment</p>
                <p className="text-sm">A muted field under a heavy ink rule. No agent data is shown, and none is kept on this device.</p>
              </li>
              <li className="grid gap-2 bg-card p-4">
                <span className="label-caps text-muted-foreground">Loading</span>
                <div className="grid grid-cols-[1fr_1.2fr_1.3fr] gap-(--seam)" aria-hidden>
                  <Skeleton className="h-16" />
                  <Skeleton className="h-16" />
                  <Skeleton className="h-16 bg-marigold-soft" />
                </div>
                <p className="text-sm text-muted-foreground">Skeleton fields in the shape of the screen, never a value from an earlier visit.</p>
              </li>
              <li className="grid gap-2 bg-lapis p-4 text-lapis-foreground">
                <span className="label-caps text-lapis-muted">Empty</span>
                <p className="font-display text-heading uppercase">No agents yet</p>
                <p className="text-sm text-lapis-muted">An empty account is a lapis board with the one next step on it.</p>
              </li>
            </ul>
          </div>
          <div className="grid gap-(--block-gap)">
            <h3 className="text-heading">Mode banner</h3>
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

      <Block title="Components" lead="Kumo components in Placard tokens: square, flat, no shadows. Stop and kill-switch actions are never disabled, so no disabled state is shown for them.">
        <div className="grid gap-(--section-gap)">
          <div className="grid gap-(--block-gap)">
            <h3 className="text-heading">Actions</h3>
            <div className="flex flex-wrap items-center gap-2">
              <Button size="lg" variant="primary" className="h-11">
                Primary
              </Button>
              <Button size="lg" variant="secondary" className="h-11">
                Secondary
              </Button>
              <Button size="lg" variant="outline" className="h-11">
                Outline
              </Button>
              <Button size="lg" variant="ghost" className="h-11">
                Ghost
              </Button>
              <Button size="lg" variant="outline" className="h-11" disabled>
                Disabled
              </Button>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <StopControl />
              <span data-meaning="kill" className="inline-flex h-11 items-center bg-crimson px-4 font-bold text-crimson-foreground">
                Kill switch (crimson, only here)
              </span>
            </div>
            <p className="text-sm text-muted-foreground">Approve and Skip use the same secondary variant and size, side by side, with nothing preselected.</p>
          </div>
          <div className="grid gap-(--block-gap)">
            <h3 className="text-heading">Labels</h3>
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
          <div className="grid gap-(--seam) lg:grid-cols-2">
            <div className="grid content-start gap-3 bg-card p-4">
              <h3 className="text-heading">Figures</h3>
              <SignedMoney value="123.45" />
              <SignedMoney value="-67.89" />
              <SignedMoney value="0" />
              <AsOf at="2026-09-28T14:05:18-04:00" now="2026-09-28T14:05:20-04:00" />
              <Deadline deadline="2026-09-28T14:14:58-04:00" now="2026-09-28T14:05:20-04:00" />
            </div>
            <div className="grid content-start gap-4 bg-marigold p-4 text-marigold-foreground">
              <h3 className="text-heading">Your mandate</h3>
              <LimitRail rail={{ key: "a", label: "Total holdings", used: dec("1618.09"), cap: dec("2000"), atCap: "No new buys" }} />
              <LimitRail rail={{ key: "b", label: "Loss today", used: dec("0"), cap: dec("201"), atCap: "Exits only until a new risk day" }} />
              <LimitRail rail={{ key: "c", label: "Loss today", used: dec("214.5"), cap: dec("201"), atCap: "Exits only until a new risk day" }} />
            </div>
          </div>
        </div>
      </Block>

      <Block title="Charts" lead="TradingView Lightweight Charts in Placard: a solid background, one flat colour per fill, no animation, figures in Atkinson and times in ET. A level is a labelled line, never a progress bar, and every level is also listed in words.">
        <div className="grid gap-(--block-gap) lg:grid-cols-2">
          <ul className="grid gap-2 text-sm">
            {CHART_RULES.map(([swatch, name, use]) => (
              <li key={name} className="grid grid-cols-[1rem_minmax(0,1fr)] items-baseline gap-2">
                <span aria-hidden className={`size-4 self-center ${swatch}`} />
                <span>
                  <span className="font-bold">{name}</span>: {use}
                </span>
              </li>
            ))}
          </ul>
          <div className="grid content-start gap-3 bg-card p-4">
            <Sparkline points={SAMPLE_POINTS} limit={9760} label="Sample sparkline: equity rising over the day, above the daily loss limit" width={240} height={56} />
            <LevelLegend levels={SAMPLE_LEVELS} />
            <p className="text-caption text-muted-foreground">
              Loading draws the chart&apos;s outline and no line; stale data keeps its age beside the chart; if the canvas cannot be drawn, the chart says so and the figures stay.
            </p>
            <ChartCredit />
          </div>
        </div>
      </Block>

      <Block
        title="Kumo surfaces, flattened"
        lead="Kumo paints an overlay on emphasis buttons, fades on sticky table cells and tab scroll buttons, masks on scrolling regions, and a shimmer on skeletons. Placard flattens every one (DEC-200); a browser test reads the computed styles of these specimens."
      >
        <KumoSurfaces />
      </Block>

      <Block title="Motion" lead="Motion answers an action or shows what changed, and never lasts past 300 ms. Deadlines and anything that could nudge a decision never move. Reduced motion keeps colour changes and drops movement.">
        <Rows caption="Motion tokens" head={["Token", "Value", "Use"]} min="32rem">
          {motionTokens.map((m) => (
            <tr key={m.name} className="border-b">
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
    </div>
  );
}
