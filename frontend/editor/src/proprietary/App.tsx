import { Suspense } from "react";
import { Routes, Route } from "react-router-dom";
import { AppProviders } from "@app/components/AppProviders";
import { AppLayout } from "@app/components/AppLayout";
import { LoadingFallback } from "@app/components/shared/LoadingFallback";
import { PreferencesProvider } from "@app/contexts/PreferencesContext";
import { ThemeProvider } from "@app/components/shared/ThemeProvider";
import Landing from "@app/routes/Landing";
import MobileScannerPage from "@app/pages/MobileScannerPage";
import Onboarding from "@app/components/onboarding/Onboarding";
import WatchedFoldersRegistration from "@app/components/watchedFolders/WatchedFoldersRegistration";
import { WATCHED_FOLDERS_ENABLED } from "@app/constants/featureFlags";

// Import global styles
import "@app/styles/tailwind.css";
import "@app/styles/cookieconsent.css";
import "@app/styles/index.css";

// Import file ID debugging helpers (development only)
import "@app/utils/fileIdSafety";

// Minimal providers for public pages (mobile scanner) - no API calls
function PublicRouteProviders({ children }: { children: React.ReactNode }) {
  return (
    <PreferencesProvider>
      <ThemeProvider>{children}</ThemeProvider>
    </PreferencesProvider>
  );
}

export default function App() {
  return (
    <Suspense fallback={<LoadingFallback />}>
      <Routes>
        {/* Mobile scanner route - no backend needed, pure P2P WebRTC */}
        <Route
          path="/mobile-scanner"
          element={
            <PublicRouteProviders>
              <MobileScannerPage />
            </PublicRouteProviders>
          }
        />

        {/* All other routes need AppProviders for backend integration */}
        <Route
          path="*"
          element={
            <AppProviders>
              <AppLayout>
                <Routes>
                  <Route path="/*" element={<Landing />} />
                </Routes>
                <Onboarding />
                {WATCHED_FOLDERS_ENABLED && <WatchedFoldersRegistration />}
              </AppLayout>
            </AppProviders>
          }
        />
      </Routes>
    </Suspense>
  );
}
