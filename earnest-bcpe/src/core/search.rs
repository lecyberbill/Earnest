use rusqlite::params_from_iter;
use rusqlite::types::Value;
use rusqlite::Connection;

use crate::core::queries::{effective_mode, ensure_product, truncate_chars};
use crate::error::{KbError, KbResult};
use crate::models::{EntrySummary, SearchInput, SearchResponse};

const COVERED_MESSAGE: &str =
    "non couvert ou non validé : vérifier dans la documentation du produit";

fn validate_kinds(kinds: &[String]) -> KbResult<()> {
    for kind in kinds {
        match kind.as_str() {
            "catalogue" | "piege" | "regle" => {}
            other => {
                return Err(KbError::invalid(format!(
                    "kind '{other}' invalide; valeurs admises: catalogue, piege, regle"
                )))
            }
        }
    }
    Ok(())
}

fn collect_tokens(query: Option<&str>, terms: &[String]) -> KbResult<Vec<String>> {
    let mut tokens: Vec<String> = Vec::new();
    if let Some(query) = query {
        for token in query.split_whitespace() {
            if !token.is_empty() {
                tokens.push(token.to_string());
            }
        }
    }
    for term in terms {
        for token in term.split_whitespace() {
            if !token.is_empty() {
                tokens.push(token.to_string());
            }
        }
    }
    if tokens.is_empty() {
        return Err(KbError::invalid(
            "query ou terms[] doit fournir au moins un mot recherché",
        ));
    }
    Ok(tokens)
}

fn quote(token: &str) -> String {
    format!("\"{}\"", token.replace('"', "\"\""))
}

/// Étape 1 : mots exacts.
fn exact_expression(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|token| quote(token))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Étape 2 : préfixe dès 3 caractères (`attend` trouve `attendre`). Plus coûteux que l'étape 1,
/// donc exécuté seulement si l'étape 1 ne trouve rien.
fn prefix_expression(tokens: &[String]) -> Option<String> {
    if !tokens.iter().any(|token| token.chars().count() >= 3) {
        return None;
    }
    Some(
        tokens
            .iter()
            .map(|token| {
                if token.chars().count() >= 3 {
                    format!("{}*", quote(token))
                } else {
                    quote(token)
                }
            })
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

/// Étape 3 : repli sur l'index trigramme : sous-chaîne d'au moins 3 caractères.
fn fragment_expression(tokens: &[String]) -> Option<String> {
    let parts: Vec<String> = tokens
        .iter()
        .filter(|token| token.chars().count() >= 3)
        .map(|token| quote(token))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" OR "))
    }
}

fn build_excerpt(correction: &str, symptom: Option<&str>) -> String {
    let base = if !correction.trim().is_empty() {
        correction
    } else {
        symptom.unwrap_or("")
    };
    let flattened = base.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&flattened, 220)
}

struct Page {
    results: Vec<EntrySummary>,
    covered: bool,
    more_available: bool,
    total: i64,
}

