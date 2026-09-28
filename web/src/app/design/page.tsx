import type { Metadata } from "next";
import type { ReactNode } from "react";
import { cn } from "cn";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Deadline } from "@/components/approvals/deadline";
import { MotionSamples } from "@/components/design/motion-samples";
import { AsOf } from "@/components/domain/as-of";
import { LimitRail } from "@/components/domain/envelope";
import { ModeBadge, ModeBanner } from "@/components/domain/mode";
import { SignedMoney } from "@/components/domain/money";
import { FixtureTag, Placeholder } from "@/components/domain/placeholders";
import { ProvenanceBadge } from "@/components/domain/provenance-badge";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { StopControl } from "@/components/shell/stop-control";
import type { Provenance } from "@/fixtures/types";
import { contrastRatio, toHex } from "@/lib/color";
import { dec } from "@/lib/decimal";
import { type Theme, colorTokens, motionTokens, textPairs, tokenValue, typeScale } from "@/lib/tokens";

export const metadata: Metadata = { title: "Design system" };

const PROVENANCES: Provenance[] = ["user_stated", "user_entered", "template_structure", "platform_proposed", "platform_default"];
const RADII = [
  ["xs", "0.375rem", "rounded-xs"],
  ["sm", "0.5rem", "rounded-sm"],
  ["md", "0.625rem", "rounded-md"],
  ["lg", "0.875rem", "rounded-lg"],
  ["xl", "1.25rem", "rounded-xl"],
  ["2xl", "1.75rem", "rounded-2xl"],
] as const;
const SPACING = [1, 2, 3, 4, 6, 8, 10, 12, 16];

function Block({ title, lead, children }: { title: string; lead?: string; children: ReactNode }) {
  return (
    <section className="grid gap-4" aria-labelledby={`design-${title}`}>
      <div className="grid gap-1">
        <h2 id={`design-${title}`} className="text-title">
          {title}
        </h2>
        {lead ? <p className="max-w-prose text-muted-foreground">{lead}</p> : null}
      </div>
      {children}
    </section>
  );
}

function Swatches({ theme }: { theme: Theme }) {
  return (
    <ul className={cn("grid gap-3 rounded-xl p-3 sm:grid-cols-2 xl:grid-cols-3", theme === "dark" ? "dark bg-background" : "bg-background")}>
      {colorTokens.map((t) => (
        <li key={t.name} className="grid grid-cols-[3.5rem_1fr] gap-3 rounded-lg border bg-card p-2.5 text-card-foreground">
          <span className="h-14 rounded-md border" style={{ background: t[theme] }} aria-hidden />
          <span className="grid min-w-0 content-center gap-0.5">
            <span className="font-mono text-caption font-semibold">--{t.name}</span>
            <span className="truncate font-mono text-[0.6875rem] text-muted-foreground">
              {t[theme]} {toHex(t[theme])}
            </span>
            <span className="text-[0.6875rem] text-muted-foreground">{t.role}</span>
          </span>
        </li>
      ))}
    </ul>
  );
}

