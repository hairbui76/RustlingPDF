import { resolve } from "node:path";
import type { StorybookConfig } from "@storybook/react-vite";
import tsconfigPaths from "vite-tsconfig-paths";

const config: StorybookConfig = {
  stories: ["../editor/src/core/**/*.stories.@(ts|tsx|mdx)"],
  addons: ["@storybook/addon-themes", "@storybook/addon-a11y"],
  framework: {
    name: "@storybook/react-vite",
    options: {},
  },
  typescript: {
    reactDocgen: "react-docgen-typescript",
  },
  staticDirs: ["../editor/public"],
  viteFinal: async (viteConfig) => {
    viteConfig.resolve = viteConfig.resolve ?? {};
    viteConfig.resolve.alias = {
      ...(viteConfig.resolve.alias ?? {}),
      "@core": resolve(__dirname, "../editor/src/core"),
      "@public": resolve(__dirname, "../editor/public"),
    };
    viteConfig.plugins = viteConfig.plugins ?? [];
    viteConfig.plugins.push(
      tsconfigPaths({
        projects: [resolve(__dirname, "../editor/tsconfig.core.vite.json")],
      }),
    );
    return viteConfig;
  },
};

export default config;
