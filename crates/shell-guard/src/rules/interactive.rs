//! Detection statique des commandes qui attendent un humain (contrats C6 et C7).
//!
//! Quatre familles, toutes susceptibles de figer un agent non supervise :
//! 1. plein ecran : editeurs, pagineurs, moniteurs (attendent une frappe clavier) ;
//! 2. saisie bloquante : `pause`, `choice`, `set /p`, `Read-Host`, `read` sans
//!    entree pipee en amont (contrat C7) ;
//! 3. REPL : interpretes lances sans script ni `-c`/`-m`/`-e`, clients de base
//!    de donnees sans requete, shells nus, sous-commandes `git`/`npm` qui
//!    ouvrent un editeur ;
//! 4. processus non terminants : `tail -f`, `ping` sans compteur,
//!    `journalctl -f`, `docker logs -f`, TTY alloue (`docker run -it`).
//!
//! Chaque blocage porte une suggestion d'equivalence non interactive.

use super::arg_present;
use crate::normalize::Normalized;
use crate::segment::Segment;

/// Blocage pour cause d'interactivite : la commande attend un humain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interaction {
    /// Commande normalisee mise en cause.
    pub command: String,
    /// Pourquoi la commande ne rendrait pas la main.
    pub reason: String,
    /// Equivalent non interactif a utiliser a la place.
    pub suggestion: String,
}

fn hit(norm: &Normalized, reason: &str, suggestion: &str) -> Option<Interaction> {
    Some(Interaction {
        command: norm.command.clone(),
        reason: reason.to_string(),
        suggestion: suggestion.to_string(),
    })
}

/// Vrai si la commande ne fait qu'afficher une aide ou une version (sortie immediate).
fn asks_help_or_version(args: &[String]) -> bool {
    args.iter().any(|a| {
        matches!(
            a.as_str(),
            "-h" | "--help" | "-help" | "/?" | "-?" | "help" | "--version" | "-V"
        )
    })
}

/// Editeurs et assimilies (bloquent sur une frappe clavier).
const EDITORS: [&str; 20] = [
    "vim", "vi", "nvim", "nano", "emacs", "pico", "micro", "joe", "jed", "ed", "ex", "view",
    "vimdiff", "gvim", "visudo", "vimtutor", "hx", "helix", "kak", "tilde",
];

/// Pagineurs (`q` obligatoire pour rendre la main).
const PAGERS: [&str; 7] = ["less", "more", "most", "pg", "man", "info", "pinfo"];

/// Moniteurs plein ecran (boucle d'affichage infinie).
const MONITORS: [&str; 10] = [
    "top", "htop", "btop", "atop", "watch", "nmon", "glances", "iotop", "iftop", "nload",
];

/// Shells POSIX pouvant ouvrir un REPL nu.
const POSIX_SHELLS: [&str; 6] = ["bash", "sh", "dash", "zsh", "ksh", "fish"];

/// Arguments positionnels après suppression des options (et de leurs valeurs).
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

/// Vrai si un TTY est alloue (`-it`, `-ti`, `-i` + `-t`, `--interactive --tty`).
fn has_tty(args: &[String]) -> bool {
    let mut interactive = false;
    let mut tty = false;
    for a in args {
        if a == "--tty" || a == "-t" {
            tty = true;
        }
        if a == "--interactive" || a == "-i" {
            interactive = true;
        }
        if a.starts_with('-') && !a.starts_with("--") && a.len() > 2 {
            let flags: Vec<char> = a.chars().skip(1).collect();
            if flags.contains(&'i') && flags.contains(&'t') {
                return true;
            }
        }
    }
    interactive && tty
}

