"use client";

import { Button, LinkButton } from "@cloudflare/kumo/components/button";
import { LayerDialog } from "@cloudflare/kumo/components/layer-dialog";
import { SkeletonLine } from "@cloudflare/kumo/components/loader";
import { Table } from "@cloudflare/kumo/components/table";
import { Tabs } from "@cloudflare/kumo/components/tabs";
import { KEY } from "@/components/kumo/key";

const ROWS = ["ABC", "XYZ", "LMN", "BTC", "ETH", "QRS", "TUV", "DEF"];
const TABS = ["Overview", "Positions", "Orders", "Decisions", "Approvals", "Mandate", "Prove", "Activity"].map((label) => ({ value: label.toLowerCase(), label }));

/**
 * One of each Kumo surface that paints a blend, fade, mask or shimmer before the theme flattens it:
 * the emphasis button's overlay, sticky table cells, overflowing tabs, skeleton lines, and the layer
 * dialog. The browser suite in `e2e/` reads their computed styles here.
 */
export function KumoSurfaces() {
  return (
    <div className="grid gap-(--block-gap)" data-specimen="kumo">
      <div className="flex flex-wrap items-center gap-2" data-specimen="button">
        <Button size="lg" variant="secondary" className={KEY}>
          Primary button
        </Button>
        <LinkButton href="/design" size="lg" variant="secondary" className={KEY}>
          Primary link
        </LinkButton>
      </div>

      <div className="max-h-48 max-w-md overflow-auto border" data-specimen="table">
        <Table className="min-w-[36rem]">
          <Table.Header sticky>
            <Table.Row>
              <Table.Head sticky="left">Instrument</Table.Head>
              <Table.Head>Quantity</Table.Head>
              <Table.Head>Mark</Table.Head>
              <Table.Head sticky="right">Value</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {ROWS.map((symbol, i) => (
              <Table.Row key={symbol}>
                <Table.Cell sticky="left">{symbol}</Table.Cell>
                <Table.Cell className="font-mono tabular">{(i + 1) * 5}</Table.Cell>
                <Table.Cell className="font-mono tabular">{(100 + i * 7).toFixed(2)}</Table.Cell>
                <Table.Cell sticky="right" className="font-mono tabular">
                  {((i + 1) * 5 * (100 + i * 7)).toFixed(2)}
                </Table.Cell>
              </Table.Row>
            ))}
          </Table.Body>
        </Table>
      </div>

      <div className="max-w-64" data-specimen="tabs">
        <Tabs variant="underline" tabs={TABS} value="overview" />
      </div>

      <div className="grid max-w-md gap-2" data-specimen="skeleton" aria-hidden>
        <SkeletonLine minWidth={90} maxWidth={90} minDuration={1.5} maxDuration={1.5} minDelay={0} maxDelay={0} />
        <SkeletonLine minWidth={60} maxWidth={60} minDuration={1.5} maxDuration={1.5} minDelay={0} maxDelay={0} />
      </div>

      <div data-specimen="layer-dialog">
        <LayerDialog.Root>
          <LayerDialog.Trigger render={<Button size="lg" variant="secondary" className={KEY} />}>Open a layer dialog</LayerDialog.Trigger>
          <LayerDialog.Content>
            <LayerDialog.Title>Layer dialog</LayerDialog.Title>
            <LayerDialog.Description>For short admin actions only; record screens are pages.</LayerDialog.Description>
            <LayerDialog.Body>
              <div className="grid gap-2">
                {Array.from({ length: 24 }, (_, i) => (
                  <p key={i}>Line {i + 1} of a body long enough to scroll.</p>
                ))}
              </div>
            </LayerDialog.Body>
            <LayerDialog.Actions>
              <LayerDialog.Actions.Primary>Save</LayerDialog.Actions.Primary>
            </LayerDialog.Actions>
          </LayerDialog.Content>
        </LayerDialog.Root>
      </div>
    </div>
  );
}
