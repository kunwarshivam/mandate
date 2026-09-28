import type { NextConfig } from "next";
import { pageExtensions } from "./src/lib/dev-routes";

const nextConfig: NextConfig = {
  poweredByHeader: false,
  pageExtensions: pageExtensions(process.env.NODE_ENV),
  reactStrictMode: true,
  devIndicators: false,
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
    ];
  },
};

export default nextConfig;