/// Analyse un segment normalise. Retourne le premier blocage interactif trouve.
pub fn check(seg: &Segment, norm: &Normalized) -> Option<Interaction> {
    if norm.command.is_empty() {
        return None;
    }
    let args: Vec<String> = norm.args_lower();
    let cmd = norm.command.as_str();

    // `--help` / `--version` sortent immediatement : jamais bloquants.
    if asks_help_or_version(&args) {
        return None;
    }
    let piped = seg.receives_piped_input();

    if EDITORS.contains(&cmd) {
        return hit(
            norm,
            "editeur de texte interactif : il attend la frappe au clavier et ne rend jamais la main",
            "lire avec `head -n 200`, `cat` ou `sed -n '1,200p'` ; modifier via une ecriture de fichier ou `patch`, jamais par un editeur plein ecran",
        );
    }
    if PAGERS.contains(&cmd) {
        return hit(
            norm,
            "pagineur interactif : il attend `q` pour rendre la main",
            "desactiver le pager (`git --no-pager ...`, `GIT_PAGER=cat PAGER=cat ...`) ou limiter la sortie (`head -n 200`)",
        );
    }
    if MONITORS.contains(&cmd) {
        // `top -b -n 1` produit un instantane puis se termine.
        if cmd == "top" && arg_present(&args, &["-b"]) {
            return None;
        }
        return hit(
            norm,
            "moniteur plein ecran : boucle d'affichage infinie jusqu'a une touche",
            "utiliser un instantane : `ps aux`, `df -h`, `top -b -n 1`",
        );
    }

    if let Some(i) = pauses(cmd, &args, piped, norm) {
        return Some(i);
    }
    if piped {
        // Une entree pipée alimente le programme : pas de saisie clavier attendue.
        return None;
    }
    if let Some(i) = repls(cmd, &args, norm) {
        return Some(i);
    }
    if let Some(i) = database_clients(cmd, &args, norm) {
        return Some(i);
    }
    if let Some(i) = remote_shells(cmd, &args, norm) {
        return Some(i);
    }
    if let Some(i) = embedded_shells(cmd, &args, norm) {
        return Some(i);
    }
    if let Some(i) = vcs(cmd, &args, norm) {
        return Some(i);
    }
    if let Some(i) = package_managers(cmd, &args, norm) {
        return Some(i);
    }
    if let Some(i) = containers(cmd, &args, norm) {
        return Some(i);
    }
    streaming(cmd, &args, norm)
}

/// Pauses et saisies : `pause`, `choice`, `set /p`, `Read-Host`, `read` (C7).
fn pauses(cmd: &str, args: &[String], piped: bool, norm: &Normalized) -> Option<Interaction> {
    match cmd {
        "pause" | "choice" => hit(
            norm,
            "pause interactive : attend une touche avant de poursuivre",
            "supprimer `pause`/`choice` ; pour patienter, `timeout /t 5` (cmd) ou `sleep 5` (POSIX)",
        ),
        "read-host" => hit(
            norm,
            "`Read-Host` demande une saisie clavier a l'operateur",
            "passer la valeur en parametre (`-Nom valeur`) ou lire `$env:NOM_VARIABLE`",
        ),
        "set" if args
            .first()
            .map(|a| a.starts_with("/p"))
            .unwrap_or(false) =>
        {
            hit(
                norm,
                "`set /p` demande une saisie clavier a l'operateur",
                "utiliser une variable d'environnement (`%NOM%`) ou un argument de ligne de commande",
            )
        }
        // Contrat C7 : `read` ne bloque que sans entree pipee en amont.
        "read" if !piped => hit(
            norm,
            "`read` attend une ligne sur l'entree standard, qui n'est pas alimentee par un pipe",
            "alimenter l'entree (`printf 'valeur\\n' | read -r var`) ou passer la valeur en argument",
        ),
        _ => None,
    }
}

