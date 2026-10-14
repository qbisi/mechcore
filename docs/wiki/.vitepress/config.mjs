import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../../..");
const gameVersion = fs
  .readFileSync(path.join(repoRoot, "GAME_VERSION"), "utf8")
  .trim();

export default {
  title: "Mechcore Wiki",
  description:
    "Player- and agent-facing guide to Mechabellum mechanics, grounded in the native game build.",
  lang: "en-US",
  base: "/mechcore/",
  outDir: "../../work/pages",
  srcExclude: ["**/README.md"],
  cleanUrls: true,
  appearance: "dark",
  themeConfig: {
    siteTitle: "Mechcore Wiki",
    nav: [
      { text: "Home", link: "/" },
      { text: "How a Match Works", link: "/how-a-match-works" },
      { text: `Game ${gameVersion}`, link: "https://github.com/qbisi/mechcore" },
    ],
    sidebar: [
      {
        text: "Getting Started",
        items: [
          { text: "Overview", link: "/" },
          { text: "How a Match Works", link: "/how-a-match-works" },
        ],
      },
    ],
    search: {
      provider: "local",
    },
    outline: {
      level: [2, 3],
    },
    socialLinks: [
      { icon: "github", link: "https://github.com/qbisi/mechcore" },
    ],
    footer: {
      message: `Pinned to native Mechabellum build ${gameVersion} (standard 1v1).`,
    },
  },
};
