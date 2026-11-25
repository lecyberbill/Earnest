//! Présentation de `shell_guard`.

# shell_guard

Analyseur statique de lignes de commande shell (POSIX sh/bash et Windows
PowerShell/cmd) destiné à sécuriser l'exécution de commandes générées par un
agent IA.

- Rust pur, édition 2021, **zéro dépendance externe** (`std` uniquement).
- Ne lance jamais la commande analysée : l'analyse est purement textuelle.
- Ne panique jamais, même sur une entrée hostile (quote non fermée,
  sous-commande tronquée, profondeur d'imbrication extrême).

## API

```rust
use shell_guard::{inspect_command, RiskLevel, Verdict};

assert_eq!(
    inspect_command("ls -la"),
    Verdict::Allowed { risk: RiskLevel::ReadOnly }
);
assert!(inspect_command("vim notes.txt").is_blocked());
assert!(matches!(
    inspect_command("rm -rf /"),
    Verdict::DangerousBlocked { .. }
));
```

Trois verdicts :

| Verdict | Signification |
| --- | --- |
| `Allowed { risk }` | Exécutable ; `RiskLevel::ReadOnly` ou `RiskLevel::Mutating`. |
| `InteractiveBlocked { command, reason, suggestion }` | La commande attendrait un humain et ne rendrait pas la main. |
| `DangerousBlocked { command, reason }` | Opération critique irréversible. |

## Pipeline

1. `segment::split_segments` — découpage conscient des quotes `'…'` / `"…"`, des
   échappements `\` et `` ` ``, des sous-commandes `$( … )` et des séparateurs
   `;`, `&&`, `||`, `|`, retour à la ligne.
2. `normalize::normalize` — identification de la commande réelle : suppression des
   préfixes (`sudo`, `env`, `timeout`, `nice`, `nohup`…) et traversée des shells
   embarqués (`bash -c`, `pwsh -Command`, `cmd /c`).
3. `rules::dangerous` — blocage des opérations critiques (racine du système de
   fichiers, formatage, écriture sur périphérique bloc, arrêt de l'hôte,
   `curl … | sh`, fork bomb).
4. `rules::interactive` — blocage des commandes bloquantes : éditeurs, pagineurs,
   `read` sans entrée pipée, REPL d'interpréteurs, clients de base de données,
   `git commit` sans `-m`, `tail -f`, `docker run -it`, etc.
5. `rules::classify` — qualification du risque (`ReadOnly` uniquement si démontré,
   conservateur sinon).

Priorité : un danger critique l'emporte sur un blocage interactif, où qu'il soit
placé dans la ligne.

## Tests

```text
cargo test
```
