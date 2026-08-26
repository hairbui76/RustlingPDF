import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { LocalIcon } from "@app/components/shared/LocalIcon";
import { useNavigate, useSearchParams } from "react-router-dom";
import {
  Alert,
  Badge,
  Box,
  Card,
  Group,
  Progress,
  Stack,
  Switch,
  Text,
} from "@mantine/core";
import { useTranslation } from "react-i18next";
import { Button as DSButton } from "@app/ui/Button";
import { SegmentedControl } from "@app/ui/SegmentedControl";
import { LogoIcon } from "@app/components/shared/LogoIcon";
import { withBasePath } from "@app/constants/app";
import apiClient from "@app/services/apiClient";
import {
  loadJscanify,
  type JscanifyCornerPoints,
  type JscanifyScanner,
} from "@app/utils/loadJscanify";
import {
  FULL_IMAGE_CORNERS,
  applyScanFilter,
  clampNormalized,
  createScansPdf,
  downloadFile,
  fitScanDimensions,
  moveScan,
  rotateScanClockwise,
  type MobileScan,
  type NormalizedCorners,
  type ScanFilter,
} from "@app/pages/mobileScannerProcessing";

const API_BASE = (apiClient.defaults.baseURL ?? "").replace(/\/+$/, "");
const DETECTION_WIDTH = 240;
const CORNER_KEYS = [
  "topLeftCorner",
  "topRightCorner",
  "bottomRightCorner",
  "bottomLeftCorner",
] as const;
type CornerKey = (typeof CORNER_KEYS)[number];
type ScannerMode = "choice" | "camera" | "file";
type SessionState = "checking" | "transfer" | "local" | "invalid";

interface ScanDraft {
  sourceDataUrl: string;
  croppedDataUrl: string;
  dataUrl: string;
  corners: NormalizedCorners;
  filter: ScanFilter;
}

declare global {
  interface MediaTrackCapabilities {
    focusMode?: string[];
    exposureMode?: string[];
    torch?: boolean;
  }
  interface MediaTrackConstraintSet {
    focusMode?: ConstrainDOMString;
    exposureMode?: ConstrainDOMString;
    torch?: ConstrainBoolean;
  }
}

function cloneFullCorners(): NormalizedCorners {
  return {
    topLeftCorner: { ...FULL_IMAGE_CORNERS.topLeftCorner },
    topRightCorner: { ...FULL_IMAGE_CORNERS.topRightCorner },
    bottomLeftCorner: { ...FULL_IMAGE_CORNERS.bottomLeftCorner },
    bottomRightCorner: { ...FULL_IMAGE_CORNERS.bottomRightCorner },
  };
}

function scanId(): string {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
}

function loadImage(dataUrl: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("The image could not be decoded."));
    image.src = dataUrl;
  });
}

function readFile(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () =>
      reject(reader.error ?? new Error("File read failed."));
    reader.readAsDataURL(file);
  });
}

function normalizeCorners(
  corners: JscanifyCornerPoints,
  width: number,
  height: number,
): NormalizedCorners {
  const normalize = (point: { x: number; y: number }) => ({
    x: clampNormalized(point.x / width),
    y: clampNormalized(point.y / height),
  });
  return {
    topLeftCorner: normalize(corners.topLeftCorner),
    topRightCorner: normalize(corners.topRightCorner),
    bottomLeftCorner: normalize(corners.bottomLeftCorner),
    bottomRightCorner: normalize(corners.bottomRightCorner),
  };
}

function denormalizeCorners(
  corners: NormalizedCorners,
  width: number,
  height: number,
): JscanifyCornerPoints {
  const denormalize = (point: { x: number; y: number }) => ({
    x: point.x * width,
    y: point.y * height,
  });
  return {
    topLeftCorner: denormalize(corners.topLeftCorner),
    topRightCorner: denormalize(corners.topRightCorner),
    bottomLeftCorner: denormalize(corners.bottomLeftCorner),
    bottomRightCorner: denormalize(corners.bottomRightCorner),
  };
}

function extractedDimensions(corners: JscanifyCornerPoints): {
  width: number;
  height: number;
} {
  const top = Math.hypot(
    corners.topRightCorner.x - corners.topLeftCorner.x,
    corners.topRightCorner.y - corners.topLeftCorner.y,
  );
  const bottom = Math.hypot(
    corners.bottomRightCorner.x - corners.bottomLeftCorner.x,
    corners.bottomRightCorner.y - corners.bottomLeftCorner.y,
  );
  const left = Math.hypot(
    corners.bottomLeftCorner.x - corners.topLeftCorner.x,
    corners.bottomLeftCorner.y - corners.topLeftCorner.y,
  );
  const right = Math.hypot(
    corners.bottomRightCorner.x - corners.topRightCorner.x,
    corners.bottomRightCorner.y - corners.topRightCorner.y,
  );
  return {
    width: Math.max(1, Math.round((top + bottom) / 2)),
    height: Math.max(1, Math.round((left + right) / 2)),
  };
}

