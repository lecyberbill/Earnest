//! Classification du risque d'une commande autorisee (contrat C8).
//!
//! Le parti pris est conservateur : toute commande non explicitement connue
//! comme NON MUTANTE est classee [`RiskLevel::Mutating`]. Une commande est
//! consideree non mutante si elle se contente de lire (fichiers, etat systeme,
//! historique Git) et n'ecrit ni fichier ni ressource distante.
//!
//! Sont donc mutantes : ecritures de fichiers (`touch`, `cp`, `mv`, `sed -i`,
//! redirections `>`), mutations de depot (`git commit`, `git branch -d`),
//! gestionnaires de paquets (`npm install`, `cargo build`), conteneurs
//! (`docker run`, `kubectl apply`) et tout binaire inconnu.

use super::arg_present;
use crate::normalize::Normalized;
use crate::segment::Segment;
use crate::RiskLevel;

/// Commandes dont l'execution n'ecrit rien sur le disque.
const READ_ONLY: &[&str] = &[
    "ls",
    "dir",
    "cat",
    "type",
    "head",
    "tail",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "ag",
    "ack",
    "wc",
    "sort",
    "uniq",
    "cut",
    "tr",
    "diff",
    "cmp",
    "comm",
    "join",
    "paste",
    "stat",
    "file",
    "du",
    "df",
    "free",
    "uname",
    "uptime",
    "date",
    "cal",
    "pwd",
    "echo",
    "printf",
    "print",
    "which",
    "whereis",
    "where",
    "whoami",
    "id",
    "groups",
    "hostname",
    "ps",
    "top",
    "pgrep",
    "pidof",
    "lsof",
    "netstat",
    "ss",
    "tree",
    "jq",
    "yq",
    "column",
    "nl",
    "tac",
    "rev",
    "fold",
    "expand",
    "strings",
    "xxd",
    "od",
    "hexdump",
    "md5sum",
    "sha1sum",
    "sha256sum",
    "sha512sum",
    "cksum",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "true",
    "false",
    "test",
    "[",
    "read",
    "ping",
    "get-content",
    "gc",
    "get-childitem",
    "gci",
    "get-item",
    "get-itemproperty",
    "get-process",
    "gps",
    "get-location",
    "gl",
    "get-command",
    "gcm",
    "get-help",
    "get-service",
    "get-date",
    "get-host",
    "test-path",
    "select-string",
    "sls",
    "measure-object",
    "write-output",
    "write-host",
    "out-string",
    "resolve-path",
    "split-path",
    "join-path",
    "convertto-json",
    "convertfrom-json",
    "select-object",
];

/// Sous-commandes Git qui n'ecrivent ni objet ni reference.
const GIT_READ_ONLY: &[&str] = &[
    "status",
    "log",
    "diff",
    "show",
    "rev-parse",
    "describe",
    "blame",
    "ls-files",
    "ls-tree",
    "shortlog",
    "reflog",
    "cat-file",
    "name-rev",
    "whatchanged",
    "count-objects",
    "grep",
    "help",
    "version",
    "var",
    "check-ignore",
    "show-ref",
    "merge-base",
    "diff-tree",
    "diff-files",
    "diff-index",
    "for-each-ref",
    "rev-list",
    "fsck",
    "verify-pack",
    "cherry",
    "show-branch",
];

/// Sous-commandes `docker`/`podman` en lecture seule.
const CONTAINER_READ_ONLY: &[&str] = &[
    "ps", "images", "inspect", "version", "info", "history", "logs", "top", "port", "diff",
    "search",
];

/// Sous-commandes `kubectl`/`oc` en lecture seule.
const KUBE_READ_ONLY: &[&str] = &[
    "get",
    "describe",
    "logs",
    "explain",
    "version",
    "api-resources",
    "api-versions",
    "cluster-info",
    "diff",
    "events",
];

/// Sous-commandes de gestionnaires JS en lecture seule.
const PKG_READ_ONLY: &[&str] = &["ls", "list", "ll", "la", "view", "info", "search", "outdated", "why", "root", "prefix", "bin", "doctor"];

