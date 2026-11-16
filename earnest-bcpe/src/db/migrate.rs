use rusqlite::Connection;

use crate::core::queries::rebuild_trigram_index;
use crate::error::{KbError, KbResult};

const SCHEMA: &str = include_str!("schema.sql");

/// Colonnes ajoutées à `entries` après la première version du schéma.
const ADDED_ENTRY_COLUMNS: [(&str, &str); 3] = [
    (
        "validation_mode",
        "TEXT CHECK (validation_mode IN ('humain','auto'))",
    ),
    ("proposed_by", "TEXT"),
    ("evidence_floor", "INTEGER NOT NULL DEFAULT 0"),
];

fn entry_columns(conn: &Connection) -> KbResult<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info('entries')")
        .map_err(KbError::Db)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(KbError::Db)?;
    let mut names = Vec::new();
    for row in rows {
        names.push(row.map_err(KbError::Db)?);
    }
    Ok(names)
}

fn count(conn: &Connection, sql: &str) -> KbResult<i64> {
    conn.query_row(sql, [], |row| row.get(0))
        .map_err(KbError::Db)
}

pub fn init(conn: &Connection) -> KbResult<()> {
    conn.execute_batch(SCHEMA).map_err(KbError::Db)?;

    let existing = entry_columns(conn)?;
    for (name, definition) in ADDED_ENTRY_COLUMNS {
        if !existing.iter().any(|column| column == name) {
            // Deux processus peuvent migrer en même temps : l'échec « duplicate column »
            // signifie que l'autre a déjà ajouté la colonne ; on revérifie après coup.
            let result = conn.execute(
                &format!("ALTER TABLE entries ADD COLUMN {name} {definition}"),
                [],
            );
            if let Err(error) = result {
                if !entry_columns(conn)?.iter().any(|column| column == name) {
                    return Err(KbError::Db(error));
                }
            }
        }
    }

    count(conn, "SELECT count(*) FROM entries_fts").map_err(|e| {
        KbError::Internal(format!("FTS5 indisponible dans le SQLite embarqué: {e}"))
    })?;
    count(conn, "SELECT count(*) FROM entries_fts_tri").map_err(|e| {
        KbError::Internal(format!(
            "tokenizer trigram indisponible dans le SQLite embarqué: {e}"
        ))
    })?;

    if count(conn, "SELECT count(*) FROM entries_fts_tri")? == 0
        && count(conn, "SELECT count(*) FROM entries")? > 0
    {
        conn.execute_batch("BEGIN IMMEDIATE").map_err(KbError::Db)?;
        let populate = (|| -> KbResult<()> {
            if count(conn, "SELECT count(*) FROM entries_fts_tri")? == 0 {
                rebuild_trigram_index(conn)?;
            }
            Ok(())
        })();
        match populate {
            Ok(()) => conn.execute_batch("COMMIT").map_err(KbError::Db)?,
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
    }
    Ok(())
}
