//! Normalisation d'un segment de commande.
//!
//! Trois responsabilites :
//! 1. `tokenize` : decoupage en jetons conscient des quotes (`'...'`, `"..."`),
//!    des echappements POSIX `\` et PowerShell `` ` ``, et des sous-commandes
//!    `$( ... )` conservees comme un jeton unique.
//! 2. suppression des prefixes d'invocation : `sudo`, `doas`, `env`, `command`,
//!    `exec`, `nohup`, `time`, `nice`, `timeout`, `stdbuf`, `busybox`, `call`.
//! 3. traversee des shells embarques : `bash -c ...`, `sh -c ...`,
//!    `powershell -Command ...`, `pwsh -c ...`, `cmd /c ...` ; la commande
//!    reellement analysee devient alors la commande interne.

use crate::segment::{matching_paren, Segment};

/// Vue normalisee d'un segment de commande.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Normalized {
    /// Nom de commande en minuscules, sans chemin ni extension Windows.
    pub command: String,
    /// Nom de commande tel qu'ecrit (sans les prefixes traverses).
    pub raw_command: String,
    /// Arguments (quotes retirees).
    pub args: Vec<String>,
    /// Affectations d'environnement de tete (`FOO=bar`).
    pub assignments: Vec<String>,
    /// Prefixes / emballages traverses, dans l'ordre.
    pub wrappers: Vec<String>,
    /// Vrai si la commande est invoquee via un shell Windows (cmd / PowerShell).
    pub windows_shell: bool,
}

impl Normalized {
    /// Vrai si aucune commande n'a pu etre identifiee.
    pub fn is_empty(&self) -> bool {
        self.command.is_empty()
    }

    /// Arguments en minuscules (comparaisons insensibles a la casse).
    pub fn args_lower(&self) -> Vec<String> {
        self.args.iter().map(|a| a.to_ascii_lowercase()).collect()
    }

    /// Vrai si l'argument exact est present (insensible a la casse).
    pub fn has_arg(&self, needle: &str) -> bool {
        self.args.iter().any(|a| a.eq_ignore_ascii_case(needle))
    }

    /// Vrai si au moins un des arguments exacts est present.
    pub fn has_any_arg(&self, needles: &[&str]) -> bool {
        needles.iter().any(|n| self.has_arg(n))
    }

    /// Arguments qui ne sont pas des options (`-x`, `--y`).
    pub fn non_flag_args(&self) -> Vec<&str> {
        self.args
            .iter()
            .filter(|a| !a.starts_with('-'))
            .map(|a| a.as_str())
            .collect()
    }
}