/// REPL d'interpretes lances sans script ni expression.
fn repls(cmd: &str, args: &[String], norm: &Normalized) -> Option<Interaction> {
    let python = matches!(cmd, "python" | "python2" | "python3" | "py" | "ipython" | "ipython3");
    if python {
        if arg_present(args, &["-i"]) {
            return hit(
                norm,
                "invocation de l'interpreteur Python en mode interactif (`-i`)",
                "supprimer `-i` : `python chemin/script.py`, `python -c \"...\"` ou `python -m module`",
            );
        }
        if arg_present(args, &["-c", "--command", "-m", "-"]) {
            return None;
        }
        if !positionals(args, &["-w", "-x"]).is_empty() {
            return None;
        }
        return hit(
            norm,
            "interpreteur Python lance en REPL (aucun script, aucun `-c`/`-m`)",
            "utiliser `python -c \"print(1)\"`, `python chemin/script.py` ou `python -m pytest -q`",
        );
    }

    if matches!(cmd, "node" | "nodejs") {
        if arg_present(args, &["-i", "--interactive", "--inspect"]) {
            return hit(
                norm,
                "node lance en mode interactif (REPL ou inspecteur)",
                "utiliser `node chemin/script.js`, `node -e \"...\"`, `node --test` ou `node --inspect-brk=0`",
            );
        }
        if arg_present(args, &["-e", "--eval", "-p", "--print", "--test", "-"]) {
            return None;
        }
        if !positionals(args, &["-r", "--require", "--loader", "--import"]).is_empty() {
            return None;
        }
        return hit(
            norm,
            "node lance en REPL (aucun script, aucun `-e`/`--test`)",
            "utiliser `node chemin/script.js`, `node -e \"console.log(1)\"` ou `node --test`",
        );
    }

    if matches!(cmd, "irb" | "pry" | "ghci" | "erl" | "iex") {
        return hit(
            norm,
            "REPL de langage lance sans expression a evaluer",
            "utiliser la forme non interactive (`ruby -e \"...\"`, `ghc -e \"...\"`) ou un script",
        );
    }

    if matches!(cmd, "ruby") && positionals(args, &["-I", "-r"]).is_empty() && !arg_present(args, &["-e"]) {
        return hit(
            norm,
            "interpreteur Ruby lance en REPL (aucun script, aucun `-e`)",
            "utiliser `ruby -e \"puts 1\"` ou `ruby chemin/script.rb`",
        );
    }

    if matches!(cmd, "deno") && positionals(args, &[]).is_empty() {
        return hit(
            norm,
            "deno lance en REPL (aucun script, aucune sous-commande)",
            "utiliser `deno run chemin/script.ts` ou `deno eval \"...\"`",
        );
    }

    None
}

/// Clients de base de donnees sans requete fournie.
fn database_clients(cmd: &str, args: &[String], norm: &Normalized) -> Option<Interaction> {
    match cmd {
        "psql" => {
            if arg_present(args, &["-c", "--command", "-f", "--file", "-l", "--list"]) {
                return None;
            }
            hit(
                norm,
                "client `psql` sans requete : il ouvre une session interactive",
                "utiliser `psql -c 'select 1'` ou `psql -f requete.sql`",
            )
        }
        "mysql" | "mariadb" => {
            if arg_present(args, &["-e", "--execute"]) {
                return None;
            }
            hit(
                norm,
                "client MySQL sans requete : il ouvre une session interactive",
                "utiliser `mysql -e 'select 1'` ou `mysql < requete.sql`",
            )
        }
        "sqlite3" => {
            let pos = positionals(args, &["-cmd", "-init", "-separator", "-newline", "-nullvalue"]);
            if pos.len() >= 2 || arg_present(args, &["-cmd", "-init"]) {
                return None;
            }
            hit(
                norm,
                "shell `sqlite3` sans instruction SQL : il ouvre une session interactive",
                "utiliser `sqlite3 base.db 'select 1;'` ou `sqlite3 base.db < requete.sql`",
            )
        }
        "mongosh" | "mongo" => {
            if arg_present(args, &["--eval", "-e"]) {
                return None;
            }
            hit(
                norm,
                "shell MongoDB sans expression : il ouvre une session interactive",
                "utiliser `mongosh --eval 'db.version()'` ou un script `--file`",
            )
        }
        "redis-cli" => {
            let pos = positionals(
                args,
                &["-h", "-p", "-a", "-u", "-n", "-r", "-t", "--user", "--pass", "--tls"],
            );
            if !pos.is_empty() || arg_present(args, &["--eval"]) {
                return None;
            }
            hit(
                norm,
                "shell `redis-cli` sans commande : il ouvre une session interactive",
                "utiliser `redis-cli get cle` ou `redis-cli --eval script.lua`",
            )
        }
        "clickhouse-client" => {
            if arg_present(args, &["-q", "--query"]) {
                return None;
            }
            hit(
                norm,
                "client ClickHouse sans requete : session interactive",
                "utiliser `clickhouse-client -q 'select 1'`",
            )
        }
        _ => None,
    }
}