fn run_query(
    conn: &Connection,
    table: &str,
    match_expr: String,
    input: &SearchInput,
    limit: i64,
    offset: i64,
) -> KbResult<Page> {
    let mut where_sql = format!("{table} MATCH ? AND e.product_id = ? AND e.status <> 'archive'");
    let mut base_values: Vec<Value> = vec![
        Value::Text(match_expr),
        Value::Text(input.product_id.clone()),
    ];
    if !input.kinds.is_empty() {
        let placeholders = vec!["?"; input.kinds.len()].join(", ");
        where_sql.push_str(&format!(" AND e.kind IN ({placeholders})"));
        for kind in &input.kinds {
            base_values.push(Value::Text(kind.clone()));
        }
    }

    let from_sql = format!("FROM {table} JOIN entries e ON e.entry_id = {table}.rowid");

    let (total, covered): (i64, bool) = conn
        .query_row(
            &format!(
                "SELECT COUNT(*), COALESCE(MAX(e.status = 'valide'), 0) {from_sql} WHERE {where_sql}"
            ),
            params_from_iter(base_values.iter()),
            |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
        )
        .map_err(KbError::Db)?;

    let mut page_values = base_values.clone();
    page_values.push(Value::Integer(limit));
    page_values.push(Value::Integer(offset));
    let sql = format!(
        "SELECT e.entry_id, e.subject_key, e.title, e.kind, e.status, e.version,
                e.correction, e.symptom, bm25({table}) AS rank, e.validation_mode
         {from_sql}
         WHERE {where_sql}
         ORDER BY CASE e.status
                    WHEN 'valide' THEN 0
                    WHEN 'non_teste' THEN 1
                    WHEN 'a_revoir' THEN 2
                    ELSE 3
                  END,
                  CASE
                    WHEN e.product_version IS NOT NULL
                     AND e.product_version = (SELECT current_version FROM products WHERE product_id = e.product_id)
                    THEN 0 ELSE 1
                  END,
                  (e.use_count - e.fail_count) DESC,
                  rank ASC, e.entry_id ASC
         LIMIT ? OFFSET ?"
    );
    let mut stmt = conn.prepare(&sql).map_err(KbError::Db)?;
    let rows = stmt
        .query_map(params_from_iter(page_values.iter()), |row| {
            let correction: String = row.get(6)?;
            let symptom: Option<String> = row.get(7)?;
            let rank: f64 = row.get(8)?;
            let status: String = row.get(4)?;
            let mode: Option<String> = row.get(9)?;
            Ok(EntrySummary {
                entry_id: row.get(0)?,
                subject_key: row.get(1)?,
                title: row.get(2)?,
                kind: row.get(3)?,
                validation_mode: effective_mode(&status, mode.as_deref()),
                status,
                version: row.get(5)?,
                excerpt: build_excerpt(&correction, symptom.as_deref()),
                score: -rank,
            })
        })
        .map_err(KbError::Db)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(KbError::Db)?);
    }
    let more_available = total > offset + results.len() as i64;
    Ok(Page {
        results,
        covered,
        more_available,
        total,
    })
}

pub fn search(conn: &Connection, input: &SearchInput) -> KbResult<SearchResponse> {
    ensure_product(conn, &input.product_id)?;
    validate_kinds(&input.kinds)?;

    let limit = match input.limit {
        None => 5,
        Some(value) if (1..=20).contains(&value) => value,
        Some(_) => {
            return Err(KbError::invalid(
                "limit doit être comprise entre 1 et 20 (5 par défaut)",
            ))
        }
    };
    let offset = match input.offset {
        None => 0,
        Some(value) if value >= 0 => value,
        Some(_) => return Err(KbError::invalid("offset doit être positif ou nul")),
    };

    let tokens = collect_tokens(input.query.as_deref(), &input.terms)?;
    let mut match_mode = "exact";
    let mut page = run_query(
        conn,
        "entries_fts",
        exact_expression(&tokens),
        input,
        limit,
        offset,
    )?;
    if page.total == 0 {
        if let Some(expression) = prefix_expression(&tokens) {
            let fallback = run_query(conn, "entries_fts", expression, input, limit, offset)?;
            if fallback.total > 0 {
                page = fallback;
                match_mode = "prefix";
            }
        }
    }
    if page.total == 0 {
        if let Some(expression) = fragment_expression(&tokens) {
            let fallback = run_query(conn, "entries_fts_tri", expression, input, limit, offset)?;
            if fallback.total > 0 {
                page = fallback;
                match_mode = "fragment";
            }
        }
    }

    let message = if page.covered {
        None
    } else {
        Some(COVERED_MESSAGE.to_string())
    };

    Ok(SearchResponse {
        results: page.results,
        covered: page.covered,
        more_available: page.more_available,
        total: page.total,
        match_mode,
        message,
        product_status: None,
    })
}
