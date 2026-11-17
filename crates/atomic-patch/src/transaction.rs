//! Transaction atomique multi-fichiers : `prepare` (sans écriture) puis
//! `commit` (seule phase d'écriture). En cas d'échec, rollback complet.
//!
//! Zéro dépendance : `std::fs` uniquement.

use std::io::ErrorKind;
use std::path::PathBuf;

/// Une opération de patch à appliquer à un fichier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchOperation {
    /// Chemin du fichier cible (relatif ou absolu).
    pub path: PathBuf,
    /// Nouveau contenu à écrire en phase `commit`.
    pub new_content: String,
}

/// Issue d'une transaction [`Transaction::commit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionOutcome {
    /// Toutes les opérations ont été appliquées.
    Committed {
        /// Fichiers effectivement écrits, dans l'ordre d'application.
        applied: Vec<PathBuf>,
    },
    /// Échec : les opérations déjà appliquées ont été restaurées.
    RolledBack {
        /// Index (0-based) de l'opération en cause.
        failed_at: usize,
        /// Cause d'échec.
        reason: String,
        /// Fichiers restaurés pendant le rollback.
        restored: Vec<PathBuf>,
    },
}

/// Transaction multi-fichiers : les opérations sont cumulées via [`add_op`]
/// puis appliquées de manière atomique par [`commit`].
pub struct Transaction {
    ops: Vec<PatchOperation>,
}

impl Default for Transaction {
    fn default() -> Self {
        Self::new()
    }
}

impl Transaction {
    /// Crée une transaction vide.
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    /// Ajoute une opération (style builder, consomme et retourne `self`).
    pub fn add_op(mut self, op: PatchOperation) -> Self {
        self.ops.push(op);
        self
    }

    /// Nombre d'opérations en attente.
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// `true` si aucune opération n'est enregistrée.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Phase `prepare` : lit l'état courant (sauvegarde) de chaque cible.
    /// **Ne modifie jamais le disque.**
    fn prepare(&self) -> Result<Vec<(PathBuf, Option<String>)>, String> {
        let mut plan = Vec::with_capacity(self.ops.len());
        for op in &self.ops {
            match std::fs::read_to_string(&op.path) {
                Ok(content) => plan.push((op.path.clone(), Some(content))),
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    plan.push((op.path.clone(), None))
                }
                Err(e) => {
                    return Err(format!(
                        "lecture impossible de {} : {}",
                        op.path.display(),
                        e
                    ))
                }
            }
        }
        Ok(plan)
    }

    /// Phase `commit` : seule phase d'écriture. Applique chaque opération
    /// dans l'ordre ; en cas d'échec, restaure les écritures précédentes.
    pub fn commit(self) -> TransactionOutcome {
        // 1) Préparation (aucune écriture).
        let plan = match self.prepare() {
            Ok(plan) => plan,
            Err(reason) => {
                return TransactionOutcome::RolledBack {
                    failed_at: 0,
                    reason,
                    restored: Vec::new(),
                }
            }
        };

        // 2) Application (seule phase d'écriture).
        let mut applied: Vec<(PathBuf, Option<String>)> = Vec::new();
        for (i, (path, backup)) in plan.iter().enumerate() {
            let new_content = &self.ops[i].new_content;
            match std::fs::write(path, new_content) {
                Ok(()) => applied.push((path.clone(), backup.clone())),
                Err(e) => {
                    // 3) Rollback : restaurer les écritures déjà effectuées.
                    let mut restored = Vec::new();
                    for (p, backup) in applied.iter().rev() {
                        let _ = match backup {
                            Some(content) => std::fs::write(p, content),
                            None => std::fs::remove_file(p),
                        };
                        restored.push(p.clone());
                    }
                    restored.reverse();
                    return TransactionOutcome::RolledBack {
                        failed_at: i,
                        reason: e.to_string(),
                        restored,
                    };
                }
            }
        }

        TransactionOutcome::Committed {
            applied: applied.into_iter().map(|(p, _)| p).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn tmp_dir(tag: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("atomic-patch-test-{}-{}", tag, std::process::id()));
        let _ = fs::create_dir_all(&d);
        d
    }

    fn cleanup(d: &Path) {
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn commit_creates_and_edits_files() {
        let d = tmp_dir("commit");
        let f = d.join("a.txt");
        fs::write(&f, "v0").unwrap();

        let t = Transaction::new().add_op(PatchOperation {
            path: f.clone(),
            new_content: "v1".to_string(),
        });

        let outcome = t.commit();
        let applied = match &outcome {
            TransactionOutcome::Committed { applied } => applied.clone(),
            TransactionOutcome::RolledBack { .. } => panic!("attendu Committed"),
        };
        assert_eq!(applied, vec![f.clone()]);
        assert_eq!(fs::read_to_string(&f).unwrap(), "v1");
        cleanup(&d);
    }

    #[test]
    fn atomicity_rolls_back_on_failure() {
        let d = tmp_dir("atomic");
        let f1 = d.join("1.txt");
        let f2 = d.join("2.txt");
        fs::write(&f1, "one").unwrap();
        fs::write(&f2, "two").unwrap();

        let bad = d.join("does-not-exist-dir").join("x.txt");

        let t = Transaction::new()
            .add_op(PatchOperation {
                path: f1.clone(),
                new_content: "ONE".to_string(),
            })
            .add_op(PatchOperation {
                path: f2.clone(),
                new_content: "TWO".to_string(),
            })
            .add_op(PatchOperation {
                path: bad,
                new_content: "three".to_string(),
            });

        let outcome = t.commit();
        match outcome {
            TransactionOutcome::RolledBack { failed_at, restored, .. } => {
                assert_eq!(failed_at, 2, "l'échec survient sur la 3e opération");
                assert_eq!(restored.len(), 2, "les 2 écritures sont restaurées");
            }
            _ => panic!("attendu RolledBack"),
        }

        // État restauré : aucun effet persistant.
        assert_eq!(fs::read_to_string(&f1).unwrap(), "one");
        assert_eq!(fs::read_to_string(&f2).unwrap(), "two");
        cleanup(&d);
    }

    #[test]
    fn phase_separation_no_write_before_commit() {
        let d = tmp_dir("phase");
        let target = d.join("fresh.txt");
        assert!(!target.exists(), "le fichier ne doit pas exister avant commit");

        // Construire la transaction ne doit rien écrire.
        let _t = Transaction::new().add_op(PatchOperation {
            path: target.clone(),
            new_content: "hello".to_string(),
        });
        assert!(!target.exists(), "add_op ne doit pas écrire sur le disque");

        // Seul commit écrit.
        let outcome = Transaction::new()
            .add_op(PatchOperation {
                path: target.clone(),
                new_content: "hello".to_string(),
            })
            .commit();
        assert!(matches!(outcome, TransactionOutcome::Committed { .. }));
        assert!(target.exists());
        assert_eq!(fs::read_to_string(&target).unwrap(), "hello");
        cleanup(&d);
    }
}
