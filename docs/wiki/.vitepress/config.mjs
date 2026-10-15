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
      { text: "Getting Started", link: "/how-a-match-works" },
      { text: "Mechanics", link: "/deployment-and-mobility" },
      { text: "Units", link: "/units" },
      { text: "Glossary", link: "/glossary" },
      { text: `Game ${gameVersion}`, link: "https://github.com/qbisi/mechcore" },
    ],
    sidebar: [
      {
        text: "Getting Started",
        items: [
          { text: "Overview & Concept Portal", link: "/" },
          { text: "How a Match Works", link: "/how-a-match-works" },
          { text: "A Round's Decisions", link: "/a-rounds-decisions" },
          { text: "Master Concept Glossary", link: "/glossary" },
        ],
      },
      {
        text: "Game Mechanics",
        items: [
          {
            text: "Deployment, Flanks, and Mobility",
            link: "/deployment-and-mobility",
          },
          {
            text: "Experience & Unit Levels",
            link: "/experience-and-levels",
          },
          {
            text: "Reactor Core Settlement",
            link: "/reactor-damage",
          },
        ],
      },
      {
        text: "Game Encyclopedia",
        items: [
          {
            text: "Units & Battlefield Objects",
            link: "/units",
          },
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
