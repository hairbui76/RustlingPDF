import { useEffect, useCallback, useState } from "react";
import {
  START_TOUR_EVENT,
  type TourType,
  type StartTourPayload,
} from "@app/constants/events";

export function useTourRequest(): {
  tourRequested: boolean;
  requestedTourType: TourType;
  clearTourRequest: () => void;
} {
  const [tourRequested, setTourRequested] = useState(false);
  const [requestedTourType, setRequestedTourType] =
    useState<TourType>("whatsnew");

  useEffect(() => {
    if (typeof window === "undefined") return;

    const handleTourRequest = (event: Event) => {
      const { detail } = event as CustomEvent<StartTourPayload>;
      setRequestedTourType(detail?.tourType ?? "whatsnew");
      setTourRequested(true);
    };

    window.addEventListener(START_TOUR_EVENT, handleTourRequest);
    return () =>
      window.removeEventListener(START_TOUR_EVENT, handleTourRequest);
  }, []);

  const clearTourRequest = useCallback(() => {
    setTourRequested(false);
  }, []);

  return { tourRequested, requestedTourType, clearTourRequest };
}