/// Sous-commandes Cargo en lecture seule.
const CARGO_READ_ONLY: &[&str] = &["metadata", "tree", "search", "locate-project", "verify-project", "pkgid"];

/// Arguments positionnels, options et valeurs d'options retirees.
fn positionals<'a>(toks: &'a [String], with_value: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < toks.len() {
        let t = toks[i].as_str();
        if t == "--" {
            out.extend(toks[i + 1..].iter().map(|s| s.as_str()));
            break;
        }
        if with_value.contains(&t) {
            i += 2;
            continue;
        }
        if t.starts_with('-') && t != "-" {
            i += 1;
            continue;
        }
        out.push(t);
        i += 1;
    }
    out
}

/// Vrai si la ligne redirige la sortie vers un fichier (`>` / `>>`).
///
/// Les duplications de descripteur (`2>&1`, `>&2`) ne comptent pas comme des
/// ecritures. Le contenu des quotes est ignore.
fn writes_file(raw: &str) -> bool {
    let chars: Vec<char> = raw.chars().collect();
    let mut single = false;
    let mut double = false;
    let mut esc = false;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if esc {
            esc = false;
            i += 1;
            continue;
        }
        match c {
            '\\' if !single => esc = true,
            '`' if !single => esc = true,
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '>' if !single && !double => {
                if chars.get(i + 1) == Some(&'&') {
                    i += 2;
                    continue;
                }
                return true;
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Vrai si la commande ne fait qu'afficher une aide ou une version.
fn asks_help_or_version(args: &[String]) -> bool {
    args.iter().any(|a| {
        matches!(
            a.as_str(),
            "-h" | "--help" | "-help" | "/?" | "-?" | "help" | "--version" | "-version" | "/help"
        )
    })
}

fn classify_git(args: &[String]) -> RiskLevel {
    let pos = positionals(
        args,
        &[
            "-c",
            "-C",
            "--git-dir",
            "--work-tree",
            "--namespace",
            "--exec-path",
            "--config-env",
            "--super-prefix",
        ],
    );
    let sub = match pos.first() {
        Some(s) => *s,
        None => return RiskLevel::ReadOnly,
    };
    if GIT_READ_ONLY.contains(&sub) {
        return RiskLevel::ReadOnly;
    }
    match sub {
        "branch" => {
            let mutating = arg_present(
                args,
                &[
                    "-d",
                    "-m",
                    "-c",
                    "-f",
                    "-u",
                    "--delete",
                    "--move",
                    "--copy",
                    "--force",
                    "--set-upstream-to",
                    "--unset-upstream",
                    "--edit-description",
                ],
            );
            if mutating {
                RiskLevel::Mutating
            } else {
                RiskLevel::ReadOnly
            }
        }
        "tag" => {
            let mutating = arg_present(
                args,
                &["-a", "-d", "-f", "-s", "-m", "--delete", "--force", "--annotate", "--sign"],
            );
            if mutating {
                RiskLevel::Mutating
            } else {
                RiskLevel::ReadOnly
            }
        }
        "remote" => {
            let writes = pos
                .get(1)
                .map(|a| {
                    matches!(
                        *a,
                        "add" | "remove" | "rm" | "rename" | "set-url" | "prune" | "update" | "set-head"
                    )
                })
                .unwrap_or(false);
            if writes {
                RiskLevel::Mutating
            } else {
                RiskLevel::ReadOnly
            }
        }
        "stash" => {
            let reads = pos
                .get(1)
                .map(|a| matches!(*a, "list" | "show"))
                .unwrap_or(false);
            if reads {
                RiskLevel::ReadOnly
            } else {
                RiskLevel::Mutating
            }
        }
        "config" => {
            let reads = arg_present(args, &["--get", "--get-all", "--get-regexp", "-l", "--list"]);
            let writes = pos.len() >= 3 || (pos.len() == 2 && !reads);
            if reads || (pos.len() <= 1 && !writes) {
                RiskLevel::ReadOnly
            } else {
                RiskLevel::Mutating
            }
        }
        _ => RiskLevel::Mutating,
    }
}

fn first_positional(args: &[String], with_value: &[&str]) -> Option<String> {
    positionals(args, with_value).first().map(|s| s.to_string())
}

/// Niveau de risque du segment : lecture seule seulement si demontre.
pub fn classify(seg: &Segment, norm: &Normalized) -> RiskLevel {
    // Une redirection vers un fichier ecrit sur le disque.
    if writes_file(&seg.raw) {
        return RiskLevel::Mutating;
    }
    if norm.command.is_empty() {
        // Affectations d'environnement seules : aucun effet de bord.
        return RiskLevel::ReadOnly;
    }
    let cmd = norm.command.as_str();
    let args: Vec<String> = norm.args_lower();

    if asks_help_or_version(&args) {
        return RiskLevel::ReadOnly;
    }

    match cmd {
        "git" => return classify_git(&args),
        "sed" | "gsed" => {
            return if arg_present(&args, &["-i", "--in-place"]) {
                RiskLevel::Mutating
            } else {
                RiskLevel::ReadOnly
            };
        }
        "find" => {
            let destructive = args.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "-delete" | "-exec" | "-execdir" | "-ok" | "-okdir" | "-fprint" | "-fprintf" | "-fls"
                )
            });
            return if destructive {
                RiskLevel::Mutating
            } else {
                RiskLevel::ReadOnly
            };
        }
        "docker" | "podman" | "nerdctl" | "docker-compose" => {
            if let Some(sub) = first_positional(&args, &CONTAINER_VALUE_FLAGS) {
                if CONTAINER_READ_ONLY.contains(&sub.as_str()) {
                    return RiskLevel::ReadOnly;
                }
            }
            return RiskLevel::Mutating;
        }
        "kubectl" | "oc" => {
            if let Some(sub) = first_positional(&args, &KUBE_VALUE_FLAGS) {
                if KUBE_READ_ONLY.contains(&sub.as_str()) {
                    return RiskLevel::ReadOnly;
                }
            }
            return RiskLevel::Mutating;
        }
        "helm" => {
            if let Some(sub) = first_positional(&args, &KUBE_VALUE_FLAGS) {
                if matches!(sub.as_str(), "list" | "ls" | "status" | "show" | "get" | "history" | "version") {
                    return RiskLevel::ReadOnly;
                }
            }
            return RiskLevel::Mutating;
        }
        "npm" | "yarn" | "pnpm" | "bun" => {
            if let Some(sub) = first_positional(&args, &["--prefix", "-w", "--workspace", "--cwd"]) {
                if PKG_READ_ONLY.contains(&sub.as_str()) {
                    return RiskLevel::ReadOnly;
                }
            }
            return RiskLevel::Mutating;
        }
        "cargo" => {
            if let Some(sub) = first_positional(&args, &["--manifest-path", "-Z", "--config"]) {
                if CARGO_READ_ONLY.contains(&sub.as_str()) {
                    return RiskLevel::ReadOnly;
                }
            }
            return RiskLevel::Mutating;
        }
        _ => {}
    }

    if READ_ONLY.contains(&cmd) {
        RiskLevel::ReadOnly
    } else {
        RiskLevel::Mutating
    }
}