export default function DesignPage() {
  return (
    <div className="grid gap-14">
      <header className="grid gap-2">
        <h1 className="text-title sm:text-display">Design system</h1>
        <p className="max-w-prose text-muted-foreground">
          Internal reference for the envelope art direction: tokens, type, components in their states, and motion. Colour values live in <code>src/lib/tokens.ts</code> and{" "}
          <code>globals.css</code>; a test fails if they drift or if any reading pair drops below WCAG AA.
        </p>
      </header>

      <Block title="The envelope" lead="Ultramarine to orchid to persimmon, with fine grain and a 32 s drift. It frames content and never sits behind dense figures.">
        <div className="mesh drift grain grid h-56 place-items-end rounded-2xl p-3">
          <p className="rounded-lg bg-card/95 px-3 py-2 text-sm">Only light text blocks sit on the gradient, on a card surface.</p>
        </div>
      </Block>

      <Block title="Paper hatch" lead="Diagonal persimmon lines mark simulated funds. The badge appears in the header of every screen.">
        <div className="flex flex-wrap items-center gap-4">
          <EnvironmentBadge environment="paper" />
          <EnvironmentBadge environment="live" />
          <div className="hatch h-16 w-40 rounded-lg border border-persimmon/60" aria-hidden />
        </div>
      </Block>

      <Block title="Colour" lead="OKLCH throughout. Crimson is reserved for the kill switch; orchid marks platform-authored labels; gains are lagoon and losses rose, always with a sign and a word.">
        <Tabs defaultValue="light">
          <TabsList>
            <TabsTrigger value="light">Porcelain (light)</TabsTrigger>
            <TabsTrigger value="dark">Midnight (dark)</TabsTrigger>
          </TabsList>
          <TabsContent value="light">
            <Swatches theme="light" />
          </TabsContent>
          <TabsContent value="dark">
            <Swatches theme="dark" />
          </TabsContent>
        </Tabs>
      </Block>

      <Block title="Contrast" lead="Computed from the OKLCH values (sRGB, gamut clipped, WCAG 2.2 relative luminance). Every reading pair reaches AA, 4.5:1.">
        <div className="-mx-4 overflow-x-auto px-4">
          <table className="w-full min-w-[36rem] text-sm">
            <caption className="sr-only">Contrast ratios of text pairs</caption>
            <thead>
              <tr className="border-b text-left text-caption text-muted-foreground">
                <th scope="col" className="py-2 pr-3 font-medium">Text on surface</th>
                <th scope="col" className="py-2 pr-3 font-medium">Use</th>
                <th scope="col" className="py-2 pr-3 text-right font-medium">Light</th>
                <th scope="col" className="py-2 text-right font-medium">Dark</th>
              </tr>
            </thead>
            <tbody>
              {textPairs.map((p) => {
                const light = contrastRatio(tokenValue(p.fg, "light"), tokenValue(p.bg, "light"));
                const dark = contrastRatio(tokenValue(p.fg, "dark"), tokenValue(p.bg, "dark"));
                return (
                  <tr key={`${p.fg}-${p.bg}`} className="border-b border-border/60">
                    <th scope="row" className="py-2 pr-3 text-left font-mono text-caption font-normal">
                      {p.fg} / {p.bg}
                    </th>
                    <td className="py-2 pr-3 text-muted-foreground">{p.use}</td>
                    <td className="py-2 pr-3 text-right font-mono tabular">{light.toFixed(2)}:1</td>
                    <td className="py-2 text-right font-mono tabular">{dark.toFixed(2)}:1</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </Block>

      <Block title="Type" lead="Bricolage Grotesque for display, Hanken Grotesk for reading, JetBrains Mono with tabular figures for every number. Self-hosted through Fontsource.">
        <ul className="grid gap-4">
          {typeScale.map((t) => (
            <li key={t.role} className="grid gap-1 border-b pb-4 sm:grid-cols-[8rem_1fr] sm:items-baseline">
              <span className="text-caption text-muted-foreground">{t.role}</span>
              <span className="grid gap-1">
                <span className={t.role === "display" ? "font-display text-display tabular" : t.role === "title" || t.role === "heading" ? `font-display ${t.className}` : t.className}>
                  {t.sample}
                </span>
                <span className="text-caption text-muted-foreground">{t.spec}</span>
              </span>
            </li>
          ))}
        </ul>
      </Block>

      <Block title="Space and radius" lead="Spacing is Tailwind's 0.25 rem step. Radius grows with the size of the surface: chips small, cards large.">
        <div className="grid gap-6 lg:grid-cols-2">
          <ul className="grid gap-2">
            {SPACING.map((s) => (
              <li key={s} className="flex items-center gap-3 text-caption">
                <span className="w-16 font-mono text-muted-foreground">{s * 0.25}rem</span>
                <span className="h-3 rounded-xs bg-ultramarine" style={{ width: `${s * 0.25}rem` }} aria-hidden />
              </li>
            ))}
          </ul>
          <ul className="grid grid-cols-3 gap-3">
            {RADII.map(([name, value, cls]) => (
              <li key={name} className="grid gap-1.5 text-caption">
                <span className={cn("h-14 border bg-muted", cls)} aria-hidden />
                <span className="font-mono">
                  {name} {value}
                </span>
              </li>
            ))}
          </ul>
        </div>
      </Block>

      <Block title="Components" lead="shadcn/ui primitives in the brand's tokens. Stop and kill-switch actions are never disabled, so no disabled state is shown for them.">
        <div className="grid gap-8">
          <div className="grid gap-3">
            <h3 className="text-heading">Buttons</h3>
            <div className="flex flex-wrap items-center gap-3">
              <Button size="lg" className="press">Primary</Button>
              <Button size="lg" variant="outline" className="press">Outline</Button>
              <Button size="lg" variant="secondary" className="press">Secondary</Button>
              <Button size="lg" variant="ghost" className="press">Ghost</Button>
              <Button size="lg" variant="link">Link</Button>
              <Button size="lg" className="ring-3 ring-ring/50">Focus ring</Button>
              <Button size="lg" variant="outline" disabled>Disabled</Button>
            </div>
            <div className="flex flex-wrap items-center gap-3">
              <StopControl />
              <span className="inline-flex h-10 items-center rounded-lg bg-crimson px-4 text-sm font-semibold text-crimson-foreground">Kill switch (crimson, only here)</span>
            </div>
            <p className="text-caption text-muted-foreground">Approve and Skip use the same outline variant and size, side by side, with nothing preselected.</p>
          </div>
          <Separator />
          <div className="grid gap-3">
            <h3 className="text-heading">Badges</h3>
            <div className="flex flex-wrap items-center gap-2">
              <ModeBadge mode="normal" />
              <ModeBadge mode="exits_only" />
              <ModeBadge mode="paused" />
              <ModeBadge mode="stopped" />
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
          <Separator />
          <div className="grid gap-4 lg:grid-cols-2">
            <div className="grid gap-3">
              <h3 className="text-heading">Figures</h3>
              <SignedMoney value="123.45" />
              <SignedMoney value="-67.89" />
              <SignedMoney value="0" />
              <AsOf at="2026-09-28T14:05:18-04:00" now="2026-09-28T14:05:20-04:00" />
              <AsOf at="2026-09-28T14:02:11-04:00" now="2026-09-28T14:05:20-04:00" stale />
              <Deadline deadline="2026-09-28T14:14:58-04:00" now="2026-09-28T14:05:20-04:00" />
            </div>
            <div className="grid gap-4">
              <h3 className="text-heading">Rails</h3>
              <LimitRail rail={{ key: "a", label: "Total holdings", used: dec("1618.09"), cap: dec("2000"), atCap: "No new buys" }} />
              <LimitRail rail={{ key: "b", label: "Loss today", used: dec("0"), cap: dec("201"), atCap: "Exits only until a new risk day" }} />
              <LimitRail rail={{ key: "c", label: "Loss today", used: dec("214.5"), cap: dec("201"), atCap: "Exits only until a new risk day" }} />
            </div>
          </div>
          <Separator />
          <div className="grid gap-3">
            <h3 className="text-heading">Mode banner</h3>
            <ModeBanner
              mode="exits_only"
              restrictions={[
                { code: "drawdown_scale_sizes", since: "2026-09-26T15:12:40-04:00" },
                { code: "drawdown_exits_only", since: "2026-09-28T14:01:12-04:00" },
              ]}
            />
          </div>
          <Separator />
          <div className="grid gap-3">
            <h3 className="text-heading">Loading</h3>
            <div className="grid grid-cols-3 gap-3">
              <Skeleton className="h-20 rounded-xl" />
              <Skeleton className="h-20 rounded-xl" />
              <Skeleton className="h-20 rounded-xl" />
            </div>
          </div>
        </div>
      </Block>

      <Block title="Motion" lead="Motion answers an action or shows what changed. Deadlines and anything that could nudge a decision never move. Everything respects prefers-reduced-motion.">
        <div className="-mx-4 overflow-x-auto px-4">
          <table className="w-full min-w-[32rem] text-sm">
            <caption className="sr-only">Motion tokens</caption>
            <tbody>
              {motionTokens.map((m) => (
                <tr key={m.name} className="border-b border-border/60">
                  <th scope="row" className="py-2 pr-3 text-left font-mono text-caption font-normal">
                    {m.name}
                  </th>
                  <td className="py-2 pr-3 font-mono text-caption">{m.value}</td>
                  <td className="py-2 text-muted-foreground">{m.use}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <MotionSamples />
      </Block>
    </div>
  );
}
