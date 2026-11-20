//! Configuration validée du détecteur de boucles (fail-fast).

/// Configuration immuable du détecteur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Taille de la fenêtre glissante (nombre d'événements retenus).
    pub window_size: usize,
    /// Seuil minimal de répétitions exactes consécutives.
    pub exact_repeat_threshold: usize,
    /// Nombre d'événements identiques consécutifs pour alerter (stagnation).
    pub stagnation_threshold: usize,
    /// Période minimale d'alternance (2 = A-B-A-B, 3 = A-B-C-…).
    pub alternation_threshold: usize,
    /// Score minimal pour qualifier une boucle de « détectée » (vs warning).
    pub graduation_threshold: u32,
}

impl Config {
    /// Construit une configuration validée ; rejette les valeurs incohérentes.
    pub fn new(
        window_size: usize,
        exact_repeat_threshold: usize,
        stagnation_threshold: usize,
        alternation_threshold: usize,
    ) -> Result<Self, ConfigError> {
        if window_size == 0 {
            return Err(ConfigError::ZeroWindow);
        }
        if window_size < 4 {
            return Err(ConfigError::WindowTooSmall(window_size));
        }
        if exact_repeat_threshold == 0 {
            return Err(ConfigError::ZeroThreshold("exact_repeat_threshold"));
        }
        if exact_repeat_threshold > window_size {
            return Err(ConfigError::ThresholdExceedsWindow {
                threshold: exact_repeat_threshold,
                window: window_size,
            });
        }
        if stagnation_threshold == 0 {
            return Err(ConfigError::ZeroThreshold("stagnation_threshold"));
        }
        if alternation_threshold < 2 {
            return Err(ConfigError::AlternationTooSmall(alternation_threshold));
        }
        Ok(Self {
            window_size,
            exact_repeat_threshold,
            stagnation_threshold,
            alternation_threshold,
            graduation_threshold: 3,
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new(64, 3, 8, 2).expect("default config must be valid")
    }
}

/// Erreurs de validation de configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// Fenêtre glissante nulle.
    ZeroWindow,
    /// Fenêtre trop petite (< 4).
    WindowTooSmall(usize),
    /// Seuil nul.
    ZeroThreshold(&'static str),
    /// Seuil supérieur à la fenêtre.
    ThresholdExceedsWindow { threshold: usize, window: usize },
    /// Seuil d'alternance insuffisant (< 2).
    AlternationTooSmall(usize),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroWindow => write!(f, "window_size must be > 0"),
            Self::WindowTooSmall(w) => write!(f, "window_size {w} is too small (min 4)"),
            Self::ZeroThreshold(field) => write!(f, "{field} must be > 0"),
            Self::ThresholdExceedsWindow { threshold, window } => {
                write!(f, "threshold {threshold} exceeds window {window}")
            }
            Self::AlternationTooSmall(a) => write!(f, "alternation_threshold {a} must be >= 2"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_rejection_zero_window() {
        assert!(matches!(Config::new(0, 3, 8, 2), Err(ConfigError::ZeroWindow)));
    }

    #[test]
    fn config_rejection_small_window() {
        assert!(matches!(Config::new(2, 1, 8, 2), Err(ConfigError::WindowTooSmall(2))));
    }

    #[test]
    fn config_rejection_threshold_exceeds_window() {
        assert!(matches!(
            Config::new(4, 10, 8, 2),
            Err(ConfigError::ThresholdExceedsWindow { threshold: 10, window: 4 })
        ));
    }

    #[test]
    fn config_rejection_zero_exact_threshold() {
        assert!(matches!(
            Config::new(64, 0, 8, 2),
            Err(ConfigError::ZeroThreshold("exact_repeat_threshold"))
        ));
    }

    #[test]
    fn config_rejection_alternation_too_small() {
        assert!(matches!(Config::new(64, 3, 8, 1), Err(ConfigError::AlternationTooSmall(1))));
    }

    #[test]
    fn config_valid_construction() {
        let c = Config::new(64, 3, 8, 2).unwrap();
        assert_eq!(c.window_size, 64);
        assert_eq!(c.graduation_threshold, 3);
    }

    #[test]
    fn config_default_is_valid() {
        let c = Config::default();
        assert!(c.window_size >= 4);
    }
}