async function registerScannerPwa(): Promise<void> {
  if (!import.meta.env.PROD || !("serviceWorker" in navigator)) return;
  const registration = await navigator.serviceWorker.register(
    withBasePath("/mobile-scanner-sw.js"),
    { scope: withBasePath("/") },
  );
  await navigator.serviceWorker.ready;
  if (!navigator.serviceWorker.controller) {
    await new Promise<void>((resolve, reject) => {
      const timeout = window.setTimeout(
        () => reject(new Error("Service worker did not take control.")),
        10_000,
      );
      navigator.serviceWorker.addEventListener(
        "controllerchange",
        () => {
          window.clearTimeout(timeout);
          resolve();
        },
        { once: true },
      );
    });
  }
  const urls = [
    window.location.href,
    ...performance
      .getEntriesByType("resource")
      .map((entry) => (entry as PerformanceResourceTiming).name),
  ];
  const worker = navigator.serviceWorker.controller ?? registration.active;
  if (!worker) throw new Error("Service worker is not active.");
  await new Promise<void>((resolve, reject) => {
    const channel = new MessageChannel();
    const timeout = window.setTimeout(
      () => reject(new Error("Scanner resource cache timed out.")),
      30_000,
    );
    channel.port1.onmessage = (event) => {
      if (event.data?.type !== "CACHE_COMPLETE") return;
      window.clearTimeout(timeout);
      if (event.data.failed > 0) {
        reject(
          new Error(
            `${event.data.failed} scanner resources could not be cached.`,
          ),
        );
        return;
      }
      resolve();
    };
    worker.postMessage({ type: "CACHE_URLS", urls }, [channel.port2]);
  });
  document.documentElement.dataset.scannerOfflineReady = "true";
}

