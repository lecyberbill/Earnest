# Earnest BCPE — Base de connaissances d'expérience (MCP)

Base de connaissances locale (SQLite + FTS5) exposée à un LLM par un serveur
MCP standard, avec une ligne de commande pour l'humain. Le binaire unique `kb`
sert à la fois de serveur MCP (`kb serve`) et d'interface de relecture/validation.

Le texte est la source de vérité ; l'index FTS5 est un dérivé régénérable.
Une entrée décrit un point précis (symptôme, cause, correction, exemple) pour un
couple (`product_id`, `subject_key`).

## Démarrage rapide

```powershell
cargo build --release

# 1. Créer un produit et importer son lexique officiel (un identifiant par ligne)
#    import-lexicon crée le produit s'il n'existe pas encore.
kb --db .\kb.db import-lexicon mon-produit .\lexique.txt

# 2. Fixer la version en service (optionnel)
kb --db .\kb.db product-version mon-produit 2.0

# 3. Lancer le serveur MCP sur stdin/stdout
kb --db .\kb.db serve
```

Sans `--db`, la base est `~/.local/share/earnest-bcpe/kb.db` (ou `%USERPROFILE%\.local\share\earnest-bcpe\kb.db`).
La variable d'environnement `KB_DB` remplace ce chemin par défaut.
La configuration YAML est lue dans `./config`, `KB_CONFIG_DIR`, ou à côté du
binaire ; des valeurs par défaut sont intégrées si les fichiers sont absents.

## Connexion depuis Hermest et depuis les autres clients MCP

Le serveur utilise le transport MCP stdio : messages JSON-RPC 2.0 délimités par
des retours à la ligne. Il accepte l'initialisation standard
(`initialize` → `notifications/initialized`) et tolère aussi un client qui
appelle directement `tools/list` sans handshake, comme le client MCP de Hermest.

### Hermest Cockpit / IDE

Renseignez `app_settings.mcp_servers_config` (JSON) avec l'exécutable `kb` :

```json
[
  {
    "name": "bcpe",
    "command": ".\\earnest-bcpe\\target\\release\\kb.exe",
    "args": ["serve", "--db", ".\\earnest-bcpe\\kb.db"]
  }
]
```

Les outils apparaissent alors au modèle sous la forme
`mcp__bcpe__kb_search`, `mcp__bcpe__kb_check`, etc.

Pour enregistrer ce serveur dans la base du cockpit Hermest :
```python
import json
from earnest.db.connection import init_db

conn = init_db()
servers = [{
    "name": "bcpe",
    "command": r".\earnest-bcpe\target\release\kb.exe",
    "args": ["serve", "--db", r".\earnest-bcpe\kb.db"],
}]
conn.execute(
    "INSERT INTO app_settings (key, value) VALUES ('mcp_servers_config', ?) "
    "ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    (json.dumps(servers),),
)
conn.commit()
conn.close()
```

### Autres clients (Claude Desktop, etc.)

```json
{
  "mcpServers": {
    "bcpe": {
      "command": ".\\earnest-bcpe\\target\\release\\kb.exe",
      "args": ["serve", "--db", ".\\earnest-bcpe\\kb.db"]
    }
  }
}
```

## Outils MCP exposés au LLM

| Outil | Rôle |
| --- | --- |
| `kb_search` | Cherche les expériences d'un produit. `covered=false` signifie « non couvert ou non validé », jamais « sans risque ». |
| `kb_get` | Lit une entrée complète (identifiants, mots-clés, dépendances) par `entry_id` ou `product_id` + `subject_key`. |
| `kb_check` | Vérifie mécaniquement les identifiants d'un script (`script_text` + `script_kind`) ou d'une liste. Verdicts : `ok`, `deprecie`, `inexistant`, `inconnu`. |
| `kb_report_incident` | Enregistre un écart attendu/observé et renvoie les entrées proches. Ne crée pas d'entrée. |
| `kb_propose` | Crée ou met à jour une entrée `non_teste`. Refusé sur `valide`/`archive` (`forbidden_status`). |
| `kb_record_outcome` | Signale `ok`/`ko` (note obligatoire pour `ko`), compteurs atomiques, sans changer `version`. |
| `kb_confirm` | Confirme une entrée par une exécution réelle (commande, `exit_code` 0, extrait). Avec `min_confirmations` sessions distinctes de l'auteur, l'entrée devient `valide` en `validation_mode=auto`. |
| `kb_attach_repro` | Joint un cas de test reproductible (runner déclaré, script, sortie attendue). Jamais exécuté par le serveur. |