/// Shells distants sans commande a executer.
fn remote_shells(cmd: &str, args: &[String], norm: &Normalized) -> Option<Interaction> {
    match cmd {
        "ssh" | "mosh" => {
            let value_flags = [
                "-b", "-c", "-d", "-e", "-f", "-i", "-l", "-m", "-o", "-p", "-q", "-s", "-w", "-j",
                "-l", "--port", "--identity", "--ssh-option", "--server",
            ];
            // `-W`, `-L`, `-R`, `-D` prennent aussi une valeur.
            let pos = positionals(args, &value_flags);
            if pos.len() >= 2 {
                return None;
            }
            if arg_present(args, &["-v", "-n", "-n"]) && pos.is_empty() {
                return None;
            }
            hit(
                norm,
                "session shell distante sans commande : elle ouvre un shell interactif",
                "executer une commande distante : `ssh hote 'git -C /depot status'` (ajouter `-o BatchMode=yes`)",
            )
        }
        "telnet" | "ftp" | "sftp" | "nc" | "ncat" | "netcat" => hit(
            norm,
            "session reseau interactive (protocole pilote au clavier)",
            "utiliser un client non interactif (`curl`, `scp`, `rsync`) ou une commande unique",
        ),
        _ => None,
    }
}

/// Shells embarques nus : `bash`, `sh`, `pwsh`, `cmd` sans script ni `-c`.
fn embedded_shells(cmd: &str, args: &[String], norm: &Normalized) -> Option<Interaction> {
    if POSIX_SHELLS.contains(&cmd) {
        if arg_present(args, &["-c"]) {
            return None;
        }
        if args
            .iter()
            .any(|a| a.starts_with('-') && !a.starts_with("--") && a.contains('c'))
        {
            return None;
        }
        if !positionals(args, &["-o", "+o", "-O", "--rcfile"]).is_empty() {
            return None;
        }
        return hit(
            norm,
            "shell POSIX lance sans script ni `-c` : il ouvre un interpreteur interactif",
            "passer la commande : `bash -c 'ls -la'` ou `bash chemin/script.sh`",
        );
    }

    if matches!(cmd, "pwsh" | "powershell") {
        if arg_present(
            args,
            &["-command", "-c", "--command", "-encodedcommand", "-ec", "-file", "-f"],
        ) {
            return None;
        }
        return hit(
            norm,
            "PowerShell lance sans `-Command`/`-File` : il ouvre un interpreteur interactif",
            "passer le script : `pwsh -NoProfile -Command \"Get-ChildItem\"`",
        );
    }

    if cmd == "cmd" {
        if arg_present(args, &["/c"]) {
            return None;
        }
        if arg_present(args, &["/k"]) {
            return hit(
                norm,
                "`cmd /k` execute puis laisse la console ouverte (attente infinie)",
                "utiliser `cmd /c ...` pour un execution unique et non interactive",
            );
        }
        return hit(
            norm,
            "`cmd` lance sans `/c` : il ouvre une console interactive",
            "utiliser `cmd /c dir` (execution unique)",
        );
    }

    None
}