/// Options des clients conteneurs qui consomment une valeur.
const CONTAINER_VALUE_FLAGS: [&str; 12] = [
    "-h",
    "--host",
    "--context",
    "--config",
    "-c",
    "--log-level",
    "-n",
    "--namespace",
    "--kubeconfig",
    "-s",
    "--server",
    "--cluster",
];

/// Options des clients Kubernetes qui consomment une valeur.
const KUBE_VALUE_FLAGS: [&str; 10] = [
    "-n",
    "--namespace",
    "--context",
    "--cluster",
    "--kubeconfig",
    "--user",
    "--server",
    "-s",
    "--request-timeout",
    "--output",
];

#[cfg(test)]
mod tests {
    use crate::{inspect_command, RiskLevel, Verdict};

    fn risk(cmd: &str) -> RiskLevel {
        match inspect_command(cmd) {
            Verdict::Allowed { risk } => risk,
            other => panic!("attendu Allowed pour {cmd:?}, obtenu {other:?}"),
        }
    }

    #[test]
    fn test_classification_readonly_vs_mutating() {
        // --- lecture seule : consultation d'etat, aucune ecriture ---
        assert_eq!(risk("ls -la"), RiskLevel::ReadOnly);
        assert_eq!(risk("cat app.log"), RiskLevel::ReadOnly);
        assert_eq!(risk("head -n 20 app.log"), RiskLevel::ReadOnly);
        assert_eq!(risk("grep -rn todo src"), RiskLevel::ReadOnly);
        assert_eq!(risk("wc -l src/lib.rs"), RiskLevel::ReadOnly);
        assert_eq!(risk("diff a.txt b.txt"), RiskLevel::ReadOnly);
        assert_eq!(risk("echo bonjour"), RiskLevel::ReadOnly);
        assert_eq!(risk("ps aux"), RiskLevel::ReadOnly);
        assert_eq!(risk("df -h"), RiskLevel::ReadOnly);
        assert_eq!(risk("git status"), RiskLevel::ReadOnly);
        assert_eq!(risk("git log --oneline -5"), RiskLevel::ReadOnly);
        assert_eq!(risk("git diff HEAD~1"), RiskLevel::ReadOnly);
        assert_eq!(risk("docker ps"), RiskLevel::ReadOnly);
        assert_eq!(risk("kubectl get pods"), RiskLevel::ReadOnly);
        assert_eq!(risk("Get-Content app.log"), RiskLevel::ReadOnly);
        // pipeline de lectures : reste une lecture
        assert_eq!(risk("cat app.log | grep ERROR"), RiskLevel::ReadOnly);
        // aide / version : sortie immediate, aucun effet de bord
        assert_eq!(risk("ls --help"), RiskLevel::ReadOnly);
        assert_eq!(risk("git --version"), RiskLevel::ReadOnly);

        // --- mutation : ecriture fichier, depot, paquets, conteneurs ---
        assert_eq!(risk("rm -rf ./build"), RiskLevel::Mutating);
        assert_eq!(risk("mkdir -p ./out"), RiskLevel::Mutating);
        assert_eq!(risk("touch fichier.txt"), RiskLevel::Mutating);
        assert_eq!(risk("cp a b"), RiskLevel::Mutating);
        assert_eq!(risk("mv a b"), RiskLevel::Mutating);
        assert_eq!(risk("sed -i 's/a/b/' f.txt"), RiskLevel::Mutating);
        assert_eq!(risk("tar -czf out.tgz src"), RiskLevel::Mutating);
        assert_eq!(risk("chmod 755 ./run.sh"), RiskLevel::Mutating);
        assert_eq!(risk("git commit -m 'x'"), RiskLevel::Mutating);
        assert_eq!(risk("git branch -d vieille"), RiskLevel::Mutating);
        assert_eq!(risk("git push origin main"), RiskLevel::Mutating);
        assert_eq!(risk("npm install"), RiskLevel::Mutating);
        assert_eq!(risk("cargo build"), RiskLevel::Mutating);
        assert_eq!(risk("docker run --rm image"), RiskLevel::Mutating);
        assert_eq!(risk("kubectl apply -f k.yaml"), RiskLevel::Mutating);
        assert_eq!(risk("python app.py"), RiskLevel::Mutating);
        // redirection vers un fichier
        assert_eq!(risk("echo x > f.txt"), RiskLevel::Mutating);
        assert_eq!(risk("ls -la >> inventaire.txt"), RiskLevel::Mutating);
        // duplication de descripteur : pas une ecriture
        assert_eq!(risk("ls -la 2>&1"), RiskLevel::ReadOnly);
        assert_eq!(risk("cat app.log 1>&2"), RiskLevel::ReadOnly);

        // --- agregation : le risque maximal l'emporte ---
        assert_eq!(risk("ls; git status"), RiskLevel::ReadOnly);
        assert_eq!(risk("ls; mkdir -p out"), RiskLevel::Mutating);
        assert_eq!(risk("cat x | grep y; rm -rf ./build"), RiskLevel::Mutating);
        // commande vide : rien a faire
        assert_eq!(risk(""), RiskLevel::ReadOnly);
    }
}
