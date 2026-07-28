import {
  AppProviders as CoreAppProviders,
  AppProvidersProps,
} from "@core/components/AppProviders";
import { ServerExperienceProvider } from "@app/contexts/ServerExperienceContext";
import { ChatProvider } from "@app/components/chat/ChatContext";

export function AppProviders({
  children,
  appConfigRetryOptions,
  appConfigProviderProps,
}: AppProvidersProps) {
  return (
    <CoreAppProviders
      appConfigRetryOptions={appConfigRetryOptions}
      appConfigProviderProps={appConfigProviderProps}
    >
      <ServerExperienceProvider>
        <ChatProvider>{children}</ChatProvider>
      </ServerExperienceProvider>
    </CoreAppProviders>
  );
}
