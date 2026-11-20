//! Détection de fichiers binaires et bornage de taille.
//! Contrats C5 (détection binaire) et C6 (borne de taille).

use crate::error::GuardError;

/// Taille maximale autorisée en octets pour un fichier analysable.
pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 Mo

/// Détermine si les octets fournis ressemblent à du contenu binaire.
/// Heuristique standard : présence d'un NUL byte ou proportion de
/// caractères non imprimables.
pub fn is_binary(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    if data.contains(&0u8) {
        return true;
    }
    let sample = &data[..data.len().min(8000)];
    let non_printable = sample
        .iter()
        .filter(|&&b| b < 9 || (b > 13 && b < 32))
        .count();
    non_printable as f64 / sample.len() as f64 > 0.30
}

/// Valide que la taille d'un fichier (déjà connue, p. ex. via metadata)
/// respecte la borne autorisée.
pub fn enforce_size_bound(size: u64) -> Result<(), GuardError> {
    if size > MAX_FILE_SIZE {
        return Err(GuardError::FileTooLarge {
            size,
            max: MAX_FILE_SIZE,
        });
    }
    Ok(())
}

/// Analyse un contenu mémoire : taille puis nature (binaire / texte).
pub fn inspect(content: &[u8]) -> Result<bool, GuardError> {
    enforce_size_bound(content.len() as u64)?;
    Ok(!is_binary(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_file_detection() {
        let binary = [0x00, 0x01, 0x02, 0xff, 0xfe];
        assert!(is_binary(&binary));

        let text = b"hello world, this is plain text content\n";
        assert!(!is_binary(text));

        let utf8_text = "café au lait — très bien".as_bytes();
        assert!(!is_binary(utf8_text));

        let empty: &[u8] = b"";
        assert!(!is_binary(empty));
    }

    #[test]
    fn test_file_size_bound() {
        assert!(enforce_size_bound(0).is_ok());
        assert!(enforce_size_bound(MAX_FILE_SIZE).is_ok());
        assert!(enforce_size_bound(MAX_FILE_SIZE + 1).is_err());

        let small = vec![b'a'; 1024];
        assert!(inspect(&small).unwrap());

        let too_big = vec![b'a'; (MAX_FILE_SIZE as usize) + 1];
        assert!(inspect(&too_big).is_err());
    }
}