/// Normalise directement un segment.
pub fn normalize_segment(seg: &Segment) -> Normalized {
    normalize(&seg.raw)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokState {
    Normal,
    Single,
    Double,
    Escape,
}

/// Decoupe un texte en jetons en respectant quotes, echappements et `$( ... )`.
pub fn tokenize(raw: &str) -> Vec<String> {
    let chars: Vec<char> = raw.chars().collect();
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut state = TokState::Normal;
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        match state {
            TokState::Escape => {
                cur.push(c);
                started = true;
                state = TokState::Normal;
                i += 1;
            }
            TokState::Single => {
                started = true;
                if c == '\'' {
                    state = TokState::Normal;
                } else {
                    cur.push(c);
                }
                i += 1;
            }
            TokState::Double => match c {
                '"' => {
                    started = true;
                    state = TokState::Normal;
                    i += 1;
                }
                '\\' if !looks_like_windows_path(&cur) => {
                    started = true;
                    state = TokState::Escape;
                    i += 1;
                }
                '\\' => {
                    // Antislash interne d'un chemin Windows : litteral.
                    cur.push(c);
                    started = true;
                    i += 1;
                }
                '`' => {
                    started = true;
                    state = TokState::Escape;
                    i += 1;
                }
                _ => {
                    cur.push(c);
                    started = true;
                    i += 1;
                }
            },
            TokState::Normal => match c {
                '\\' if !looks_like_windows_path(&cur) => {
                    started = true;
                    state = TokState::Escape;
                    i += 1;
                }
                '\\' => {
                    // Antislash interne d'un chemin Windows : litteral.
                    cur.push(c);
                    started = true;
                    i += 1;
                }
                '`' => {
                    started = true;
                    state = TokState::Escape;
                    i += 1;
                }
                '\'' => {
                    started = true;
                    state = TokState::Single;
                    i += 1;
                }
                '"' => {
                    started = true;
                    state = TokState::Double;
                    i += 1;
                }
                '$' if chars.get(i + 1) == Some(&'(') => {
                    let end = matching_paren(&chars, i).unwrap_or(i + 1);
                    for k in i..=end {
                        if let Some(ch) = chars.get(k) {
                            cur.push(*ch);
                        }
                    }
                    started = true;
                    i = end + 1;
                }
                c if c.is_whitespace() => {
                    if started || !cur.is_empty() {
                        tokens.push(std::mem::take(&mut cur));
                        started = false;
                    }
                    i += 1;
                }
                _ => {
                    cur.push(c);
                    started = true;
                    i += 1;
                }
            },
        }
    }

    if started || !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

/// Dernier composant d'un chemin POSIX ou Windows.
pub fn basename(path: &str) -> &str {
    let mut start = 0usize;
    for (idx, c) in path.char_indices() {
        if c == '/' || c == '\\' {
            start = idx + c.len_utf8();
        }
    }
    &path[start..]
}

fn strip_windows_ext(name: &str) -> &str {
    // Comparaison insensible a la casse : `NODE.EXE`, `node.Exe`, `node.exe`.
    for ext in [".exe", ".cmd", ".bat", ".com", ".ps1"] {
        if name.len() > ext.len() {
            let (stem, suffix) = name.split_at(name.len() - ext.len());
            if suffix.eq_ignore_ascii_case(ext) && !stem.is_empty() {
                return stem;
            }
        }
    }
    name
}

/// Vrai si le jeton en construction est deja un chemin Windows, auquel cas
/// l'antislash est un separateur de chemin et non un caractere d'echappement.
///
/// Couvre `C:\...`, `.\relatif`, `..\parent` et les chemins deja separes.
fn looks_like_windows_path(cur: &str) -> bool {
    if cur.contains('\\') {
        return true;
    }
    let bytes = cur.as_bytes();
    // Lettre de lecteur : `C:`.
    if bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return true;
    }
    // Chemins relatifs explicites : `.` et `..`.
    cur == "." || cur == ".."
}

/// Nom de commande analyse : minuscules, sans chemin, sans extension Windows.
pub fn command_name(token: &str) -> String {
    strip_windows_ext(basename(token))
        .to_ascii_lowercase()
        .trim()
        .to_string()
}