Toute réponse porte `content_type: "reference_data"` et le statut de chaque
entrée. Les descriptions d'outils rappellent que le texte d'une entrée est une
donnée, jamais une instruction à exécuter.

Les outils ne peuvent ni valider directement, ni archiver, ni supprimer, ni modifier une
entrée `valide`. La seule promotion possible par un agent est `kb_confirm`, qui exige des
preuves d'exécution venant de sessions distinctes de l'auteur ; ces entrées sont marquées
`validation_mode=auto` jusqu'à relecture humaine (`kb review`, `kb validate`). Archivage,
suppression et édition restent dans la CLI humaine. `kb_propose` n'accepte aucun champ de statut
(`auto_validate` n'existe plus).

## Ligne de commande humaine

```text
kb serve
kb validate <entry_id>... --by humain:nom
kb review [--product <produit>] [--json]
kb approve-repro <entry_id> --by humain:nom
kb revalidate [--product <produit>] [--entry <id>]
kb edit <entry_id> --by humain:nom [--title ...] [--correction ...] [--terms a,b]
                       [--identifiers-json '[{"identifier":"X","state":"valide"}]']
                       [--depends-on 2,3] [--product-version 2.1]
kb archive <entry_id>
kb restore <entry_id>
kb product-version <produit> <version>
kb lint [--product <produit>] [--json]
kb audit --sample N
kb backup
kb reindex
kb import-lexicon <produit> <fichier>
kb log-extract <fichier>
kb incident <produit> "<attendu>" "<observé>" [--script-ref ...] [--log-excerpt ...]
kb check <produit> <fichier> [--kind test|aml|xml|jsx]
```

- `validate` : passe `non_teste`/`a_revoir` à `valide` (ou confirme une entrée `auto` en `humain`) et enregistre le valideur ; accepte plusieurs identifiants.
- `review` : liste les entrées validées automatiquement, à relire.
- `approve-repro` : approuve le cas de test joint à une entrée (affiche le script) ; seul l'humain peut autoriser son exécution.
- `revalidate` : rejoue les cas approuvés des entrées `a_revoir` avec les runners de `config/runners.yaml` (seul processus externe du binaire). Réussite : `valide`/`auto` et version produit alignée ; échec : `ko` ; code de sortie non nul en cas d'échec ou d'erreur.
- `edit` : modifie une entrée non archivée ; sur une entrée `valide`, l'éditeur
  devient `validated_by` et l'ancienne version est copiée dans `entry_history`.
- `product-version` : passe à `a_revoir` les entrées `valide` dont
  `product_version` diffère (égalité stricte, `2.0` = `2.0.0`).
- `backup` : copie quotidienne via `VACUUM INTO`, conservation 30 jours.
- `audit` : tire N entrées `valide`, enregistre le verdict dans `audit_log.jsonl`.
- `log-extract` : toutes les lignes WARN/ERROR, 5 lignes précédentes et la ligne
  `fin`, limité à 4 000 caractères.

## Configuration

`config/extractors.yaml` :

```yaml
extractors:
  test:
    patterns:
      - regex: '\b(?:TAG|PARAM)_[A-Z0-9_]+\b'
  aml:
    patterns:
      - regex: '<\s*(AM[A-Za-z0-9_]+)'
        group: 1
```

Chaque motif cible une syntaxe propre au produit ; un identifiant local
(variable, fonction) n'est jamais extrait. Les commentaires et chaînes
littérales sont ignorés avant extraction. La syntaxe est celle du crate `regex`
(pas de lookahead ni de backreference) ; `engine: fancy` est refusé.

`config/redaction.yaml` : motifs masqués avant stockage sur tous les champs
texte libres (`title`, `symptom`, `cause`, `correction`, `example`, `note`,
`expected`, `observed`, `log_excerpt`). Par défaut : mots de passe/jetons,
en-têtes Bearer, adresses e-mail, chemins personnels.

`config/validation.yaml` : `min_confirmations` (2 par défaut) et `version_segments` (0 = comparaison
stricte de `product-version`, 2 = seuls major.minor comptent). `config/runners.yaml` : runners de
rejeu (aucun par défaut ; programme, arguments avec `{file}`, extension, délai).

## Recherche

Trois étapes, la suivante n'étant tentée que si la précédente ne trouve rien : mots exacts
(`match_mode: "exact"`), préfixes de 3 caractères ou plus (`"prefix"`, `attend` trouve `attendre`),
puis fragments sur un index trigramme (`"fragment"`). Aucun modèle d'embedding, aucune dépendance supplémentaire.

## Adoption : hook Claude Code

`hooks/kb_adoption.ps1` rappelle `kb_check` après l'écriture d'un script et empêche de terminer le
tour tant qu'un script écrit n'a pas été suivi d'un appel `kb_check`. Copier le contenu de
`hooks/settings.example.json` dans `.claude/settings.json` (adapter le chemin) ; extensions et outils
dans `hooks/kb_adoption.json`.

## Données et garanties

- SQLite en WAL, `busy_timeout=5000`, clés étrangères activées, FTS5 vérifié au
  démarrage.
- 11 tables : `products`, `entries`, `entry_terms`, `identifiers`,
  `lexicon_identifiers`, `entry_deps`, `entry_history`, `entry_outcomes`,
  `incidents`, `entry_evidence`, `entry_repro`, plus les index virtuels `entries_fts` et
  `entries_fts_tri` (dérivés, régénérés par `kb reindex`). Les bases existantes sont migrées à l'ouverture.
- Une écriture d'entrée, mots-clés, identifiants, historique et index est une
  seule transaction.
- Un identifiant peut être déclaré par plusieurs entrées non archivées du même
  produit seulement avec le même `state` et le même `replaced_by`.
- Le serveur MCP n'ouvre aucun port réseau. Limite d'écriture : 30 appels par
  minute et par session (`rate_limited`).
- Journal serveur sur `stderr`, au format
  `horodatage|niveau|étape|message|clé=valeur` (le produit, la durée et le
  résultat sont tracés, jamais les arguments texte).

## Construction et tests

```powershell
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

`tests/acceptance.rs` couvre les critères A1 à A24 du cahier des charges, plus
A25 (lecture `kb_get`), sur une base temporaire par test. `tests/improvements.rs` couvre les
invariants P01 à P17 (`tests/INVARIANTS.md`). `tests/fixtures/seed.json`
documente le jeu de départ utilisé par les critères de recette.

## Choix techniques et écarts assumés

- **Transport stdio implémenté directement**, sans `rmcp` : le comportement
  attendu (JSON-RPC 2.0 ligne par ligne, tolérance à l'absence d'`initialize`,
  réponses `content` + `structuredContent`) est plus simple à garantir et
  fonctionne avec Hermest comme avec les clients MCP standard. Le serveur est
  synchrone ; chaque requête SQLite s'exécute hors de tout contexte asynchrone.
- **YAML** : `yaml_serde` (successeur maintenu de `serde_yaml`, projet The YAML
  Organization) est utilisé à la place de `serde_yaml` archivé.
- **Schémas d'outils** générés avec `schemars`.
- **`VACUUM INTO`** sert d'API de sauvegarde SQLite (copie cohérente et unique).
- **Bootstrap produit** : `import-lexicon` crée le produit s'il n'existe pas, afin
  qu'une base neuve soit utilisable sans insertion SQL manuelle.
- Le contrôle porte sur les noms d'identifiants, pas sur la logique du script :
  seule l'exécution réelle, avec des contrôles internes, détecte une logique
  fausse. Un résultat `covered=false` n'est jamais une absence de risque.