export default function MobileScannerPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const sessionId = searchParams.get("session");

  const [sessionState, setSessionState] = useState<SessionState>(
    sessionId ? "checking" : "local",
  );
  const [sessionMessage, setSessionMessage] = useState<string | null>(null);
  const [online, setOnline] = useState(navigator.onLine);
  const [mode, setMode] = useState<ScannerMode>("choice");
  const [capturedImages, setCapturedImages] = useState<MobileScan[]>([]);
  const [currentPreview, setCurrentPreview] = useState<ScanDraft | null>(null);
  const [adjustingCorners, setAdjustingCorners] = useState(false);
  const [activeCorner, setActiveCorner] = useState<CornerKey | null>(null);
  const [autoEnhance, setAutoEnhance] = useState(true);
  const [openCvReady, setOpenCvReady] = useState(false);
  const [cameraReady, setCameraReady] = useState(false);
  const [cameraError, setCameraError] = useState<string | null>(null);
  const [torchEnabled, setTorchEnabled] = useState(false);
  const [torchSupported, setTorchSupported] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [isUploading, setIsUploading] = useState(false);
  const [uploadProgress, setUploadProgress] = useState(0);
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [uploadSuccess, setUploadSuccess] = useState(false);
  const [exportedPdf, setExportedPdf] = useState<{
    file: File;
    pages: string[];
  } | null>(null);
  const [loadingStatus, setLoadingStatus] = useState("Initializing scanner…");

  const videoRef = useRef<HTMLVideoElement>(null);
  const captureCanvasRef = useRef<HTMLCanvasElement>(null);
  const highlightCanvasRef = useRef<HTMLCanvasElement>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const scannerRef = useRef<JscanifyScanner | null>(null);

  const pageDataUrls = useMemo(
    () => [
      ...capturedImages.map((scan) => scan.dataUrl),
      ...(currentPreview ? [currentPreview.dataUrl] : []),
    ],
    [capturedImages, currentPreview],
  );

  const currentExportedPdf =
    exportedPdf?.pages === pageDataUrls ? exportedPdf.file : null;

  useEffect(() => {
    const manifest = document.querySelector<HTMLLinkElement>(
      'link[rel="manifest"]',
    );
    const originalManifest = manifest?.getAttribute("href");
    manifest?.setAttribute(
      "href",
      withBasePath("/mobile-scanner-manifest.json"),
    );
    void registerScannerPwa().catch((error) => {
      console.warn(
        "Mobile scanner offline shell could not be registered:",
        error,
      );
    });
    return () => {
      if (manifest && originalManifest)
        manifest.setAttribute("href", originalManifest);
    };
  }, []);

  useEffect(() => {
    const handleOnline = () => setOnline(true);
    const handleOffline = () => setOnline(false);
    window.addEventListener("online", handleOnline);
    window.addEventListener("offline", handleOffline);
    return () => {
      window.removeEventListener("online", handleOnline);
      window.removeEventListener("offline", handleOffline);
    };
  }, []);

  useEffect(() => {
    if (!sessionId) {
      setSessionState("local");
      return;
    }
    const controller = new AbortController();
    setSessionState("checking");
    fetch(
      `${API_BASE}/api/v1/mobile-scanner/validate-session/${encodeURIComponent(sessionId)}`,
      { signal: controller.signal },
    )
      .then(async (response) => {
        if (!response.ok) {
          setSessionState("invalid");
          setSessionMessage(
            t(
              "mobileScanner.sessionExpired",
              "This desktop transfer session is missing or expired.",
            ),
          );
          return;
        }
        const body = await response.json();
        if (body.valid) {
          setSessionState("transfer");
          setSessionMessage(null);
        } else {
          setSessionState("invalid");
        }
      })
      .catch((error) => {
        if (error instanceof DOMException && error.name === "AbortError")
          return;
        setSessionState("local");
        setSessionMessage(
          t(
            "mobileScanner.desktopUnavailable",
            "The desktop is unavailable. You can keep scanning and export locally.",
          ),
        );
      });
    return () => controller.abort();
  }, [sessionId, t]);

  useEffect(() => {
    let cancelled = false;
    loadJscanify({
      onStatus: (status) => !cancelled && setLoadingStatus(status),
    })
      .then(() => {
        if (cancelled) return;
        scannerRef.current = new window.jscanify!();
        setOpenCvReady(true);
        setLoadingStatus("Scanner ready");
      })
      .catch((error) => {
        if (cancelled) return;
        setLoadingStatus("Automatic edge detection unavailable");
        console.warn("Scanner library failed to load:", error);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (mode !== "camera" || currentPreview) return;
    if (!navigator.mediaDevices?.getUserMedia) {
      setCameraError(
        t(
          "mobileScanner.httpsRequired",
          "Camera access requires HTTPS or localhost. Use photo upload instead.",
        ),
      );
      setMode("file");
      return;
    }

    let cancelled = false;
    setCameraReady(false);
    navigator.mediaDevices
      .getUserMedia({
        video: {
          facingMode: "environment",
          width: { ideal: 1920, max: 2560 },
          height: { ideal: 1440, max: 1920 },
        },
        audio: false,
      })
      .then(async (stream) => {
        if (cancelled) {
          stream.getTracks().forEach((track) => track.stop());
          return;
        }
        streamRef.current = stream;
        const video = videoRef.current;
        if (!video) return;
        video.srcObject = stream;
        await video.play();
        setCameraReady(true);
        setLoadingStatus("Camera ready");

        const track = stream.getVideoTracks()[0];
        try {
          const capabilities = track.getCapabilities();
          const advanced: MediaTrackConstraintSet[] = [];
          if (capabilities.focusMode?.includes("continuous")) {
            advanced.push({ focusMode: "continuous" });
          }
          if (capabilities.exposureMode?.includes("continuous")) {
            advanced.push({ exposureMode: "continuous" });
          }
          setTorchSupported(Boolean(capabilities.torch));
          if (advanced.length) await track.applyConstraints({ advanced });
        } catch {
          setTorchSupported(false);
        }
      })
      .catch(() => {
        setCameraError(
          t(
            "mobileScanner.cameraAccessDenied",
            "Camera access was denied. Use photo upload or enable camera permission.",
          ),
        );
        setMode("file");
      });

    return () => {
      cancelled = true;
      streamRef.current?.getTracks().forEach((track) => track.stop());
      streamRef.current = null;
      setCameraReady(false);
      setTorchEnabled(false);
    };
  }, [currentPreview, mode, t]);

  useEffect(() => {
    if (
      mode !== "camera" ||
      currentPreview ||
      !cameraReady ||
      !autoEnhance ||
      !openCvReady
    ) {
      const canvas = highlightCanvasRef.current;
      canvas?.getContext("2d")?.clearRect(0, 0, canvas.width, canvas.height);
      return;
    }
    const video = videoRef.current;
    const overlay = highlightCanvasRef.current;
    const scanner = scannerRef.current;
    const cv = window.cv;
    if (!video || !overlay || !scanner || !cv) return;

    const detection = document.createElement("canvas");
    const detectionContext = detection.getContext("2d", {
      willReadFrequently: true,
    });
    const overlayContext = overlay.getContext("2d");
    if (!detectionContext || !overlayContext) return;
    let frame = 0;
    let lastRun = 0;
    let cancelled = false;

    const detect = (time: number) => {
      if (cancelled) return;
      if (
        time - lastRun >= 400 &&
        video.videoWidth > 0 &&
        video.videoHeight > 0
      ) {
        lastRun = time;
        const scale = DETECTION_WIDTH / video.videoWidth;
        detection.width = DETECTION_WIDTH;
        detection.height = Math.max(1, Math.round(video.videoHeight * scale));
        overlay.width = video.videoWidth;
        overlay.height = video.videoHeight;
        detectionContext.drawImage(
          video,
          0,
          0,
          detection.width,
          detection.height,
        );
        overlayContext.clearRect(0, 0, overlay.width, overlay.height);

        let mat;
        let contour;
        try {
          mat = cv.imread(detection);
          contour = scanner.findPaperContour(mat);
          if (contour) {
            const corners = scanner.getCornerPoints(contour);
            const ratio = video.videoWidth / detection.width;
            const points = [
              corners.topLeftCorner,
              corners.topRightCorner,
              corners.bottomRightCorner,
              corners.bottomLeftCorner,
            ];
            overlayContext.strokeStyle = "#22c55e";
            overlayContext.lineWidth = 5;
            overlayContext.beginPath();
            points.forEach((point, index) => {
              const x = point.x * ratio;
              const y = point.y * ratio;
              if (index === 0) overlayContext.moveTo(x, y);
              else overlayContext.lineTo(x, y);
            });
            overlayContext.closePath();
            overlayContext.stroke();
          }
        } catch {
          // A missed detection is expected while the phone is moving.
        } finally {
          contour?.delete();
          mat?.delete();
        }
      }
      frame = requestAnimationFrame(detect);
    };
    frame = requestAnimationFrame(detect);
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  }, [autoEnhance, cameraReady, currentPreview, mode, openCvReady]);

  const extractWithCorners = useCallback(
    async (
      sourceDataUrl: string,
      normalizedCorners: NormalizedCorners,
    ): Promise<string> => {
      const image = await loadImage(sourceDataUrl);
      const canvas = document.createElement("canvas");
      canvas.width = image.naturalWidth || image.width;
      canvas.height = image.naturalHeight || image.height;
      const context = canvas.getContext("2d");
      if (!context) throw new Error("Image processing is unavailable.");
      context.drawImage(image, 0, 0, canvas.width, canvas.height);
      const scanner = scannerRef.current;
      if (!scanner) return canvas.toDataURL("image/jpeg", 0.94);
      const corners = denormalizeCorners(
        normalizedCorners,
        canvas.width,
        canvas.height,
      );
      const dimensions = extractedDimensions(corners);
      return scanner
        .extractPaper(canvas, dimensions.width, dimensions.height, corners)
        .toDataURL("image/jpeg", 0.94);
    },
    [],
  );

  const prepareDraft = useCallback(
    async (sourceDataUrl: string): Promise<ScanDraft> => {
      const image = await loadImage(sourceDataUrl);
      const canvas = document.createElement("canvas");
      const sourceDimensions = fitScanDimensions(
        image.naturalWidth || image.width,
        image.naturalHeight || image.height,
      );
      canvas.width = sourceDimensions.width;
      canvas.height = sourceDimensions.height;
      const context = canvas.getContext("2d", { willReadFrequently: true });
      if (!context) throw new Error("Image processing is unavailable.");
      context.drawImage(image, 0, 0, canvas.width, canvas.height);

      let corners = cloneFullCorners();
      const boundedSourceDataUrl = canvas.toDataURL("image/jpeg", 0.94);
      let croppedDataUrl = boundedSourceDataUrl;
      const scanner = scannerRef.current;
      const cv = window.cv;
      if (autoEnhance && scanner && cv) {
        const detection = document.createElement("canvas");
        const detectionScale = Math.min(1, DETECTION_WIDTH / canvas.width);
        detection.width = Math.max(
          1,
          Math.round(canvas.width * detectionScale),
        );
        detection.height = Math.max(
          1,
          Math.round(canvas.height * detectionScale),
        );
        detection
          .getContext("2d", { willReadFrequently: true })
          ?.drawImage(canvas, 0, 0, detection.width, detection.height);
        let mat;
        let contour;
        try {
          mat = cv.imread(detection);
          contour = scanner.findPaperContour(mat);
          if (contour) {
            const detected = scanner.getCornerPoints(contour);
            corners = normalizeCorners(
              detected,
              detection.width,
              detection.height,
            );
            const fullCorners = denormalizeCorners(
              corners,
              canvas.width,
              canvas.height,
            );
            const dimensions = extractedDimensions(fullCorners);
            croppedDataUrl = scanner
              .extractPaper(
                canvas,
                dimensions.width,
                dimensions.height,
                fullCorners,
              )
              .toDataURL("image/jpeg", 0.94);
          }
        } finally {
          contour?.delete();
          mat?.delete();
        }
      }
      return {
        sourceDataUrl: boundedSourceDataUrl,
        croppedDataUrl,
        dataUrl: croppedDataUrl,
        corners,
        filter: "color",
      };
    },
    [autoEnhance],
  );

  const captureImage = useCallback(async () => {
    const video = videoRef.current;
    const canvas = captureCanvasRef.current;
    if (!video || !canvas || !video.videoWidth || !video.videoHeight) return;
    setIsProcessing(true);
    setUploadError(null);
    try {
      canvas.width = video.videoWidth;
      canvas.height = video.videoHeight;
      const context = canvas.getContext("2d");
      if (!context) throw new Error("Camera capture is unavailable.");
      context.drawImage(video, 0, 0, canvas.width, canvas.height);
      setCurrentPreview(
        await prepareDraft(canvas.toDataURL("image/jpeg", 0.96)),
      );
    } catch (error) {
      setUploadError(
        error instanceof Error
          ? error.message
          : "The page could not be captured.",
      );
    } finally {
      setIsProcessing(false);
    }
  }, [prepareDraft]);

  const handleFileSelect = useCallback(
    async (event: React.ChangeEvent<HTMLInputElement>) => {
      const files = Array.from(event.target.files ?? []);
      event.target.value = "";
      if (!files.length) return;
      setIsProcessing(true);
      setUploadError(null);
      try {
        const drafts: ScanDraft[] = [];
        for (const file of files) {
          drafts.push(await prepareDraft(await readFile(file)));
        }
        if (drafts.length === 1 && !currentPreview) {
          setCurrentPreview(drafts[0]);
        } else {
          setCapturedImages((existing) => [
            ...existing,
            ...drafts.map((draft) => ({
              id: scanId(),
              dataUrl: draft.dataUrl,
            })),
          ]);
        }
      } catch (error) {
        setUploadError(
          error instanceof Error
            ? error.message
            : "The images could not be opened.",
        );
      } finally {
        setIsProcessing(false);
      }
    },
    [currentPreview, prepareDraft],
  );

  const applyCorners = useCallback(async () => {
    if (!currentPreview) return;
    setIsProcessing(true);
    try {
      const croppedDataUrl = await extractWithCorners(
        currentPreview.sourceDataUrl,
        currentPreview.corners,
      );
      const dataUrl = await applyScanFilter(
        croppedDataUrl,
        currentPreview.filter,
      );
      setCurrentPreview({ ...currentPreview, croppedDataUrl, dataUrl });
      setAdjustingCorners(false);
    } catch (error) {
      setUploadError(
        error instanceof Error
          ? error.message
          : "Perspective correction failed.",
      );
    } finally {
      setIsProcessing(false);
    }
  }, [currentPreview, extractWithCorners]);

  const selectFilter = useCallback(
    async (value: string) => {
      if (!currentPreview) return;
      const filter = value as ScanFilter;
      setIsProcessing(true);
      try {
        const dataUrl = await applyScanFilter(
          currentPreview.croppedDataUrl,
          filter,
        );
        setCurrentPreview({ ...currentPreview, filter, dataUrl });
      } catch (error) {
        setUploadError(
          error instanceof Error ? error.message : "Image cleanup failed.",
        );
      } finally {
        setIsProcessing(false);
      }
    },
    [currentPreview],
  );

  const rotatePreview = useCallback(async () => {
    if (!currentPreview) return;
    setIsProcessing(true);
    try {
      const rotated = await rotateScanClockwise(currentPreview.dataUrl);
      setCurrentPreview({
        sourceDataUrl: rotated,
        croppedDataUrl: rotated,
        dataUrl: rotated,
        corners: cloneFullCorners(),
        filter: "color",
      });
      setAdjustingCorners(false);
    } catch (error) {
      setUploadError(
        error instanceof Error ? error.message : "Image rotation failed.",
      );
    } finally {
      setIsProcessing(false);
    }
  }, [currentPreview]);

  const addToBatch = useCallback(() => {
    if (!currentPreview) return;
    setCapturedImages((existing) => [
      ...existing,
      { id: scanId(), dataUrl: currentPreview.dataUrl },
    ]);
    setCurrentPreview(null);
    setAdjustingCorners(false);
    setExportedPdf(null);
  }, [currentPreview]);

  const editScan = useCallback((index: number) => {
    setCapturedImages((existing) => {
      const selected = existing[index];
      if (!selected) return existing;
      setCurrentPreview({
        sourceDataUrl: selected.dataUrl,
        croppedDataUrl: selected.dataUrl,
        dataUrl: selected.dataUrl,
        corners: cloneFullCorners(),
        filter: "color",
      });
      return existing.filter((_, itemIndex) => itemIndex !== index);
    });
  }, []);

  const buildPdf = useCallback(async () => {
    const stamp = new Date().toISOString().replace(/[:.]/g, "-");
    const file = await createScansPdf(
      pageDataUrls,
      `rustlingpdf-scan-${stamp}.pdf`,
    );
    setExportedPdf({ file, pages: pageDataUrls });
    return file;
  }, [pageDataUrls]);

  const exportPdf = useCallback(async () => {
    setIsProcessing(true);
    setUploadError(null);
    try {
      const file = await buildPdf();
      downloadFile(file);
    } catch (error) {
      setUploadError(
        error instanceof Error ? error.message : "PDF export failed.",
      );
    } finally {
      setIsProcessing(false);
    }
  }, [buildPdf]);

  const openNextTool = useCallback(
    async (tool: "ocr" | "sign") => {
      setIsProcessing(true);
      try {
        const file = currentExportedPdf ?? (await buildPdf());
        if (!currentExportedPdf) downloadFile(file);
        navigate(`/${tool}`);
      } catch (error) {
        setUploadError(
          error instanceof Error ? error.message : "PDF handoff failed.",
        );
      } finally {
        setIsProcessing(false);
      }
    },
    [buildPdf, currentExportedPdf, navigate],
  );

  const uploadPdf = useCallback(async () => {
    if (!sessionId || sessionState !== "transfer") return;
    setIsUploading(true);
    setUploadProgress(10);
    setUploadError(null);
    try {
      const file = await buildPdf();
      setUploadProgress(45);
      const body = new FormData();
      body.append("files", file);
      const response = await fetch(
        `${API_BASE}/api/v1/mobile-scanner/upload/${encodeURIComponent(sessionId)}`,
        { method: "POST", body },
      );
      if (!response.ok) throw new Error("Desktop transfer failed.");
      setUploadProgress(100);
      setUploadSuccess(true);
    } catch (error) {
      setUploadError(
        error instanceof Error ? error.message : "Desktop transfer failed.",
      );
    } finally {
      setIsUploading(false);
    }
  }, [buildPdf, sessionId, sessionState]);

  const toggleTorch = useCallback(async () => {
    const track = streamRef.current?.getVideoTracks()[0];
    if (!track) return;
    try {
      await track.applyConstraints({
        advanced: [{ torch: !torchEnabled }],
      });
      setTorchEnabled((enabled) => !enabled);
    } catch {
      setTorchSupported(false);
    }
  }, [torchEnabled]);

  const updateCorner = useCallback(
    (event: React.PointerEvent<SVGSVGElement>) => {
      if (!activeCorner || !currentPreview) return;
      const bounds = event.currentTarget.getBoundingClientRect();
      const point = {
        x: clampNormalized((event.clientX - bounds.left) / bounds.width),
        y: clampNormalized((event.clientY - bounds.top) / bounds.height),
      };
      setCurrentPreview({
        ...currentPreview,
        corners: { ...currentPreview.corners, [activeCorner]: point },
      });
    },
    [activeCorner, currentPreview],
  );

  if (sessionState === "checking") {
    return (
      <Box p="xl">
        <Text ta="center">
          {t("mobileScanner.validating", "Connecting to desktop…")}
        </Text>
      </Box>
    );
  }

  if (sessionState === "invalid") {
    return (
      <Stack p="xl" maw={520} mx="auto">
        <Alert
          color="orange"
          title={t(
            "mobileScanner.sessionInvalid",
            "Transfer session unavailable",
          )}
        >
          {sessionMessage}
        </Alert>
        <DSButton
          variant="primary"
          onClick={() => {
            setSessionState("local");
            setSessionMessage(
              t(
                "mobileScanner.localMode",
                "Local mode: scans stay in this browser until you export them.",
              ),
            );
          }}
        >
          {t("mobileScanner.continueLocally", "Continue locally")}
        </DSButton>
      </Stack>
    );
  }

  if (uploadSuccess) {
    return (
      <Stack align="center" justify="center" mih="100dvh" p="xl">
        <LocalIcon
          icon="check-circle-rounded"
          style={{ fontSize: "4rem", color: "var(--mantine-color-green-6)" }}
        />
        <Text size="xl" fw={700}>
          {t("mobileScanner.uploadSuccess", "PDF transferred")}
        </Text>
        <Text ta="center" c="dimmed">
          {t(
            "mobileScanner.uploadSuccessMessage",
            "The ordered multi-page PDF is ready on the desktop for OCR, signing, or export.",
          )}
        </Text>
      </Stack>
    );
  }

  return (
    <Box
      style={{
        minHeight: "100dvh",
        background: "var(--c-bg)",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <Box
        p="md"
        style={{
          background: "var(--c-bg-raised)",
          borderBottom: "1px solid var(--c-border-subtle)",
        }}
      >
        <Group justify="space-between">
          <Group gap="sm">
            <LogoIcon
              alt={t("home.mobile.brandAlt", "RustlingPDF logo")}
              style={{ height: 32, width: "auto" }}
            />
          </Group>
          <Badge
            color={sessionState === "transfer" && online ? "green" : "gray"}
          >
            {sessionState === "transfer" && online
              ? t("mobileScanner.desktopConnected", "Desktop connected")
              : t("mobileScanner.localOnly", "Local only")}
          </Badge>
        </Group>
      </Box>

      {(sessionMessage || !online) && (
        <Box p="sm">
          <Alert color="blue" icon={<LocalIcon icon="info-rounded" />}>
            {sessionMessage ??
              t(
                "mobileScanner.offlineReady",
                "Offline mode: capture, correction, cleanup, ordering, and PDF export remain local.",
              )}
          </Alert>
        </Box>
      )}
      {uploadError && (
        <Box p="sm">
          <Alert
            color="red"
            icon={<LocalIcon icon="error-rounded" />}
            withCloseButton
            onClose={() => setUploadError(null)}
          >
            {uploadError}
          </Alert>
        </Box>
      )}
      {isUploading && (
        <Box p="sm">
          <Text size="sm" mb="xs">
            {t("mobileScanner.uploading", "Building and transferring PDF…")}
          </Text>
          <Progress value={uploadProgress} animated />
        </Box>
      )}
      {cameraError && (
        <Box p="sm">
          <Alert color="orange" icon={<LocalIcon icon="info-rounded" />}>
            {cameraError}
          </Alert>
        </Box>
      )}

      {mode === "choice" && !currentPreview && (
        <Stack gap="lg" p="xl" maw={520} mx="auto" w="100%">
          <Stack gap="xs" align="center">
            <Text size="xl" fw={700} ta="center">
              {t("mobileScanner.title", "Scan on this device")}
            </Text>
            <Text size="sm" c="dimmed" ta="center">
              {t(
                "mobileScanner.localPrivacy",
                "Capture and edit locally. Nothing leaves this browser unless you choose desktop transfer.",
              )}
            </Text>
          </Stack>
          <Card
            withBorder
            p="xl"
            onClick={() => setMode("camera")}
            style={{ cursor: "pointer" }}
          >
            <Stack align="center">
              <LocalIcon
                icon="photo-camera-rounded"
                style={{
                  fontSize: "3rem",
                  color: "var(--mantine-color-blue-6)",
                }}
              />
              <Text fw={600}>{t("mobileScanner.camera", "Camera")}</Text>
              <Text size="sm" c="dimmed" ta="center">
                {t(
                  "mobileScanner.cameraDescription",
                  "Live edge detection with perspective correction",
                )}
              </Text>
            </Stack>
          </Card>
          <Card
            withBorder
            p="xl"
            onClick={() => setMode("file")}
            style={{ cursor: "pointer" }}
          >
            <Stack align="center">
              <LocalIcon
                icon="upload-rounded"
                style={{
                  fontSize: "3rem",
                  color: "var(--mantine-color-green-6)",
                }}
              />
              <Text fw={600}>{t("mobileScanner.fileUpload", "Photos")}</Text>
              <Text size="sm" c="dimmed" ta="center">
                {t(
                  "mobileScanner.fileDescription",
                  "Select one or many existing document photos",
                )}
              </Text>
            </Stack>
          </Card>
        </Stack>
      )}

      {mode === "camera" && !currentPreview && (
        <Box
          style={{
            display: "flex",
            flexDirection: "column",
            minHeight: "70dvh",
          }}
        >
          <Box
            style={{
              position: "relative",
              flex: 1,
              minHeight: 420,
              background: "#000",
            }}
          >
            <video
              ref={videoRef}
              autoPlay
              muted
              playsInline
              style={{ width: "100%", height: "100%", objectFit: "contain" }}
            />
            <canvas ref={captureCanvasRef} style={{ display: "none" }} />
            <canvas
              ref={highlightCanvasRef}
              style={{
                position: "absolute",
                inset: 0,
                width: "100%",
                height: "100%",
                pointerEvents: "none",
              }}
            />
            <DSButton
              size="sm"
              variant="secondary"
              onClick={() => setMode("choice")}
              style={{ position: "absolute", top: 12, left: 12 }}
            >
              ← {t("mobileScanner.back", "Back")}
            </DSButton>
          </Box>
          <Stack p="md" gap="sm">
            <Group justify="space-between">
              <Switch
                label={t("mobileScanner.edgeDetection", "Edge detection")}
                checked={autoEnhance}
                onChange={(event) =>
                  setAutoEnhance(event.currentTarget.checked)
                }
                disabled={!openCvReady}
              />
              {torchSupported && (
                <Switch
                  label={t("mobileScanner.flashlight", "Flash")}
                  checked={torchEnabled}
                  onChange={() => void toggleTorch()}
                />
              )}
            </Group>
            <DSButton
              fullWidth
              size="md"
              variant="primary"
              onClick={() => void captureImage()}
              loading={isProcessing}
              disabled={!cameraReady}
            >
              {t("mobileScanner.capture", "Capture page")}
            </DSButton>
            {!openCvReady && (
              <Text size="xs" c="dimmed" ta="center">
                {loadingStatus}
              </Text>
            )}
          </Stack>
        </Box>
      )}

      {mode === "file" && !currentPreview && (
        <Stack gap="lg" p="xl" maw={520} mx="auto" w="100%">
          <DSButton
            variant="tertiary"
            size="sm"
            onClick={() => setMode("choice")}
          >
            ← {t("mobileScanner.back", "Back")}
          </DSButton>
          <Card withBorder p="xl">
            <Stack align="center">
              <LocalIcon
                icon="add-photo-alternate-rounded"
                style={{
                  fontSize: "4rem",
                  color: "var(--mantine-color-gray-5)",
                }}
              />
              <Text fw={600} ta="center">
                {t("mobileScanner.selectFilesPrompt", "Select document photos")}
              </Text>
              <input
                ref={fileInputRef}
                type="file"
                accept="image/*"
                multiple
                hidden
                onChange={(event) => void handleFileSelect(event)}
              />
              <DSButton
                fullWidth
                size="lg"
                loading={isProcessing}
                onClick={() => fileInputRef.current?.click()}
              >
                {t("mobileScanner.selectImages", "Select images")}
              </DSButton>
            </Stack>
          </Card>
        </Stack>
      )}

      {currentPreview && (
        <Stack gap="sm" p="sm" maw={760} mx="auto" w="100%">
          <Box
            style={{
              minHeight: 360,
              maxHeight: "62dvh",
              display: "flex",
              justifyContent: "center",
              alignItems: "center",
              background: "#000",
              overflow: "hidden",
            }}
          >
            {adjustingCorners ? (
              <Box
                style={{
                  position: "relative",
                  display: "inline-block",
                  maxWidth: "100%",
                  maxHeight: "62dvh",
                }}
              >
                <img
                  src={currentPreview.sourceDataUrl}
                  alt={t(
                    "mobileScanner.cornerPreview",
                    "Adjust document corners",
                  )}
                  style={{
                    display: "block",
                    maxWidth: "100%",
                    maxHeight: "62dvh",
                  }}
                />
                <svg
                  viewBox="0 0 100 100"
                  preserveAspectRatio="none"
                  onPointerMove={updateCorner}
                  onPointerUp={() => setActiveCorner(null)}
                  onPointerCancel={() => setActiveCorner(null)}
                  style={{
                    position: "absolute",
                    inset: 0,
                    width: "100%",
                    height: "100%",
                    touchAction: "none",
                  }}
                >
                  <polygon
                    points={CORNER_KEYS.map(
                      (key) =>
                        `${currentPreview.corners[key].x * 100},${currentPreview.corners[key].y * 100}`,
                    ).join(" ")}
                    fill="rgba(34,197,94,0.16)"
                    stroke="#22c55e"
                    strokeWidth="1"
                    vectorEffect="non-scaling-stroke"
                  />
                  {CORNER_KEYS.map((key) => (
                    <circle
                      key={key}
                      cx={currentPreview.corners[key].x * 100}
                      cy={currentPreview.corners[key].y * 100}
                      r="3"
                      fill="#fff"
                      stroke="#16a34a"
                      strokeWidth="1"
                      vectorEffect="non-scaling-stroke"
                      onPointerDown={(event) => {
                        event.currentTarget.setPointerCapture(event.pointerId);
                        setActiveCorner(key);
                      }}
                    />
                  ))}
                </svg>
              </Box>
            ) : (
              <img
                src={currentPreview.dataUrl}
                alt={t("mobileScanner.preview", "Scanned page preview")}
                style={{
                  maxWidth: "100%",
                  maxHeight: "62dvh",
                  objectFit: "contain",
                }}
              />
            )}
          </Box>

          {adjustingCorners ? (
            <Group grow>
              <DSButton
                variant="secondary"
                onClick={() => setAdjustingCorners(false)}
              >
                {t("mobileScanner.cancel", "Cancel")}
              </DSButton>
              <DSButton
                variant="primary"
                loading={isProcessing}
                onClick={() => void applyCorners()}
              >
                {t("mobileScanner.applyPerspective", "Apply perspective")}
              </DSButton>
            </Group>
          ) : (
            <>
              <Group grow>
                <DSButton
                  variant="secondary"
                  onClick={() => setAdjustingCorners(true)}
                >
                  {t("mobileScanner.adjustCorners", "Adjust corners")}
                </DSButton>
                <DSButton
                  variant="secondary"
                  onClick={() => void rotatePreview()}
                >
                  {t("mobileScanner.rotate", "Rotate")}
                </DSButton>
              </Group>
              <SegmentedControl
                fullWidth
                value={currentPreview.filter}
                onChange={(value) => void selectFilter(value)}
                options={[
                  {
                    value: "color",
                    label: t("mobileScanner.filterColor", "Color"),
                  },
                  {
                    value: "clean",
                    label: t("mobileScanner.filterClean", "Clean"),
                  },
                  {
                    value: "grayscale",
                    label: t("mobileScanner.filterGray", "Gray"),
                  },
                  {
                    value: "blackWhite",
                    label: t("mobileScanner.filterBw", "B&W"),
                  },
                ]}
              />
              <Group grow>
                <DSButton
                  variant="secondary"
                  onClick={() => {
                    setCurrentPreview(null);
                    setAdjustingCorners(false);
                  }}
                >
                  {t("mobileScanner.retake", "Discard")}
                </DSButton>
                <DSButton variant="primary" onClick={addToBatch}>
                  {t("mobileScanner.addToBatch", "Add page")}
                </DSButton>
              </Group>
            </>
          )}
        </Stack>
      )}

      {capturedImages.length > 0 && (
        <Box p="sm" style={{ borderTop: "1px solid var(--c-border-subtle)" }}>
          <Group justify="space-between" mb="sm">
            <Text fw={600}>
              {t("mobileScanner.pages", "Pages")} ({capturedImages.length})
            </Text>
            <DSButton
              size="sm"
              variant="secondary"
              accent="danger"
              onClick={() => {
                setCapturedImages([]);
                setExportedPdf(null);
              }}
            >
              {t("mobileScanner.clearBatch", "Clear")}
            </DSButton>
          </Group>
          <Box
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(auto-fill, minmax(120px, 1fr))",
              gap: 10,
            }}
          >
            {capturedImages.map((scan, index) => (
              <Card key={scan.id} withBorder p="xs">
                <Text size="xs" fw={700} mb={4}>
                  {t("mobileScanner.pageNumber", "Page {{number}}", {
                    number: index + 1,
                  })}
                </Text>
                <img
                  src={scan.dataUrl}
                  alt={`Page ${index + 1}`}
                  style={{ width: "100%", height: 100, objectFit: "cover" }}
                />
                <Group gap={4} mt="xs" grow>
                  <DSButton
                    size="sm"
                    variant="tertiary"
                    disabled={index === 0}
                    aria-label={`Move page ${index + 1} left`}
                    onClick={() =>
                      setCapturedImages((pages) => moveScan(pages, index, -1))
                    }
                  >
                    ←
                  </DSButton>
                  <DSButton
                    size="sm"
                    variant="tertiary"
                    disabled={index === capturedImages.length - 1}
                    aria-label={`Move page ${index + 1} right`}
                    onClick={() =>
                      setCapturedImages((pages) => moveScan(pages, index, 1))
                    }
                  >
                    →
                  </DSButton>
                </Group>
                <Group gap={4} mt={4} grow>
                  <DSButton
                    size="sm"
                    variant="secondary"
                    onClick={() => editScan(index)}
                  >
                    {t("mobileScanner.edit", "Edit")}
                  </DSButton>
                  <DSButton
                    size="sm"
                    variant="secondary"
                    accent="danger"
                    onClick={() =>
                      setCapturedImages((pages) =>
                        pages.filter((_, pageIndex) => pageIndex !== index),
                      )
                    }
                  >
                    {t("mobileScanner.remove", "Remove")}
                  </DSButton>
                </Group>
              </Card>
            ))}
          </Box>
        </Box>
      )}

      {pageDataUrls.length > 0 && (
        <Stack
          p="sm"
          gap="sm"
          style={{ borderTop: "1px solid var(--c-border-subtle)" }}
        >
          <Text size="sm" c="dimmed">
            {t(
              "mobileScanner.pdfOrder",
              "PDF pages follow the order above. The current preview, if any, is last.",
            )}
          </Text>
          <Group grow>
            <DSButton
              variant="primary"
              loading={isProcessing}
              onClick={() => void exportPdf()}
            >
              {t("mobileScanner.exportPdf", "Export PDF")}
            </DSButton>
            {sessionState === "transfer" && (
              <DSButton
                variant="primary"
                loading={isUploading}
                disabled={!online}
                onClick={() => void uploadPdf()}
              >
                {t("mobileScanner.transferPdf", "Send PDF to desktop")}
              </DSButton>
            )}
          </Group>
          {currentExportedPdf && (
            <Alert
              color="green"
              title={t("mobileScanner.readyForNextStep", "PDF ready")}
            >
              <Stack gap="xs">
                <Text size="sm">
                  {t(
                    "mobileScanner.handoffHint",
                    "The PDF was downloaded locally. Open it in OCR or Sign for the next step.",
                  )}
                </Text>
                <Group grow>
                  <DSButton
                    variant="secondary"
                    onClick={() => void openNextTool("ocr")}
                  >
                    {t("home.ocr.title", "Open OCR")}
                  </DSButton>
                  <DSButton
                    variant="secondary"
                    onClick={() => void openNextTool("sign")}
                  >
                    {t("home.sign.title", "Open Sign")}
                  </DSButton>
                </Group>
              </Stack>
            </Alert>
          )}
        </Stack>
      )}
    </Box>
  );
}