fn is_assignment(tok: &str) -> bool {
    let mut chars = tok.char_indices();
    match chars.next() {
        Some((_, c)) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    for (idx, c) in chars {
        if c == '=' {
            return idx > 0;
        }
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
    }
    false
}

fn is_windows_shell(name: &str) -> bool {
    matches!(name, "powershell" | "pwsh" | "cmd")
}

fn is_posix_shell(name: &str) -> bool {
    matches!(name, "bash" | "sh" | "dash" | "zsh" | "ksh" | "fish")
}

/// Normalise un texte de commande complet.
pub fn normalize(raw: &str) -> Normalized {
    normalize_tokens(tokenize(raw), 0)
}

/// Vrai si le jeton est une redirection de flux (`>`, `>>`, `<`, `2>&1`, `&>f`).
fn is_redirection_token(tok: &str) -> bool {
    let rest = tok.trim_start_matches(|c: char| c.is_ascii_digit());
    if rest.is_empty() {
        return false;
    }
    if rest.starts_with("&>") || rest.starts_with("&<") {
        return true;
    }
    let rest = rest.strip_prefix('&').unwrap_or(rest);
    rest.starts_with('>') || rest.starts_with('<')
}

/// Retire les redirections et leur cible des arguments : `cmd a > log 2>&1`
/// devient `cmd a`. Le contenu quote est deja agglomere par `tokenize`.
fn cut_redirections(mut tokens: Vec<String>) -> Vec<String> {
    if let Some(pos) = tokens.iter().position(|t| is_redirection_token(t)) {
        tokens.truncate(pos);
    }
    tokens
}

fn normalize_tokens(tokens: Vec<String>, depth: usize) -> Normalized {
    let mut tokens = cut_redirections(tokens);
    let mut out = Normalized::default();
    let mut guard = 0usize;

    loop {
        guard += 1;
        if guard > 32 {
            break;
        }

        // 1. affectations d'environnement de tete
        while let Some(tok) = tokens.first() {
            if is_assignment(tok) {
                out.assignments.push(tokens.remove(0));
            } else {
                break;
            }
        }

        // 2. commande restante
        let head = match tokens.first() {
            Some(h) => h.clone(),
            None => break,
        };
        let name = command_name(&head);

        // 3. shell embarque : `bash -c ...`, `powershell -Command ...`
        if let Some(inner) = embedded_command(&name, &tokens) {
            out.wrappers.push(name.clone());
            if is_windows_shell(&name) {
                out.windows_shell = true;
            }
            if depth < 4 {
                let nested = normalize_tokens(tokenize(&inner), depth + 1);
                if !nested.command.is_empty() {
                    out.command = nested.command;
                    out.raw_command = nested.raw_command;
                    out.args = nested.args;
                    out.windows_shell = out.windows_shell || nested.windows_shell;
                    out.assignments.extend(nested.assignments);
                    out.wrappers.extend(nested.wrappers);
                    return out;
                }
            }
            out.command = name;
            out.raw_command = head;
            out.args = tokens[1..].to_vec();
            return out;
        }

        // 4. prefixe simple : on retire et on recommence
        if let Some(skip) = simple_prefix_skip(&name, &tokens) {
            let skip = skip.min(tokens.len());
            if skip == 0 {
                break;
            }
            out.wrappers.push(name);
            tokens.drain(0..skip);
            continue;
        }

        out.command = name;
        out.raw_command = head;
        out.args = tokens[1..].to_vec();
        break;
    }

    if out.command.is_empty() && !out.assignments.is_empty() {
        out.raw_command = out.assignments.join(" ");
    }
    out
}

fn join_tokens(toks: &[String]) -> String {
    toks.join(" ")
}

fn embedded_command(name: &str, toks: &[String]) -> Option<String> {
    if is_posix_shell(name) {
        let mut i = 1usize;
        while i < toks.len() {
            let t = toks[i].as_str();
            if t == "--" {
                return None;
            }
            if t == "-c" || (t.starts_with('-') && !t.starts_with("--") && t.contains('c')) {
                return Some(join_tokens(&toks[i + 1..]));
            }
            if t.starts_with('-') {
                i += if matches!(t, "-o" | "+o" | "-O" | "--rcfile") {
                    2
                } else {
                    1
                };
                continue;
            }
            return None;
        }
        return None;
    }

    if is_windows_shell(name) {
        if name == "cmd" {
            for (idx, tok) in toks.iter().enumerate().skip(1) {
                let l = tok.to_ascii_lowercase();
                if l == "/c" {
                    return Some(join_tokens(&toks[idx + 1..]));
                }
                if l == "/k" {
                    // /k laisse la console ouverte : reste interactif
                    return None;
                }
            }
            return None;
        }

        let mut i = 1usize;
        while i < toks.len() {
            let t = toks[i].to_ascii_lowercase();
            match t.as_str() {
                "-command" | "--command" | "-c" => return Some(join_tokens(&toks[i + 1..])),
                "-encodedcommand" | "-encodedarguments" | "-ec" | "-e" => return Some(String::new()),
                "-file" | "-f" => return None,
                "-executionpolicy" | "-ep" | "-windowstyle" | "-inputformat" | "-outputformat"
                | "-configurationname" | "-workingdirectory" | "-version" => {
                    i += 2;
                    continue;
                }
                other if other.starts_with('-') => {
                    i += 1;
                    continue;
                }
                _ => return None,
            }
        }
        return None;
    }

    None
}

fn skip_flags(toks: &[String], start: usize, with_value: &[&str]) -> usize {
    let mut i = start;
    while i < toks.len() {
        let t = toks[i].as_str();
        if t == "--" {
            return i + 1;
        }
        if with_value.contains(&t) {
            i += if i + 1 < toks.len() { 2 } else { 1 };
            continue;
        }
        if t.starts_with('-') && t != "-" {
            i += 1;
            continue;
        }
        break;
    }
    i
}

const SUDO_VALUE_FLAGS: [&str; 18] = [
    "-u",
    "-g",
    "-p",
    "-C",
    "-U",
    "-r",
    "-t",
    "-D",
    "-T",
    "--user",
    "--group",
    "--prompt",
    "--chdir",
    "--host",
    "--role",
    "--type",
    "--other-user",
    "--close-from",
];

fn simple_prefix_skip(name: &str, toks: &[String]) -> Option<usize> {
    let skip = match name {
        "sudo" | "doas" => skip_flags(toks, 1, &SUDO_VALUE_FLAGS),
        "env" => skip_flags(
            toks,
            1,
            &["-u", "--unset", "-C", "--chdir", "-S", "--split-string"],
        ),
        "command" | "builtin" | "exec" | "nohup" | "setsid" | "unbuffer" | "busybox" | "call" => {
            skip_flags(toks, 1, &[])
        }
        "time" => skip_flags(toks, 1, &["-f", "--format", "-o", "--output"]),
        "nice" => skip_flags(toks, 1, &["-n", "--adjustment"]),
        "stdbuf" => skip_flags(toks, 1, &["-i", "-o", "-e"]),
        "timeout" => {
            let after_flags = skip_flags(toks, 1, &["-s", "-k", "--signal", "--kill-after"]);
            if after_flags < toks.len() {
                after_flags + 1
            } else {
                after_flags
            }
        }
        _ => return None,
    };
    if skip == 0 {
        return None;
    }
    Some(skip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalisation_nom_commande_prefixes() {
        // --- tokenisation consciente des quotes et echappements ---
        assert_eq!(
            tokenize("ls -la \"a b\" 'c d'"),
            vec!["ls", "-la", "a b", "c d"]
        );
        assert_eq!(tokenize("echo a\\ b"), vec!["echo", "a b"]);
        assert_eq!(tokenize("echo ''"), vec!["echo", ""]);
        assert_eq!(tokenize("rmdir \"a'b\""), vec!["rmdir", "a'b"]);
        assert_eq!(tokenize("Write-Host a`;b"), vec!["Write-Host", "a;b"]);
        // sous-commande conservee en un seul jeton
        assert_eq!(
            tokenize("echo $(ls -la) suffix"),
            vec!["echo", "$(ls -la)", "suffix"]
        );
        assert!(tokenize("   ").is_empty());

        // --- nom de commande : chemin + extension Windows ---
        assert_eq!(command_name("ls"), "ls");
        assert_eq!(command_name("/usr/bin/grep"), "grep");
        assert_eq!(command_name("C:\\Tools\\Node.EXE"), "node");
        assert_eq!(command_name("./script.ps1"), "script");
        assert_eq!(basename("a/b/c"), "c");
        assert_eq!(basename("a\\b\\c"), "c");

        // --- normalisation simple ---
        assert_eq!(normalize("ls -la").command, "ls");
        assert_eq!(normalize("Grep -i foo").command, "grep");
        assert_eq!(normalize("").command, "");
        assert!(normalize("FOO=1").is_empty());
        assert_eq!(normalize("FOO=1").assignments, vec!["FOO=1"]);

        // --- affectations ---
        let n = normalize("FOO=1 BAR=2 make all");
        assert_eq!(n.command, "make");
        assert_eq!(n.args, vec!["all"]);
        assert_eq!(n.assignments.len(), 2);

        // --- prefixes d'invocation ---
        let n = normalize("sudo -u root rm -rf /tmp/x");
        assert_eq!(n.command, "rm");
        assert_eq!(n.args, vec!["-rf", "/tmp/x"]);
        assert_eq!(n.wrappers, vec!["sudo"]);

        let n = normalize("env FOO=bar GIT_PAGER=cat git log");
        assert_eq!(n.command, "git");
        assert_eq!(n.args, vec!["log"]);
        assert_eq!(n.assignments, vec!["FOO=bar", "GIT_PAGER=cat"]);

        let n = normalize("nohup nice -n 10 python3 app.py > log 2>&1");
        assert_eq!(n.command, "python3");
        assert_eq!(n.args, vec!["app.py"]);

        let n = normalize("timeout -s KILL 30 npm test");
        assert_eq!(n.command, "npm");
        assert_eq!(n.args, vec!["test"]);

        // --- shell embarque POSIX ---
        let n = normalize("bash -lc 'rm -rf /'");
        assert_eq!(n.command, "rm");
        assert_eq!(n.args, vec!["-rf", "/"]);
        assert!(n.wrappers.contains(&"bash".to_string()));

        // --- shell embarque Windows ---
        let n = normalize("powershell.exe -NoProfile -Command \"Remove-Item x\"");
        assert_eq!(n.command, "remove-item");
        assert!(n.windows_shell);

        let n = normalize("cmd.exe /c dir");
        assert_eq!(n.command, "dir");
        assert!(n.windows_shell);

        // PowerShell opaque : on garde le wrapper comme commande analysee
        let n = normalize("pwsh -EncodedCommand ZQBjAGgAbwA=");
        assert_eq!(n.command, "pwsh");
        assert!(n.windows_shell);

        // --- appels d'outils habituels ---
        assert_eq!(normalize("python").command, "python");
        assert_eq!(normalize("C:\\Tools\\node.exe app.js").command, "node");
        assert_eq!(normalize("git commit -m 'x'").command, "git");
        assert!(normalize("git commit -m 'x'").has_arg("-m"));
    }
}
