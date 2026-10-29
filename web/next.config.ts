import type { NextConfig } from "next";

// When set, the browser talks to this origin only and Next forwards /api/v1 to the backend
// (no CORS, and the backend port need not be reachable from the browser, e.g. in Codespaces).
const apiProxyTarget = process.env.API_PROXY_TARGET;

const config: NextConfig = {
  reactStrictMode: true,
  devIndicators: false,
  experimental: {
    optimizePackageImports: ["lucide-react"],
  },
  async rewrites() {
    if (!apiProxyTarget) return [];
    return [{ source: "/api/v1/:path*", destination: `${apiProxyTarget}/api/v1/:path*` }];
  },
};

export default config;
