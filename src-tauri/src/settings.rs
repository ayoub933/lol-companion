//! Réglages de l'app, gardés dans reglages.json (dossier de configuration de l'app).

use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub import_runes: bool,
    pub import_item_set: bool,
    pub import_spells: bool,
    /// Touche du Saut éclair : "auto" (celle où il est déjà, sinon F), "D" ou "F".
    pub flash_key: String,
    /// true = un set d'objets par champion ; false = un seul, remplacé à chaque partie.
    pub item_set_per_champion: bool,
    /// Lancement avec Windows : c'est Windows qui le garde, ce champ ne fait que le transmettre.
    pub start_with_windows: bool,
    pub show_on_client_start: bool,
    pub notify_match_found: bool,
    /// true = la croix réduit l'app dans la zone de notification (près de l'horloge).
    pub close_to_tray: bool,
    /// En partie : objets à acheter sur le bord gauche de l'écran du jeu.
    pub show_build_overlay: bool,
    /// true = ce panneau reste affiché toute la partie ; false = seulement en tenant Tab.
    pub build_overlay_always: bool,
    /// En partie, en tenant Tab : écarts d'or à côté du tableau des scores.
    pub show_gold_overlay: bool,
    /// true = noms des objets, runes et sorts en anglais ; false = ceux du client, dans sa langue.
    pub english_names: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            import_runes: true,
            import_item_set: true,
            import_spells: true,
            flash_key: "auto".to_string(),
            item_set_per_champion: false,
            start_with_windows: false,
            show_on_client_start: true,
            notify_match_found: true,
            close_to_tray: true,
            show_build_overlay: true,
            build_overlay_always: false,
            show_gold_overlay: true,
            english_names: false,
        }
    }
}

impl Settings {
    /// Réglages enregistrés, ou ceux par défaut si le fichier manque ou est illisible.
    pub fn load(file: &Path) -> Self {
        fs::read_to_string(file)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, file: &Path) -> Result<(), String> {
        let write = || -> std::io::Result<()> {
            if let Some(dir) = file.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(file, serde_json::to_string_pretty(self)?)
        };
        write().map_err(|e| format!("Réglages non enregistrés : {e}"))
    }
}
