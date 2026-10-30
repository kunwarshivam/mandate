import type { NewAgent } from "@/lib/fixture-journey";

/**
 * A new agent's request as the fixture journey's tests start from it: $3,000 on paper in MSFT on the
 * momentum model, with a $300 loss limit, on `connectionId`. It is the mandate the old set-up's
 * readers built from those answers, kept as data now that a model reads the owner's words (DEC-2293).
 */
export function newAgentRequest(connectionId: string): NewAgent {
  return {
    mandate: {
      mandate_schema_version: 1,
      name: "msft-agent",
      source_text_ref:
        "sha256:dd384d63ca6a04529f39d507c866c45e1fb0c2d29416dda93098556b391a4ace",
      environment: "paper",
      connection_id: connectionId,
      capital: {
        allocation_usd: "3000",
        max_loss_from_allocation: "0.1",
      },
      goal: {
        type: "continuous",
        end_date: null,
        on_complete: "hold_protected",
      },
      universe: {
        pinned: true,
        pinned_instruments: [
          {
            asset_id: "2764d836-e365-4e13-996c-f5e6359cd854",
            symbol: "MSFT",
            asset_class: "us_equity",
          },
        ],
        max_instruments: 1,
        asset_classes: ["us_equity"],
        leveraged_etps_enabled: false,
        leveraged_etp_disclosure_version: null,
      },
      behavior: {
        description: "Grow it",
        signal_models: [
          {
            id: "quant.momentum",
            version: "1.0.0",
            content_hash:
              "sha256:2222222222222222222222222222222222222222222222222222222222222222",
            params: [
              {
                key: "lookback_bars",
                value: "20",
              },
            ],
            weight: "1",
            max_output_age_s: 900,
            admits_instruments: false,
          },
        ],
        research: null,
        cadence: {
          interval_s: 300,
          event_sources: ["price"],
        },
        sizing: {
          method: "conviction_linear",
          entry_threshold: "0.3",
          exit_threshold: "0.3",
          rebalance_band: "0.05",
        },
      },
      protection: {
        enabled: true,
        stop_distance: "0.08",
        take_profit_distance: null,
        crypto_stop_limit_offset: null,
      },
      risk: {
        max_position_usd: "750",
        max_position_fraction: "0.25",
        max_gross_exposure_usd: "3000",
        max_order_usd: "300",
        max_orders_per_day: 10,
        max_daily_loss: "0.025",
        daily_loss_action: "exits_only",
        max_drawdown: "0.05",
        drawdown_ladder: [
          {
            at: "0.05",
            action: "flatten_and_pause",
            factor: null,
          },
        ],
        hysteresis: "0.01",
        scale_action: "limit_buys",
        breach_confirm_s: 60,
        daily_breach_min_s: 3600,
        scale_lift_after_s: 600,
        reentry_cooldown_s: 3600,
      },
      autonomy: {
        rules: [],
        default: "ask",
        admission: "ask",
        approval: {
          timeout_s: 300,
          on_timeout: "skip",
          approvers: ["role:owner"],
          two_approver_above_usd: null,
        },
      },
      notifications: {
        channels: ["web_push"],
        quiet_hours: null,
      },
    },
    provenance: [
      {
        path: "/capital/allocation_usd",
        provenance: "user_stated",
        quote: "$3,000",
      },
      {
        path: "/capital/max_loss_from_allocation",
        provenance: "user_stated",
        quote: "$300",
      },
      {
        path: "/goal/end_date",
        provenance: "platform_proposed",
      },
      {
        path: "/environment",
        provenance: "platform_default",
      },
      {
        path: "/risk/max_daily_loss",
        provenance: "platform_proposed",
      },
      {
        path: "/risk/max_drawdown",
        provenance: "platform_proposed",
      },
      {
        path: "/risk/max_position_usd",
        provenance: "platform_proposed",
      },
      {
        path: "/risk/max_order_usd",
        provenance: "platform_proposed",
      },
      {
        path: "/risk/max_orders_per_day",
        provenance: "platform_proposed",
      },
      {
        path: "/risk/max_gross_exposure_usd",
        provenance: "platform_proposed",
      },
      {
        path: "/protection/stop_distance",
        provenance: "platform_proposed",
      },
      {
        path: "/risk/scale_action",
        provenance: "platform_proposed",
      },
      {
        path: "/risk/breach_confirm_s",
        provenance: "platform_proposed",
      },
      {
        path: "/risk/reentry_cooldown_s",
        provenance: "platform_proposed",
      },
      {
        path: "/behavior/signal_models/0/id",
        provenance: "user_entered",
      },
      {
        path: "/behavior/signal_models/0/params",
        provenance: "user_entered",
      },
      {
        path: "/behavior/signal_models/0/weight",
        provenance: "platform_default",
      },
      {
        path: "/behavior/cadence/interval_s",
        provenance: "platform_proposed",
      },
      {
        path: "/behavior/sizing/entry_threshold",
        provenance: "platform_proposed",
      },
      {
        path: "/autonomy/default",
        provenance: "platform_default",
      },
      {
        path: "/autonomy/approval/timeout_s",
        provenance: "platform_proposed",
      },
      {
        path: "/autonomy/approval/on_timeout",
        provenance: "platform_default",
      },
      {
        path: "/autonomy/approval/approvers",
        provenance: "platform_default",
      },
      {
        path: "/notifications/channels",
        provenance: "platform_default",
      },
      {
        path: "/universe/pinned_instruments",
        provenance: "user_entered",
      },
      {
        path: "/universe/asset_classes",
        provenance: "platform_proposed",
      },
      {
        path: "/universe/max_instruments",
        provenance: "platform_proposed",
      },
      {
        path: "/universe/leveraged_etps_enabled",
        provenance: "platform_default",
      },
      {
        path: "/risk/hysteresis",
        provenance: "platform_default",
      },
      {
        path: "/risk/daily_breach_min_s",
        provenance: "platform_default",
      },
      {
        path: "/risk/scale_lift_after_s",
        provenance: "platform_default",
      },
    ],
  };
}
