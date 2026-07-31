import { Suspense, lazy, useEffect, useState } from "react";

// Defer settings UI until the user first opens it, then keep it mounted so the
// close animation runs.
const AppConfigModal = lazy(
  () => import("@app/components/shared/AppConfigModal"),
);

interface AppConfigModalLazyProps {
  opened: boolean;
  onClose: () => void;
}

export default function AppConfigModalLazy({
  opened,
  onClose,
}: AppConfigModalLazyProps) {
  const [shouldMount, setShouldMount] = useState(false);

  useEffect(() => {
    if (opened) setShouldMount(true);
  }, [opened]);

  return (
    <Suspense fallback={null}>
      {shouldMount && <AppConfigModal opened={opened} onClose={onClose} />}
    </Suspense>
  );
}
