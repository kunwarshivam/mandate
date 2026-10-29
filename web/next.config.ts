import type { NextConfig } from "next";
import { E2E_FLAG, distDir, isE2eBuild } from "./scripts/e2e-build.mjs";
import { refuseWaiverOutsideDev, stagingWaiverUntil } from "./src/lib/cool-off-build";
import { devOnlyAliases, pageExtensions } from "./src/lib/dev-routes";

refuseWaiverOutsideDev(process.env.NODE_ENV, process.env.NEXT_PUBLIC_OWLHEAD_COOL_OFF_WAIVED);
stagingWaiverUntil(process.env.NEXT_PUBLIC_OWLHEAD_STAGING_COOL_OFF_UNTIL);

const nextConfig: NextConfig = {
  poweredByHeader: false,
  pageExtensions: pageExtensions(process.env.NODE_ENV),
  distDir: distDir(),
  env: { [E2E_FLAG]: isE2eBuild() ? "1" : "0" },
  turbopack: { resolveAlias: devOnlyAliases(process.env.NODE_ENV) },
  reactStrictMode: true,
  devIndicators: false,
  // Cloudflare Workers serves images as they are; nothing optimises them on request (DEC-731 item 2).
  images: { unoptimized: true },
  agentRules: false,
  experimental: {
    optimizePackageImports: ["@cloudflare/kumo", "@phosphor-icons/react"],
  },
  async headers() {
    return [
      {
        source: "/:path*",
        headers: [
          { key: "Referrer-Policy", value: "no-referrer" },
          { key: "X-Content-Type-Options", value: "nosniff" },
          { key: "X-Frame-Options", value: "DENY" },
        ],
      },
      {
        source: "/demo",
        headers: [
          { key: "X-Frame-Options", value: "SAMEORIGIN" },
          { key: "Content-Security-Policy", value: "frame-ancestors 'self'" },
        ],
      },
    ];
  },
};

export default nextConfig;