/// Sous-commandes Git qui ouvrent un editeur de message.
fn vcs(cmd: &str, args: &[String], norm: &Normalized) -> Option<Interaction> {
    if cmd != "git" {
        return None;
    }
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
        None => return None,
    };
    let message_flags = ["-m", "--message", "-f", "--file", "--no-edit", "--no-interactive"];

    match sub {
        "commit" => {
            if arg_present(args, &message_flags) || arg_present(args, &["--amend"]) {
                return None;
            }
            hit(
                norm,
                "`git commit` sans message : Git ouvre un editeur de texte et bloque",
                "fournir le message en ligne : `git commit -m \"type(scope): description\"`",
            )
        }
        "merge" => {
            if arg_present(
                args,
                &[
                    "-m",
                    "--message",
                    "-f",
                    "--file",
                    "--no-edit",
                    "--ff-only",
                    "--abort",
                    "--continue",
                    "--quit",
                    "--squash",
                    "--no-commit",
                ],
            ) {
                return None;
            }
            hit(
                norm,
                "`git merge` peut ouvrir un editeur pour le message de fusion",
                "ajouter `--no-edit` (ou `-m \"message\"`) pour une fusion non interactive",
            )
        }
        "revert" => {
            if arg_present(
                args,
                &["-m", "--message", "--no-edit", "--no-commit", "--abort", "--continue", "--skip"],
            ) {
                return None;
            }
            hit(
                norm,
                "`git revert` ouvre un editeur pour le message de commit",
                "ajouter `--no-edit` (ou `-m \"message\"`)",
            )
        }
        "rebase" => {
            if arg_present(args, &["-i", "--interactive", "--edit-todo"]) {
                return hit(
                    norm,
                    "`git rebase -i` ouvre l'editeur de todo-list (interaction obligatoire)",
                    "utiliser un rebase non interactif : `git rebase --onto cible base` ou `git cherry-pick`",
                );
            }
            None
        }
        "add" => {
            if arg_present(args, &["-p", "--patch", "-i", "--interactive", "-e", "--edit"]) {
                return hit(
                    norm,
                    "`git add` en mode interactif (selection ou edition de hunks)",
                    "indexer explicitement : `git add chemin/fichier.rs`",
                );
            }
            None
        }
        "clean" => {
            if arg_present(args, &["-i", "--interactive"]) {
                return hit(
                    norm,
                    "`git clean -i` propose une selection interactive",
                    "utiliser `git clean -fdx chemin/` sur un chemin explicite",
                );
            }
            None
        }
        "checkout" | "restore" | "switch" | "stash" => {
            if arg_present(args, &["-p", "--patch"]) {
                return hit(
                    norm,
                    "mode interactif de selection de hunks (`-p`/`--patch`)",
                    "restaurer explicitement : `git restore chemin/fichier.rs`",
                );
            }
            None
        }
        "config" => {
            if arg_present(args, &["-e", "--edit"]) {
                return hit(
                    norm,
                    "`git config --edit` ouvre un editeur",
                    "ecrire la cle directement : `git config --global cle valeur`",
                );
            }
            None
        }
        "tag" => {
            if arg_present(args, &["-a", "--annotate", "-s", "--sign"])
                && !arg_present(args, &["-m", "--message", "-f", "--file"])
            {
                return hit(
                    norm,
                    "`git tag` annoté sans `-m` ouvre un editeur",
                    "fournir le message : `git tag -a v1.0 -m \"version 1.0\"`",
                );
            }
            None
        }
        "difftool" | "mergetool" | "send-email" | "gui" | "citool" => hit(
            norm,
            "outil Git graphique ou interactif",
            "utiliser les equivalents texte : `git diff`, `git log --stat`",
        ),
        _ => None,
    }
}

