//! Tests réels de robustesse et de cross-validation (`json-repair` vs `serde_json`).
//!
//! Vérifie en conditions réelles :
//! 1. Troncature à chaque frontière UTF-8 d'un payload LLM complexe.
//! 2. Fuzzing déterministe (5 000 entrées hostiles + octets invalides).
//! 3. Préservation byte-à-byte de JSON valides (ordre des clés, espaces, flottants).
//! 4. Performance linéaire O(n) sur un payload de ~1 Mo tronqué.

use serde_json::Value;
use std::time::Instant;

fn assert_valid_and_idempotent(input: &str, context: &str) -> String {
    let repaired = json_repair::repair(input);
    assert!(
        json_repair::is_valid(&repaired),
        "[{context}] json_repair::is_valid a rejeté la sortie pour {input:?} -> {repaired:?}"
    );
    let parsed: Result<Value, _> = serde_json::from_str(&repaired);
    assert!(
        parsed.is_ok(),
        "[{context}] serde_json a rejeté la sortie réparée: {repaired:?} (err={:?})",
        parsed.err()
    );
    let twice = json_repair::repair(&repaired);
    assert_eq!(
        repaired, twice,
        "[{context}] idempotence violée: once={repaired:?} twice={twice:?}"
    );
    repaired
}

#[test]
fn real_truncation_every_utf8_boundary_of_complex_llm_payload() {
    let payload = concat!(
        "Voici la réponse demandée :\n```json\n",
        r#"{"tool":"write_code","target_file":"src/lib.rs","meta":{"ok":true,"empty":null,"score":3.14159,"exp":-1.5e+4},"items":["éàü€","🎉","escaped \"quote\" and \\backslash\\",[1,2,3]]}"#,
        "\n```\nMerci !"
    );

    let mut count = 0usize;
    for (idx, _) in payload.char_indices() {
        let prefix = &payload[..idx];
        assert_valid_and_idempotent(prefix, &format!("prefix@{idx}"));
        count += 1;
    }
    assert_valid_and_idempotent(payload, "full");
    assert!(count > 150);
}

#[test]
fn real_deterministic_fuzz_5000_hostile_inputs_against_serde_json() {
    let fragments = [
        "{", "}", "[", "]", ":", ",", "\"", "\\", "true", "false", "null", "tru", "fal", "nu",
        "123", "-0.5e", "42.", "1.2.3", " ", "\n", "\t", "```json\n", "\n```", "~~~",
        "é", "€", "🎉", "prose ", "\x00", "\x1f", "\"key\"", "xyz",
    ];

    let mut seed: u64 = 0x5EED_CAFE_BABE_1234;
    let mut next_u64 = || -> u64 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };

    for case in 0..5_000 {
        let len = (next_u64() % 18) as usize;
        let mut s = String::new();
        for _ in 0..len {
            let idx = (next_u64() as usize) % fragments.len();
            s.push_str(fragments[idx]);
        }
        assert_valid_and_idempotent(&s, &format!("fuzz#{case}"));
    }

    // Test octets bruts invalides (UTF-8 corrompu)
    for case in 0..500 {
        let len = (next_u64() % 64) as usize;
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push((next_u64() & 0xFF) as u8);
        }
        let repaired = json_repair::repair_bytes(&bytes);
        assert!(
            json_repair::is_valid(&repaired),
            "[bytes#{case}] is_valid=false -> {repaired:?}"
        );
        assert!(
            serde_json::from_str::<Value>(&repaired).is_ok(),
            "[bytes#{case}] serde_json a rejeté -> {repaired:?}"
        );
    }
}

#[test]
fn real_valid_json_preserved_byte_identical() {
    let samples = [
        r#"{"z":1,"a":2,"nested":{"keep_order":true,"num":-0.0,"sci":1e+05,"uni":"é€🎉"}}"#,
        "[  1,   2,\n\t3 ]",
        "\"\"",
        "null",
    ];
    for s in samples {
        assert_eq!(json_repair::repair(s), s);
    }
}

#[test]
fn real_1mb_truncated_payload_linear_performance() {
    let mut big = String::with_capacity(1_100_000);
    big.push_str(r#"{"records":["#);
    for i in 0..25_000 {
        if i > 0 {
            big.push(',');
        }
        big.push_str(&format!(r#"{{"id":{},"tag":"item_{}","ok":true}}"#, i, i));
    }
    // Troncature volontaire en plein milieu du dernier objet
    big.push_str(r#",{"id":25000,"tag":"tronq"#);

    let start = Instant::now();
    let repaired = json_repair::repair(&big);
    let elapsed = start.elapsed();

    assert!(json_repair::is_valid(&repaired));
    assert!(serde_json::from_str::<Value>(&repaired).is_ok());
    assert!(
        elapsed.as_millis() < 2_000,
        "Réparation 1 Mo trop lente: {:?}",
        elapsed
    );
    println!(
        "  ⚡ [ROBUSTNESS] 1 Mo ({} bytes) réparé + validé en {:?}",
        big.len(),
        elapsed
    );
}
