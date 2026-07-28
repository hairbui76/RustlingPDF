import type { SavedSignature } from "@app/types/signature";

/**
 * Signatures are per-browser state: the server keeps no user data, so the
 * only store is localStorage. The two-member type survives because the
 * sign-tool hook narrows on it; "backend" can never be returned.
 */
export type StorageType = "backend" | "localStorage";

/**
 * Service to handle signature storage in the browser's localStorage.
 */
class SignatureStorageService {
  /**
   * Get current storage type (always localStorage — there is no server store).
   */
  async getStorageType(): Promise<StorageType> {
    return "localStorage";
  }

  /**
   * Load all signatures
   */
  async loadSignatures(): Promise<SavedSignature[]> {
    return this._loadFromLocalStorage();
  }

  /**
   * Save a signature
   */
  async saveSignature(signature: SavedSignature): Promise<void> {
    // Force scope to localStorage for browser storage
    signature.scope = "localStorage";
    this._saveToLocalStorage(signature);
  }

  /**
   * Delete a signature
   */
  async deleteSignature(id: string): Promise<void> {
    this._deleteFromLocalStorage(id);
  }

  /**
   * Update signature label
   */
  async updateSignatureLabel(id: string, label: string): Promise<void> {
    this._updateLabelInLocalStorage(id, label);
  }

  // LocalStorage methods
  private readonly STORAGE_KEY = "stirling:saved-signatures:v1";

  private _loadFromLocalStorage(): SavedSignature[] {
    try {
      const raw = localStorage.getItem(this.STORAGE_KEY);
      if (!raw) return [];
      const signatures = JSON.parse(raw);
      // Ensure all localStorage signatures have the correct scope
      return signatures.map((sig: SavedSignature) => ({
        ...sig,
        scope: "localStorage" as const,
      }));
    } catch {
      return [];
    }
  }

  private _saveToLocalStorage(signature: SavedSignature): void {
    const signatures = this._loadFromLocalStorage();
    const index = signatures.findIndex((s) => s.id === signature.id);

    if (index >= 0) {
      signatures[index] = signature;
    } else {
      signatures.unshift(signature);
    }

    localStorage.setItem(this.STORAGE_KEY, JSON.stringify(signatures));
  }

  private _deleteFromLocalStorage(id: string): void {
    const signatures = this._loadFromLocalStorage();
    const filtered = signatures.filter((s) => s.id !== id);
    localStorage.setItem(this.STORAGE_KEY, JSON.stringify(filtered));
  }

  private _updateLabelInLocalStorage(id: string, label: string): void {
    const signatures = this._loadFromLocalStorage();
    const signature = signatures.find((s) => s.id === id);
    if (signature) {
      signature.label = label;
      signature.updatedAt = Date.now();
      localStorage.setItem(this.STORAGE_KEY, JSON.stringify(signatures));
    }
  }
}

export const signatureStorageService = new SignatureStorageService();
