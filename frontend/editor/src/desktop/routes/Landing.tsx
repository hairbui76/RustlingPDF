import HomePage from "@app/pages/HomePage";

/**
 * Desktop override of Landing.
 * Desktop builds have no authentication and no /login route; always render
 * the main app directly. First-run setup is handled by the
 * DesktopOnboardingModal rendered on top by AppProviders.
 */
export default function Landing() {
  return <HomePage />;
}
