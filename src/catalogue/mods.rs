//! Mods : `mod.toml` et ordre de chargement (F-03).
//!
//! L'ordre suit les dépendances, puis l'ordre déclaré : comme un `kustomize`
//! dont chaque overlay déclare sur quoi il s'appuie. Une dépendance absente
//! de la liste ou un cycle empêchent le démarrage.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Deserialize;

use super::{Erreurs, identifiant_valide};

/// Contenu de `mod.toml`.
#[derive(Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifeste {
    /// Identifiant du mod : nom de son dossier, en minuscules ASCII et tirets.
    pub id: String,
    /// Version, sous la forme `x.y.z`.
    pub version: String,
    /// Identifiants des mods à charger avant celui-ci.
    #[serde(default)]
    pub dependances: Vec<String>,
}

/// Un mod trouvé sur le disque.
#[derive(Debug, Clone)]
pub struct Mod {
    /// Son `mod.toml`.
    pub manifeste: Manifeste,
    /// Son dossier.
    pub dossier: PathBuf,
}

/// Lit les mods de `liste` dans `racine` et les rend dans l'ordre de chargement.
pub fn ordonner(racine: &Path, liste: &[String], erreurs: &mut Erreurs) -> Vec<Mod> {
    let mut lus = Vec::new();
    for (rang, id) in liste.iter().enumerate() {
        if liste[..rang].contains(id) {
            erreurs.ajouter(racine, format!("mod « {id} » listé deux fois"));
            continue;
        }
        let dossier = racine.join(id);
        let chemin = dossier.join("mod.toml");
        let texte = match std::fs::read_to_string(&chemin) {
            Ok(t) => t,
            Err(e) => {
                erreurs.ajouter(&chemin, format!("lecture impossible : {e}"));
                continue;
            }
        };
        let manifeste: Manifeste = match toml::from_str(&texte) {
            Ok(m) => m,
            Err(e) => {
                erreurs.ajouter(&chemin, e.to_string().trim_end());
                continue;
            }
        };
        if &manifeste.id != id {
            erreurs.ajouter(&chemin, format!("id « {} » différent du dossier « {id} »", manifeste.id));
            continue;
        }
        if !identifiant_valide(id) {
            erreurs.ajouter(&chemin, format!("id « {id} » invalide (minuscules ASCII, chiffres et tirets)"));
        }
        if !version_valide(&manifeste.version) {
            erreurs.ajouter(&chemin, format!("version « {} » invalide (attendu : x.y.z)", manifeste.version));
        }
        for d in &manifeste.dependances {
            if !liste.contains(d) {
                erreurs.ajouter(&chemin, format!("dépendance « {d} » absente de la liste des mods"));
            }
        }
        lus.push(Mod { manifeste, dossier });
    }
    if !erreurs.vide() {
        return Vec::new();
    }
    // Tri topologique stable : à chaque pas, le premier mod déclaré dont
    // toutes les dépendances sont déjà placées.
    let mut ordre: Vec<Mod> = Vec::with_capacity(lus.len());
    while !lus.is_empty() {
        let pret =
            lus.iter().position(|m| m.manifeste.dependances.iter().all(|d| ordre.iter().any(|o| &o.manifeste.id == d)));
        match pret {
            Some(i) => ordre.push(lus.remove(i)),
            None => {
                let restants: Vec<&str> = lus.iter().map(|m| m.manifeste.id.as_str()).collect();
                erreurs.ajouter(racine, format!("cycle de dépendances entre : {}", restants.join(", ")));
                return Vec::new();
            }
        }
    }
    ordre
}

fn version_valide(v: &str) -> bool {
    let parties: Vec<&str> = v.split('.').collect();
    parties.len() == 3 && parties.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(version_valide("0.1.0"));
        assert!(version_valide("12.0.3"));
        assert!(!version_valide("1.0"));
        assert!(!version_valide("1.0.x"));
        assert!(!version_valide("1..0"));
    }
}
