import { useState, useEffect } from "react";
import apiClient from "@app/services/apiClient";

export interface FooterInfo {
  termsAndConditions?: string;
  privacyPolicy?: string;
  accessibilityStatement?: string;
  cookiePolicy?: string;
  impressum?: string;
}

/**
 * Hook to fetch public footer configuration data.
 * This endpoint is public application configuration.
 */
export function useFooterInfo() {
  const [footerInfo, setFooterInfo] = useState<FooterInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    const fetchFooterInfo = async () => {
      try {
        setLoading(true);
        const response = await apiClient.get<FooterInfo>(
          "/api/v1/ui-data/footer-info",
          {
            suppressErrorToast: true,
          } as any,
        );
        setFooterInfo(response.data);
        setError(null);
      } catch (err) {
        console.error("[useFooterInfo] Failed to fetch footer info:", err);
        setError(err as Error);
        // No links on error; every field is optional and renders nothing.
        setFooterInfo({});
      } finally {
        setLoading(false);
      }
    };

    fetchFooterInfo();
  }, []);

  return { footerInfo, loading, error };
}
