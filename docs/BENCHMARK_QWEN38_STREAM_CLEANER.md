# Benchmark Run — `stream-cleaner` (Qwen 3.8 Local + Earnest IDE Governance)

**Date**: 2026-11-22  
**Target Crate**: `crates/stream-cleaner` (développée dans `D:\Projet\test_vib`, zéro dépendance externe)  
**Model**: `qwen3.8:27b` (Ollama local, effort `Low`)  
**Workflow**: Mode 3 (Spec-First) → Scellement SHA-256 → Mode 2 (Shadow Worktree + `project-integrity-gate`)

---

## 1. Architecture & Contrats scellés (Mode 3)

- **Machine à états** : `Outside | InThought` avec buffer `pending` et calcul du plus long suffixe/préfixe (`max_prefix_suffix`) pour reconstruire les balises coupées entre chunks.
- **Contrats (`C1`–`C6`)** :
  - `C1` : Balise complète → routage `visible` / `thought`
  - `C2` : Balise coupée sur plusieurs `push()`
  - `C3` : Texte avant et après la balise émis en `visible`
  - `C4` : Balise non fermée au `finish()` → reste routé en `thought`
  - `C5` : Texte sans balise → `visible` uniquement
  - `C6` : Support de la paire `<think>...</think>` (en plus de `<thought>...</thought>`)

---

## 2. Observations & Auto-débogage en Mode 2

- Lors du premier essai dans `test_vib`, le tokenizer/parser de `qwen3.8` dans Ollama a intercepté `<think>` et `</think>` à l'intérieur des arguments JSON de `write_code`, tronquant `>` / `</think>`.
- L'agent a écrit un test de diagnostic `test_debug_bytes` via `shadow_check` pour inspecter les octets réels et s'est corrigé avec l'échappement Unicode `\u{3E}`.
- **Sondes finales dans Shadow Worktree** : **6/6 tests passés** + `cargo clippy` (`project-integrity-gate`) **PASS** → fusion atomique dans le workspace.
- **Observation complémentaire (Mode 1 — `SlidingWindowLimiter`, 4/4 tests)** : mise en évidence du comportement PowerShell 5.1 (`;` au lieu de `&&`) et de la nécessité d'exposer `kb_list_products` + une résolution unifiée de `kb.db` pour les projets externes.

---

## 3. Intégration dans Earnest (`crates/stream-cleaner`)

- Porté vers `crates/stream-cleaner` (`Cargo.toml`, `src/lib.rs`, `src/stream_cleaner.rs`) avec la paire complète `"\u{3C}think\u{3E}"` / `"\u{3C}/think\u{3E}"` et un 7ᵉ test (`test_split_think_tag`).
- **`cargo test --manifest-path crates\stream-cleaner\Cargo.toml`** : **7/7 tests passés** (`0.00s`).
- Corrections associées dans `earnest-ide-core` :
  - `resolve_kb_db_path()` partagé (`tools/mod.rs`, `server/mod.rs`, `engine/react.rs`) + exposition de `kb_list_products` et auto-création du produit manquant dans `kb_propose`.
  - Création manuelle de produits et filtre par produit dans la vue BCPE (`kb-view.js`, `/api/ide/kb/products`).
