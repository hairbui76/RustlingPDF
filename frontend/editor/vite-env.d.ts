/// <reference types="vite/client" />

interface ImportMetaEnv {
  // Used by all builds (.env)
  readonly VITE_API_BASE_URL: string;
  readonly VITE_GOOGLE_DRIVE_CLIENT_ID: string;
  readonly VITE_GOOGLE_DRIVE_API_KEY: string;
  readonly VITE_GOOGLE_DRIVE_APP_ID: string;
  readonly VITE_PUBLIC_POSTHOG_KEY: string;
  readonly VITE_PUBLIC_POSTHOG_HOST: string;

  // Desktop only (.env.desktop)
  readonly VITE_DESKTOP_BACKEND_URL: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

/**
 * Dev-only worktree folder basename injected by vite.config at dev-serve time
 * (empty string in production builds). Used to prefix the browser tab title so
 * concurrent worktrees are distinguishable.
 */
declare const __DEV_WORKTREE_LABEL__: string;
