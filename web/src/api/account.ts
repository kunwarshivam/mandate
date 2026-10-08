/**
 * The account figure on the workspace API (E11-9; workspace API spec §4.5, API-11): `GET
 * /connections/{id}`, decoded strictly. D1's account board shows the single equity figure with its
 * freshness; the equity series the chart draws waits for a read model of its own (DEC-737).
 */
import type { Environment } from "@/fixtures/types";
import type { Decoder, Timestamp } from "./types";
import type { Freshness, FreshnessView } from "./workspace-source";

export interface ConnectionReadModel {
  served_at: Timestamp;
  connection: {
    connection_id: string;
    broker: "alpaca" | "robinhood" | "kraken_derivatives_us";
    environment: Environment;
    state: "connecting" | "active" | "degraded" | "suspended" | "revoked";
    account: { state: "normal" | "closing_only" | "blocked"; equity: string; day_trading_regime: "intraday_margin"; freshness: Freshness };
  };
}

export interface AccountEquity {
  connection_id: string;
  environment: Environment;
  equity: string;
  freshness: FreshnessView;
  account_state: ConnectionReadModel["connection"]["account"]["state"];
}

export const decodeConnection: Decoder<ConnectionReadModel> = () => {
  throw new Error("Unimplemented: E11-9");
};

export function accountEquity(connection: ConnectionReadModel): AccountEquity {
  void connection;
  throw new Error("Unimplemented: E11-9");
}
