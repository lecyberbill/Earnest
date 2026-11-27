use js_syntax_check::{check_file, CheckResult};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

fn tmp_path(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.set_file_name(format!("jssyntaxcheck_test_{}_{}_{}", std::process::id(), n, name));
    p
}

#[test]
fn js_valide() {
    let p = tmp_path("ok.js");
    std::fs::write(&p, "const a = 1 + 1;\nfunction f(x) { return x * 2; }\n").unwrap();
    let r = check_file(&p);
    let _ = std::fs::remove_file(&p);
    assert!(r.ok, "JS valide doit passer : {:?}", r);
    assert_eq!(r.scripts_checked, 1);
    assert!(r.error.is_none());
    assert!(r.line.is_none());
}

#[test]
fn js_syntax_error() {
    let p = tmp_path("bad.js");
    // `\<fin de ligne>` = invalid escape sequence -> SyntaxError sur la ligne 2.
    std::fs::write(&p, "const ok = 42;\nconst bad = '\\';\n").unwrap();
    let r = check_file(&p);
    let _ = std::fs::remove_file(&p);
    assert!(!r.ok, "JS invalide doit echouer");
    assert!(r.error.is_some(), "un message d'erreur est attendu");
    assert_eq!(r.line, Some(2), "ligne 2 attendue, obtenue : {:?}", r.line);
}

#[test]
fn html_script_inline_invalide() {
    let p = tmp_path("page.html");
    //      1: <!DOCTYPE html>
    //      2: <html>
    //      3: <head>
    //      4:   <title>test</title>
    //      5: </head>
    //      6: <body>
    //      7:   <p>bonjour</p>
    //      8:   <script>
    //      9:     const ok = 1 + 1;
    //     10:     const bad = '\';          <- erreur, ligne 10 du HTML
    //     11:   </script>
    //     12:   <script src="..."></script>
    //     13: </body>
    //     14: </html>
    let html = "<!DOCTYPE html>\n\
                <html>\n\
                <head>\n\
                  <title>test</title>\n\
                </head>\n\
                <body>\n\
                  <p>bonjour</p>\n\
                  <script>\n\
                    const ok = 1 + 1;\n\
                    const bad = '\\';\n\
                  </script>\n\
                  <script src=\"https://cdn.example.com/lib.js\"></script>\n\
                </body>\n\
                </html>\n";
    std::fs::write(&p, html).unwrap();
    let r = check_file(&p);
    let _ = std::fs::remove_file(&p);
    assert!(!r.ok, "HTML avec script inline invalide doit echouer : {:?}", r);
    assert_eq!(
        r.line,
        Some(10),
        "ligne reelle dans le HTML attendue (10), obtenue : {:?}",
        r.line
    );
    // Le script avec src= est ignore, seul le bloc inline est compte.
    assert_eq!(r.scripts_checked, 1);
}

#[test]
fn html_scripts_valides() {
    let p = tmp_path("ok.html");
    let html = "<!DOCTYPE html>\n<html>\n<body>\n\
                <script src=\"app.js\"></script>\n\
                <script>\nconst a = 1;\n</script>\n\
                </body>\n</html>\n";
    std::fs::write(&p, html).unwrap();
    let r = check_file(&p);
    let _ = std::fs::remove_file(&p);
    assert!(r.ok, "HTML avec scripts valides doit passer : {:?}", r);
    assert_eq!(r.scripts_checked, 1, "seul le bloc inline sans src= est verifie");
}

#[test]
fn to_json_structure() {
    let r = CheckResult {
        ok: false,
        scripts_checked: 1,
        error: Some("SyntaxError: Invalid or unexpected token".to_string()),
        line: Some(3),
    };
    let j = r.to_json();
    let expected = "{\"ok\": false, \"scripts_checked\": 1, \"error\": \"SyntaxError: Invalid or unexpected token\", \"line\": 3}";
    assert_eq!(j, expected);

    let r2 = CheckResult {
        ok: true,
        scripts_checked: 2,
        error: None,
        line: None,
    };
    let j2 = r2.to_json();
    assert_eq!(j2, "{\"ok\": true, \"scripts_checked\": 2, \"error\": null, \"line\": null}");

    // Echappement des guillemets dans le champ error.
    let r3 = CheckResult {
        ok: false,
        scripts_checked: 1,
        error: Some("contains \"quotes\"".to_string()),
        line: None,
    };
    let j3 = r3.to_json();
    assert_eq!(j3, "{\"ok\": false, \"scripts_checked\": 1, \"error\": \"contains \\\"quotes\\\"\", \"line\": null}");
}

#[test]
fn api_surface_et_extension_inconnue() {
    // Construction directe + champs publics accessibles.
    let r = CheckResult {
        ok: true,
        scripts_checked: 0,
        error: None,
        line: None,
    };
    assert!(r.ok);
    let _s: String = r.to_json();

    // Extension inconnue -> resultat coherent, pas de panic.
    let p = tmp_path("data.txt");
    std::fs::write(&p, "hello").unwrap();
    let r2 = check_file(&p);
    let _ = std::fs::remove_file(&p);
    assert!(!r2.ok);
    assert!(r2.error.is_some());
}