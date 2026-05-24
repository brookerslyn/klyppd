import Database from "better-sqlite3";
import path from "node:path";

export interface Clip {
  id: string;
  filename: string;
  path: string;
  duration: number;
  created_at: string;
  thumbnail_path: string | null;
  tags: string | null;
  folder: string | null;
  upload_status: string;
  r2_key: string | null;
  r2_url: string | null;
  expiry_date: string | null;
  is_permanent: boolean;
}

const SCHEMA = `
CREATE TABLE IF NOT EXISTS clips (
    id              TEXT PRIMARY KEY,
    filename        TEXT NOT NULL,
    path            TEXT NOT NULL,
    duration        REAL NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    thumbnail_path  TEXT,
    tags            TEXT,
    folder          TEXT,
    upload_status   TEXT NOT NULL DEFAULT 'local',
    r2_key          TEXT,
    r2_url          TEXT,
    expiry_date     TEXT,
    is_permanent    INTEGER NOT NULL DEFAULT 0
);`;

function rowToClip(row: Record<string, unknown>): Clip {
  return {
    id: row.id as string,
    filename: row.filename as string,
    path: row.path as string,
    duration: row.duration as number,
    created_at: row.created_at as string,
    thumbnail_path: (row.thumbnail_path as string) || null,
    tags: (row.tags as string) || null,
    folder: (row.folder as string) || null,
    upload_status: row.upload_status as string,
    r2_key: (row.r2_key as string) || null,
    r2_url: (row.r2_url as string) || null,
    expiry_date: (row.expiry_date as string) || null,
    is_permanent: Boolean(row.is_permanent),
  };
}

export class ClipDatabase {
  private db: Database.Database;

  constructor(dbPath: string) {
    this.db = new Database(dbPath);
    this.db.pragma("journal_mode = WAL");
    this.db.exec(SCHEMA);
  }

  insertClip(c: Clip): void {
    this.db
      .prepare(
        `INSERT INTO clips
       (id, filename, path, duration, created_at, thumbnail_path, tags, folder,
        upload_status, r2_key, r2_url, expiry_date, is_permanent)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`
      )
      .run(
        c.id,
        c.filename,
        c.path,
        c.duration,
        c.created_at,
        c.thumbnail_path,
        c.tags,
        c.folder,
        c.upload_status,
        c.r2_key,
        c.r2_url,
        c.expiry_date,
        c.is_permanent ? 1 : 0
      );
  }

  getAllClips(): Clip[] {
    const rows = this.db
      .prepare(
        "SELECT * FROM clips WHERE upload_status != 'deleted' ORDER BY created_at DESC"
      )
      .all() as Record<string, unknown>[];
    return rows.map(rowToClip);
  }

  getClipsByFolder(folder: string): Clip[] {
    const rows = this.db
      .prepare("SELECT * FROM clips WHERE folder = ? ORDER BY created_at DESC")
      .all(folder) as Record<string, unknown>[];
    return rows.map(rowToClip);
  }

  getUploadedClips(permanent: boolean): Clip[] {
    const rows = this.db
      .prepare(
        "SELECT * FROM clips WHERE upload_status = 'uploaded' AND is_permanent = ? ORDER BY created_at DESC"
      )
      .all(permanent ? 1 : 0) as Record<string, unknown>[];
    return rows.map(rowToClip);
  }

  getClip(id: string): Clip {
    const row = this.db
      .prepare("SELECT * FROM clips WHERE id = ?")
      .get(id) as Record<string, unknown>;
    if (!row) throw new Error(`Clip not found: ${id}`);
    return rowToClip(row);
  }

  updateClipTags(id: string, tags: string): void {
    this.db.prepare("UPDATE clips SET tags = ? WHERE id = ?").run(tags, id);
  }

  updateClipFolder(id: string, folder: string): void {
    this.db.prepare("UPDATE clips SET folder = ? WHERE id = ?").run(folder, id);
  }

  markUploaded(
    id: string,
    url: string,
    permanent: boolean,
    expiry: string | null
  ): void {
    const key = url.split("/").pop() || url;
    this.db
      .prepare(
        `UPDATE clips SET upload_status = 'uploaded', r2_url = ?, r2_key = ?,
       is_permanent = ?, expiry_date = ? WHERE id = ?`
      )
      .run(url, key, permanent ? 1 : 0, expiry, id);
  }

  markDeleted(id: string): void {
    this.db
      .prepare(
        "UPDATE clips SET upload_status = 'deleted', r2_key = NULL, r2_url = NULL WHERE id = ?"
      )
      .run(id);
  }

  deleteClip(id: string): void {
    this.db.prepare("DELETE FROM clips WHERE id = ?").run(id);
  }

  renameClip(id: string, filename: string, clipPath: string): void {
    this.db
      .prepare("UPDATE clips SET filename = ?, path = ? WHERE id = ?")
      .run(filename, clipPath, id);
  }

  close(): void {
    this.db.close();
  }
}