/// Assistants de creation de projet et authentifications interactives.
fn package_managers(cmd: &str, args: &[String], norm: &Normalized) -> Option<Interaction> {
    if matches!(cmd, "npm" | "yarn" | "pnpm" | "bun") {
        let pos = positionals(args, &["--prefix", "-w", "--workspace", "--cwd"]);
        match pos.first().copied() {
            Some("init") | Some("create") => {
                if arg_present(args, &["-y", "--yes", "--init-only"]) || pos.len() > 1 {
                    return None;
                }
                hit(
                    norm,
                    "assistant de creation de projet : questions interactives",
                    "utiliser `npm init -y` puis editer `package.json` explicitement",
                )
            }
            Some("login") | Some("adduser") => hit(
                norm,
                "authentification interactive au registre (saisie du mot de passe)",
                "configurer un token : `npm config set //registre/:_authToken=$TOKEN`",
            ),
            _ => None,
        }
    } else if cmd == "cargo" {
        let pos = positionals(args, &["--manifest-path", "-Z", "--config"]);
        if pos.first().copied() == Some("login") {
            if arg_present(args, &["--token", "--registry-token"]) {
                return None;
            }
            return hit(
                norm,
                "`cargo login` demande un token au clavier",
                "utiliser `cargo login --token $CARGO_TOKEN`",
            );
        }
        None
    } else {
        None
    }
}

/// Conteneurs et orchestrateurs : TTY, attachement, flux suivis.
fn containers(cmd: &str, args: &[String], norm: &Normalized) -> Option<Interaction> {
    let is_container = matches!(cmd, "docker" | "podman" | "nerdctl" | "docker-compose");
    let is_kube = matches!(cmd, "kubectl" | "oc" | "helm");
    if !is_container && !is_kube {
        return None;
    }
    let pos = positionals(
        args,
        &[
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
            "--user",
            "--request-timeout",
        ],
    );
    let sub = match pos.first() {
        Some(s) => *s,
        None => return None,
    };

    if has_tty(args) {
        return hit(
            norm,
            "allocation d'un TTY interactif (`-it`) : la commande attend le clavier",
            "retirer `-it` (garder la sortie standard) : `docker run --rm image commande`",
        );
    }
    if matches!(sub, "attach" | "edit") {
        return hit(
            norm,
            "session attachante ou edition interactive dans le cluster",
            "lire l'etat (`logs`, `describe`, `get -o yaml`) au lieu d'attacher un terminal",
        );
    }
    if matches!(sub, "login") && !arg_present(args, &["-p", "--password", "--password-stdin"]) {
        return hit(
            norm,
            "authentification interactive au registre",
            "utiliser `docker login --password-stdin` avec le mot de passe sur stdin",
        );
    }
    if arg_present(args, &["-f", "--follow"]) && matches!(sub, "logs" | "events" | "attach") {
        return hit(
            norm,
            "flux de journaux suivi indefiniment (`-f`/`--follow`)",
            "retirer `--follow` et limiter : `logs --tail 200`",
        );
    }
    None
}

