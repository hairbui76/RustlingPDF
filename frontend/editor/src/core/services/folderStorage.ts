/** IndexedDB persistence for the local folder hierarchy. */

import { FolderId, FolderRecord } from "@app/types/folder";
import {
  indexedDBManager,
  DATABASE_CONFIGS,
} from "@app/services/indexedDBManager";

class FolderStorageService {
  private readonly dbConfig = DATABASE_CONFIGS.FILES;
  private readonly storeName = "folders";

  private async getDatabase(): Promise<IDBDatabase> {
    return indexedDBManager.openDatabase(this.dbConfig);
  }

  /** Atomically replace the complete local folder set. */
  async replaceAll(folders: FolderRecord[]): Promise<void> {
    const db = await this.getDatabase();
    await new Promise<void>((resolve, reject) => {
      const transaction = db.transaction([this.storeName], "readwrite");
      const store = transaction.objectStore(this.storeName);
      transaction.oncomplete = () => resolve();
      transaction.onerror = () =>
        reject(transaction.error ?? new Error("folder cache replace failed"));
      transaction.onabort = () =>
        reject(transaction.error ?? new Error("folder cache replace aborted"));
      store.clear();
      for (const folder of folders) {
        store.put(folder);
      }
    });
  }

  /** Insert or overwrite a single folder in the cache. */
  async upsertFolder(folder: FolderRecord): Promise<void> {
    const db = await this.getDatabase();
    await new Promise<void>((resolve, reject) => {
      const transaction = db.transaction([this.storeName], "readwrite");
      const store = transaction.objectStore(this.storeName);
      const req = store.put(folder);
      req.onerror = () => reject(req.error);
      req.onsuccess = () => resolve();
    });
  }

  /** Remove a set of folders from local storage. */
  async removeFolders(ids: FolderId[]): Promise<void> {
    if (ids.length === 0) return;
    const db = await this.getDatabase();
    await new Promise<void>((resolve, reject) => {
      const transaction = db.transaction([this.storeName], "readwrite");
      const store = transaction.objectStore(this.storeName);
      transaction.oncomplete = () => resolve();
      transaction.onerror = () =>
        reject(transaction.error ?? new Error("folder cache delete failed"));
      transaction.onabort = () =>
        reject(transaction.error ?? new Error("folder cache delete aborted"));
      for (const id of ids) store.delete(id);
    });
  }

  async getAllFolders(): Promise<FolderRecord[]> {
    const db = await this.getDatabase();
    return new Promise((resolve, reject) => {
      const transaction = db.transaction([this.storeName], "readonly");
      const store = transaction.objectStore(this.storeName);
      const request = store.getAll();
      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        const records = (request.result as FolderRecord[]) ?? [];
        resolve(records);
      };
    });
  }

  async getFolder(id: FolderId): Promise<FolderRecord | null> {
    const db = await this.getDatabase();
    return new Promise((resolve, reject) => {
      const transaction = db.transaction([this.storeName], "readonly");
      const store = transaction.objectStore(this.storeName);
      const request = store.get(id);
      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        const record = request.result as FolderRecord | undefined;
        resolve(record ?? null);
      };
    });
  }

  async clearAll(): Promise<void> {
    const db = await this.getDatabase();
    return new Promise((resolve, reject) => {
      const transaction = db.transaction([this.storeName], "readwrite");
      const store = transaction.objectStore(this.storeName);
      const request = store.clear();
      request.onerror = () => reject(request.error);
      request.onsuccess = () => resolve();
    });
  }
}

export const folderStorage = new FolderStorageService();
