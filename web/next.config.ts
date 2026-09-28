import type { NextConfig } from "next";
import { devFrameHeaders, pageExtensions } from "./src/lib/dev-routes";

const nextConfig: NextConfig = {
  poweredByHeader: false,
  reactStrictMode: true,
  devIndicators: false,
  agentRules: false,
  pageExtensions: pageExtensions(process.env.NODE_ENV),
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
      ...devFrameHeaders(process.env.NODE_ENV),
    ];
  },
};

export default nextConfig;