/// Processus qui ne se terminent pas seuls.
fn streaming(cmd: &str, args: &[String], norm: &Normalized) -> Option<Interaction> {
    if cmd == "tail" && arg_present(args, &["-f", "--follow", "-f"]) {
        return hit(
            norm,
            "`tail -f` suit le fichier indefiniment et ne rend jamais la main",
            "lire un instantane : `tail -n 200 fichier.log`",
        );
    }
    if cmd == "journalctl" && arg_present(args, &["-f", "--follow"]) {
        return hit(
            norm,
            "`journalctl -f` suit le journal indefiniment",
            "utiliser `journalctl -n 200 --no-pager`",
        );
    }
    if cmd == "ping" && !arg_present(args, &["-c", "--count"]) {
        return hit(
            norm,
            "`ping` sans compteur emet des requetes sans fin",
            "limiter le nombre de paquets : `ping -c 4 hote` (POSIX) ou `ping -n 4 hote` (Windows)",
        );
    }
    if matches!(cmd, "watch") {
        return hit(
            norm,
            "`watch` re-execute la commande indefiniment",
            "executer la commande une seule fois, ou en boucle bornee dans un script",
        );
    }
    if cmd == "su" && !arg_present(args, &["-c", "--command", "-s", "--shell"]) {
        return hit(
            norm,
            "`su` sans `-c` ouvre un shell interactif et demande un mot de passe",
            "executer une commande unique : `su -c 'commande'` (ou `sudo commande`)",
        );
    }
    if cmd == "passwd" {
        return hit(
            norm,
            "`passwd` demande l'ancien et le nouveau mot de passe au clavier",
            "utiliser `chpasswd` avec une entree pipée, ou une gestion de secrets dediee",
        );
    }
    if cmd == "crontab" && arg_present(args, &["-e"]) {
        return hit(
            norm,
            "`crontab -e` ouvre un editeur de crontab",
            "ecrire le fichier puis `crontab fichier.cron`",
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::{inspect_command, RiskLevel, Verdict};

    fn blocked(cmd: &str) -> (String, String, String) {
        match inspect_command(cmd) {
            Verdict::InteractiveBlocked {
                command,
                reason,
                suggestion,
            } => (command, reason, suggestion),
            other => panic!("attendu InteractiveBlocked pour {cmd:?}, obtenu {other:?}"),
        }
    }

    fn allowed(cmd: &str) -> RiskLevel {
        match inspect_command(cmd) {
            Verdict::Allowed { risk } => risk,
            other => panic!("attendu Allowed pour {cmd:?}, obtenu {other:?}"),
        }
    }

    #[test]
    fn test_commandes_interactives_bloquees_avec_suggestion() {
        // --- editeurs et pagineurs ---
        assert_eq!(blocked("vim notes.txt").0, "vim");
        assert_eq!(blocked("vi app.py").0, "vi");
        assert_eq!(blocked("nvim .").0, "nvim");
        assert_eq!(blocked("nano /etc/hosts").0, "nano");
        assert_eq!(blocked("emacs -nw fichier").0, "emacs");
        assert_eq!(blocked("less README.md").0, "less");
        assert_eq!(blocked("more trace.log").0, "more");
        assert_eq!(blocked("man ls").0, "man");
        // `sudo` est traverse par la normalisation
        assert_eq!(blocked("sudo vim /etc/hosts").0, "vim");

        // --- saisies ---
        assert_eq!(blocked("pause").0, "pause");
        assert_eq!(blocked("Read-Host -Prompt 'Nom'").0, "read-host");
        let (_, reason, suggestion) = blocked("read -p 'Nom ?' nom");
        assert!(!reason.is_empty());
        assert!(suggestion.to_lowercase().contains("pipe") || suggestion.contains("printf"));

        // --- REPL d'interpretes ---
        assert_eq!(blocked("python").0, "python");
        assert_eq!(blocked("python3").0, "python3");
        assert_eq!(blocked("py -3").0, "py");
        assert_eq!(blocked("python -i app.py").0, "python");
        assert_eq!(blocked("ipython").0, "ipython");
        assert_eq!(blocked("node").0, "node");
        assert_eq!(blocked("irb").0, "irb");

        // --- clients de base de donnees ---
        assert_eq!(blocked("psql").0, "psql");
        assert_eq!(blocked("psql -U postgres").0, "psql");
        assert_eq!(blocked("sqlite3 ma.db").0, "sqlite3");
        assert_eq!(blocked("mysql -u root -p").0, "mysql");

        // --- shells nus ---
        assert_eq!(blocked("bash").0, "bash");
        assert_eq!(blocked("powershell -NoProfile").0, "powershell");
        assert_eq!(blocked("cmd").0, "cmd");
        assert_eq!(blocked("ssh hote.exemple").0, "ssh");

        // --- outils CLI a mode non interactif ---
        let (command, _, suggestion) = blocked("git commit");
        assert_eq!(command, "git");
        assert!(suggestion.contains("-m"), "{suggestion}");
        assert_eq!(blocked("git commit -a").0, "git");
        assert_eq!(blocked("git rebase -i HEAD~2").0, "git");
        assert_eq!(blocked("git add -p").0, "git");
        assert_eq!(blocked("npm init").0, "npm");
        assert_eq!(blocked("docker run -it ubuntu bash").0, "docker");
        assert_eq!(blocked("kubectl edit deployment api").0, "kubectl");

        // --- processus non terminants ---
        assert_eq!(blocked("tail -f app.log").0, "tail");
        assert_eq!(blocked("journalctl -f -u nginx").0, "journalctl");
        assert_eq!(blocked("ping exemple.invalid").0, "ping");
        assert_eq!(blocked("watch -n 1 ls").0, "watch");
        assert_eq!(blocked("su - postgres").0, "su");
        assert_eq!(blocked("passwd").0, "passwd");

        // --- equivalents non interactifs : jamais bloques ---
        assert_eq!(allowed("python -c 'print(1)'"), RiskLevel::Mutating);
        assert_eq!(allowed("python app.py"), RiskLevel::Mutating);
        assert_eq!(allowed("python -m pytest -q"), RiskLevel::Mutating);
        assert_eq!(allowed("python -W ignore app.py"), RiskLevel::Mutating);
        assert_eq!(allowed("node app.js"), RiskLevel::Mutating);
        assert_eq!(allowed("node -e 'console.log(1)'"), RiskLevel::Mutating);
        assert_eq!(allowed("node --test"), RiskLevel::Mutating);
        assert_eq!(allowed("psql -c 'select 1'"), RiskLevel::Mutating);
        assert_eq!(allowed("sqlite3 app.db 'select 1'"), RiskLevel::Mutating);
        assert_eq!(allowed("git commit -m 'fix: bug'"), RiskLevel::Mutating);
        assert_eq!(allowed("git commit --no-edit --amend"), RiskLevel::Mutating);
        assert_eq!(allowed("git status"), RiskLevel::ReadOnly);
        assert_eq!(allowed("bash -c 'ls -la'"), RiskLevel::ReadOnly);
        assert_eq!(allowed("vim --version"), RiskLevel::ReadOnly);
        assert_eq!(allowed("ls --help"), RiskLevel::ReadOnly);
        assert_eq!(allowed("npm test"), RiskLevel::Mutating);
        assert_eq!(allowed("tail -n 200 app.log"), RiskLevel::ReadOnly);
        assert_eq!(allowed("top -b -n 1"), RiskLevel::ReadOnly);
        assert_eq!(allowed("docker ps"), RiskLevel::ReadOnly);
        assert_eq!(allowed("docker logs --tail 200 api"), RiskLevel::ReadOnly);
    }

    #[test]
    fn test_read_sans_entree_pipee_uniquement() {
        // Contrat C7 : `read` est bloquant seulement sans pipe en amont.
        match inspect_command("read -p 'Nom ?' nom") {
            Verdict::InteractiveBlocked { command, .. } => assert_eq!(command, "read"),
            other => panic!("attendu InteractiveBlocked, obtenu {other:?}"),
        }
        match inspect_command("read -r ligne") {
            Verdict::InteractiveBlocked { command, .. } => assert_eq!(command, "read"),
            other => panic!("attendu InteractiveBlocked, obtenu {other:?}"),
        }

        // Avec entree pipee : autorise (et non mutant).
        assert_eq!(allowed("echo du texte | read ligne"), RiskLevel::ReadOnly);
        assert_eq!(allowed("cat data.txt | read -r l"), RiskLevel::ReadOnly);
        assert_eq!(allowed("printf 'x\\n' | read -r v"), RiskLevel::ReadOnly);

        // `Read-Host` reste bloquant meme si un pipe existe : il lit le clavier.
        match inspect_command("Get-Content x.txt | Read-Host") {
            Verdict::InteractiveBlocked { command, .. } => assert_eq!(command, "read-host"),
            other => panic!("attendu InteractiveBlocked, obtenu {other:?}"),
        }
        match inspect_command("pause") {
            Verdict::InteractiveBlocked { command, .. } => assert_eq!(command, "pause"),
            other => panic!("attendu InteractiveBlocked, obtenu {other:?}"),
        }
    }
}
