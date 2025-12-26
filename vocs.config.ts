import { defineConfig } from "vocs";

export default defineConfig({
  title: "🦀 zkip",
  baseUrl: "https://zkip.nahiko.dev",
  description: "Zero-knowledge proof library for proving IP location privacy.",
  iconUrl: "/favicon.svg",
  sidebar: [
    {
      text: "Introduction",
      items: [
        {
          text: "What is zkip?",
          link: "/introduction/what-is-zkip",
        },
        {
          text: "Privacy Model",
          link: "/introduction/privacy",
        },
      ],
    },
    {
      text: "Getting Started",
      items: [
        {
          text: "Installation",
          link: "/getting-started/installation",
        },
        {
          text: "Quick Start",
          link: "/getting-started/quick-start",
        },
      ],
    },
    {
      text: "Architecture",
      items: [
        {
          text: "Overview",
          link: "/architecture/overview",
        },
        {
          text: "Flow",
          link: "/architecture/flow",
        },
      ],
    },
    {
      text: "API",
      items: [
        {
          text: "Reference",
          link: "/api/reference",
        },
      ],
    },
    {
      text: "Coming soon..",
      items: [
        {
          text: "REST API",
          link: "/coming-soon/rest-api",
        },
        {
          text: "On-Chain Verification",
          link: "/coming-soon/on-chain",
        },
      ],
    },
    {
      text: "Changelog",
      link: "/changelog",
    },
  ],
  topNav: [
    { text: "Docs", link: "/getting-started/installation", match: "/getting-started" },
    { text: "Changelog", link: "/changelog" },
  ],
  socials: [
    {
      icon: "github",
      link: "https://github.com/nahiiko/zkip",
    },
  ],
});
