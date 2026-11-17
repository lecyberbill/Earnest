use std::fs;
use std::path::Path;
use serde_json::Value;
use crate::db::connection::get_data_dir;

pub fn create_artifact(
    project_id: &str,
    title: &str,
    content: &str,
    artifact_type: &str,
    file_path: Option<&str>,
) -> Result<Value, String> {
    let art_id = format!("art_{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..10]);
    let ext = match artifact_type {
        "python" => "py",
        "rust" => "rs",
        "javascript" => "js",
        "json" => "json",
        "html" => "html",
        _ => "md",
    };

    let safe_title = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>();

    let store_dir = get_data_dir().join("artifacts").join(project_id);
    fs::create_dir_all(&store_dir).map_err(|e| format!("Erreur création dossier artefacts: {}", e))?;

    let filename = format!("{}_{}.{}", safe_title, art_id, ext);
    let store_path = store_dir.join(&filename);

    // Écriture dans le magasin centralisé
    fs::write(&store_path, content).map_err(|e| format!("Erreur écriture artefact: {}", e))?;

    // Si un chemin de fichier personnalisé est fourni, miroir vers ce chemin
    let mut mirrored_path: Option<String> = None;
    if let Some(target_p) = file_path {
        let p = Path::new(target_p);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Erreur création dossier cible '{}': {}", target_p, e))?;
        }
        fs::write(p, content).map_err(|e| format!("Erreur écriture fichier cible '{}': {}", target_p, e))?;
        mirrored_path = Some(target_p.to_string());
    }

    Ok(serde_json::json!({
        "status": "success",
        "artifact_id": art_id,
        "filename": filename,
        "path": mirrored_path.as_deref().unwrap_or(&store_path.to_string_lossy()),
        "store_path": store_path.to_string_lossy(),
        "mirrored_path": mirrored_path,
        "size_bytes": content.len(),
        "artifact_type": artifact_type
    }))
}

