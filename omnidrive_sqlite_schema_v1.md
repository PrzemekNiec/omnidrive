---
# OmniDrive VFS - SQLite Schema Spec v1

## 1. Overview
This document defines the local state store for the `angeld` process. All tables use SQLite-compatible types.

## 2. Table Definitions

### 2.1 Table: vault_state
Stores the current master key parameters and local vault status.
- `id`: INTEGER PRIMARY KEY CHECK (id = 1)
- `master_key_salt`: BLOB NOT NULL
- `argon2_params`: TEXT NOT NULL -- JSON representation of cost factors
- `vault_id`: TEXT NOT NULL

### 2.2 Table: inodes (Files & Directories)
- `id`: INTEGER PRIMARY KEY AUTOINCREMENT
- `parent_id`: INTEGER REFERENCES inodes(id)
- `name`: TEXT NOT NULL
- `kind`: TEXT NOT NULL -- 'FILE' or 'DIR'
- `size`: INTEGER DEFAULT 0
- `mode`: INTEGER
- `mtime`: INTEGER
- UNIQUE(parent_id, name)

### 2.3 Table: chunk_refs
Maps files to their encrypted chunks.
- `id`: INTEGER PRIMARY KEY AUTOINCREMENT
- `inode_id`: INTEGER REFERENCES inodes(id) ON DELETE CASCADE
- `chunk_id`: BLOB NOT NULL -- HMAC-SHA256 of plaintext
- `file_offset`: INTEGER NOT NULL
- `size`: INTEGER NOT NULL

### 2.4 Table: pack_locations
Maps chunk_ids to physical packs on S3.
- `chunk_id`: BLOB PRIMARY KEY
- `pack_id`: TEXT NOT NULL
- `pack_offset`: INTEGER NOT NULL
- `encrypted_size`: INTEGER NOT NULL

### 2.5 Table: upload_jobs
Queue for the background uploader.
- `id`: INTEGER PRIMARY KEY AUTOINCREMENT
- `pack_id`: TEXT UNIQUE NOT NULL
- `status`: TEXT NOT NULL -- 'PENDING', 'IN_PROGRESS', 'FAILED'
- `attempts`: INTEGER DEFAULT 0
---
