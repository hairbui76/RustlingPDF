import { Suspense } from "react";
import { Routes, Route } from "react-router-dom";
import { AppProviders } from "@app/components/AppProviders";
import { AppLayout } from "@app/components/AppLayout";
import { LoadingFallback } from "@app/components/shared/LoadingFallback";
import Landing from "@app/routes/Landing";
import Onboarding from "@app/components/onboarding/Onboarding";

// Import global styles
import "@app/styles/tailwind.css";
import "@app/styles/cookieconsent.css";
import "@app/styles/index.css";

// Import file ID debugging helpers (development only)
import "@app/utils/fileIdSafety";

export default function App() {
  return (
    <Suspense fallback={<LoadingFallback />}>
      <Routes>
        {/* All routes need AppProviders for backend integration */}
        <Route
          path="*"
          element={
            <AppProviders>
              <AppLayout>
                <Routes>
                  <Route path="/*" element={<Landing />} />
                </Routes>
                <Onboarding />
              </AppLayout>
            </AppProviders>
          }
        />
      </Routes>
    </Suspense>
  );
}
