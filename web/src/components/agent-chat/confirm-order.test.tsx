import { act, fireEvent, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ConfirmOrderCard } from "@/api/chat";
import { AGENT_IDS } from "@/fixtures/workspace";
import { renderWithRuntime } from "@/test/harness";
import { ConfirmOrderView } from "./cards";

const CARD: ConfirmOrderCard = {
  kind: "confirm_order",
  card_id: "01JCARD000000000000000000",
  symbol: "AAPL",
  instrument_id: "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415",
  qty: "2",
  reference_price: "190.5",
  estimated_cost_usd: "381",
  note: "Nothing is placed until you confirm. The platform may clip the quantity to your mandate's caps and sets the limit price when it places the order.",
};

const card = () => screen.getByRole("region", { name: "Confirm your order" });
const status = () => screen.getByRole("status");

describe("the chat's order card (DEC-2859)", () => {
  it("shows what would be placed, and that nothing is placed until the owner confirms", () => {
    renderWithRuntime(<ConfirmOrderView card={CARD} agentId={AGENT_IDS.swing} messageId="m1" />);
    expect(card()).toHaveTextContent("Buy 2 AAPL");
    expect(card()).toHaveTextContent("$190.50, last daily close");
    expect(card()).toHaveTextContent("$381.00");
    expect(card()).toHaveTextContent("may clip the quantity");
    expect(screen.getByRole("button", { name: "Confirm" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Dismiss" })).toBeInTheDocument();
  });

  it("says when no trusted price is served rather than inventing one", () => {
    renderWithRuntime(<ConfirmOrderView card={{ ...CARD, reference_price: null, estimated_cost_usd: null }} agentId={AGENT_IDS.swing} messageId="m1" />);
    expect(card()).toHaveTextContent("No trusted price yet");
    expect(card()).toHaveTextContent("Not known yet");
  });

  it("confirms once: the tap is recorded, then the buttons are gone", async () => {
    renderWithRuntime(<ConfirmOrderView card={CARD} agentId={AGENT_IDS.swing} messageId="m1" />);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Confirm" })));
    expect(status()).toHaveTextContent("Confirmed.");
    expect(screen.queryByRole("button", { name: "Confirm" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Dismiss" })).toBeNull();
  });

  it("shows a refusal with its reason, and nothing placed", async () => {
    renderWithRuntime(<ConfirmOrderView card={CARD} agentId="agt_unknown" messageId="m1" />);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Confirm" })));
    expect(status()).toHaveTextContent("Refused: not found. Nothing was placed.");
    expect(screen.queryByRole("button", { name: "Confirm" })).toBeNull();
  });

  it("dismisses without sending anything", async () => {
    renderWithRuntime(<ConfirmOrderView card={CARD} agentId={AGENT_IDS.swing} messageId="m1" />);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Dismiss" })));
    expect(status()).toHaveTextContent("Dismissed. Nothing was sent.");
    expect(screen.queryByRole("button", { name: "Confirm" })).toBeNull();
  });
});
