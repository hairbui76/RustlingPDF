import { Stack, TextInput, Text, Group } from "@mantine/core";
import { Button } from "@app/ui/Button";
import { useTranslation } from "react-i18next";
import { useEffect } from "react";
import FileUploadButton from "@app/components/shared/FileUploadButton";

export type CertificateType = "USER_CERT" | "SERVER" | "UPLOAD";
export type UploadFormat = "PKCS12" | "PFX" | "PEM" | "JKS";

interface CertificateSelectorProps {
  certType: CertificateType;
  onCertTypeChange: (certType: CertificateType) => void;
  uploadFormat: UploadFormat;
  onUploadFormatChange: (format: UploadFormat) => void;
  p12File: File | null;
  onP12FileChange: (file: File | null) => void;
  privateKeyFile: File | null;
  onPrivateKeyFileChange: (file: File | null) => void;
  certFile: File | null;
  onCertFileChange: (file: File | null) => void;
  jksFile: File | null;
  onJksFileChange: (file: File | null) => void;
  password: string;
  onPasswordChange: (password: string) => void;
  disabled?: boolean;
}

export const CertificateSelector: React.FC<CertificateSelectorProps> = ({
  certType,
  onCertTypeChange,
  uploadFormat,
  onUploadFormatChange,
  p12File,
  onP12FileChange,
  privateKeyFile,
  onPrivateKeyFileChange,
  certFile,
  onCertFileChange,
  jksFile,
  onJksFileChange,
  password,
  onPasswordChange,
  disabled = false,
}) => {
  const { t } = useTranslation();

  // Account-managed certificates are not part of the local, stateless build.
  useEffect(() => {
    if (certType !== "UPLOAD") {
      onCertTypeChange("UPLOAD");
    }
  }, [certType, onCertTypeChange]);

  const handleFormatChange = (fmt: UploadFormat) => {
    onUploadFormatChange(fmt);
    onP12FileChange(null);
    onPrivateKeyFileChange(null);
    onCertFileChange(null);
    onJksFileChange(null);
    onPasswordChange("");
  };

  const showPassword =
    ((uploadFormat === "PKCS12" || uploadFormat === "PFX") && p12File) ||
    (uploadFormat === "PEM" && privateKeyFile && certFile) ||
    (uploadFormat === "JKS" && jksFile);

  return (
    <Stack gap="md">
      {/* Upload section */}
      {certType === "UPLOAD" && (
        <Stack gap="sm">
          {/* Format picker */}
          <Group gap="xs">
            {(["PKCS12", "PFX", "PEM", "JKS"] as UploadFormat[]).map((fmt) => (
              <Button
                key={fmt}
                size="sm"
                variant={uploadFormat === fmt ? "primary" : "secondary"}
                onClick={() => handleFormatChange(fmt)}
                disabled={disabled}
              >
                {fmt}
              </Button>
            ))}
          </Group>

          {/* PKCS12 / PFX */}
          {(uploadFormat === "PKCS12" || uploadFormat === "PFX") && (
            <FileUploadButton
              file={p12File ?? undefined}
              onChange={(file) => onP12FileChange(file || null)}
              accept=".p12,.pfx"
              disabled={disabled}
              placeholder={
                uploadFormat === "PFX"
                  ? t("certSign.choosePfxFile", "Choose PFX File")
                  : t("certSign.chooseP12File", "Choose PKCS12 File")
              }
            />
          )}

          {/* PEM — private key and certificate are two separate files */}
          {uploadFormat === "PEM" && (
            <Stack gap="sm">
              <Stack gap={4}>
                <Text size="xs" fw={600}>
                  {t(
                    "certSign.pemPrivateKeyLabel",
                    "Private key (.pem / .key)",
                  )}
                </Text>
                <FileUploadButton
                  file={privateKeyFile ?? undefined}
                  onChange={(file) => onPrivateKeyFileChange(file || null)}
                  accept=".pem,.der,.key"
                  disabled={disabled}
                  placeholder={t(
                    "certSign.choosePrivateKey",
                    "Choose Private Key File",
                  )}
                />
              </Stack>
              <Stack gap={4}>
                <Text size="xs" fw={600}>
                  {t(
                    "certSign.pemCertificateLabel",
                    "Certificate (.pem / .crt)",
                  )}
                </Text>
                <FileUploadButton
                  file={certFile ?? undefined}
                  onChange={(file) => onCertFileChange(file || null)}
                  accept=".pem,.der,.crt,.cer"
                  disabled={disabled}
                  placeholder={t(
                    "certSign.chooseCertificate",
                    "Choose Certificate File",
                  )}
                />
              </Stack>
            </Stack>
          )}

          {/* JKS */}
          {uploadFormat === "JKS" && (
            <FileUploadButton
              file={jksFile ?? undefined}
              onChange={(file) => onJksFileChange(file || null)}
              accept=".jks,.keystore"
              disabled={disabled}
              placeholder={t("certSign.chooseJksFile", "Choose JKS File")}
            />
          )}

          {/* Password */}
          {showPassword && (
            <TextInput
              label={t(
                "certSign.collab.signRequest.password",
                "Certificate Password",
              )}
              type="password"
              placeholder={t(
                "certSign.passwordOptional",
                "Leave empty if no password",
              )}
              value={password}
              onChange={(e) => onPasswordChange(e.target.value)}
              disabled={disabled}
              size="sm"
            />
          )}
        </Stack>
      )}
    </Stack>
  );
};
