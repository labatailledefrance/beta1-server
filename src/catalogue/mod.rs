//! Catalogue : tout ce qui décrit le jeu, lu depuis des mods (E-02, E-05, F-03).
//!
//! Analogie : les fichiers TOML sont des objets d'une CRD, ce module est
//! l'admission controller qui les refuse au moindre écart, puis les compile en
//! une ressource [`Catalogue`] indexée par entiers. Aucune chaîne de caractères
//! ne sert dans la boucle de simulation.
//!
//! Disposition d'un mod :
//!
//! ```text
//! mods/<id>/mod.toml                 identifiant, version, dépendances
//! mods/<id>/**/<objet>.toml          un objet par fichier, l'identifiant est le nom du fichier
//! mods/<id>/locales/fr/*.ftl         textes (Fluent), clés « <genre>-<identifiant> »
//! ```
//!
//! Un mod chargé plus tard remplace un objet de même genre et de même
//! identifiant, et un texte de même clé.

pub mod mods;
pub mod objets;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use fluent_syntax::ast::Entry;

use beta1_common::codec::Writer;
use beta1_common::protocol::TICKS_PER_SECOND;

use objets::Objet;

/// Dossier des textes dans un mod, et seule langue de la beta 1 (E-05).
pub const LOCALES: &str = "locales";
/// Langue des textes.
pub const LANGUE: &str = "fr";

/// Version de l'encodage binaire du catalogue (envoyé au client, haché).
const FORMAT: u8 = 1;

/// Erreurs de chargement, toutes rapportées d'un coup, une par ligne.
#[derive(Debug, Default)]
pub struct Erreurs(Vec<String>);

impl Erreurs {
    fn ajouter(&mut self, chemin: &Path, message: impl fmt::Display) {
        self.0.push(format!("{} : {message}", chemin.display()));
    }
    fn vide(&self) -> bool {
        self.0.is_empty()
    }
    /// Nombre d'erreurs.
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Aucune erreur.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Messages, un par erreur.
    pub fn messages(&self) -> &[String] {
        &self.0
    }
}

impl fmt::Display for Erreurs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for e in &self.0 {
            writeln!(f, "{e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Erreurs {}

/// Identifiant d'objet ou de mod : minuscules ASCII, chiffres, tirets, commence par une lettre.
/// C'est aussi un identifiant Fluent valide et un nom de fichier du projet.
pub fn identifiant_valide(id: &str) -> bool {
    let mut o = id.bytes();
    matches!(o.next(), Some(b'a'..=b'z'))
        && id.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
        && !id.ends_with('-')
        && !id.contains("--")
}

/// Indice d'un type de zombie dans le catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdZombie(pub u16);

/// Type de zombie compilé : unités de simulation (m, m/s, ticks).
#[derive(Debug, Clone, PartialEq)]
pub struct TypeZombie {
    /// Identifiant (affichage, journaux ; jamais dans la boucle de simulation).
    pub id: String,
    /// m/s.
    pub vitesse_errance: f32,
    /// m/s.
    pub vitesse_course: f32,
    /// Fraction.
    pub ecart_vitesse: f32,
    /// Facteur.
    pub facteur_eau: f32,
    /// Ticks.
    pub periode_cap: u32,
    /// Cosinus du demi-angle du cône de vision en errance.
    pub cos_demi_cone: f32,
    /// m.
    pub rayon_odorat: f32,
    /// Ticks.
    pub perception_errance: u32,
    /// Ticks.
    pub perception_chasse: u32,
    /// Ticks.
    pub memoire_proie: u32,
    /// m.
    pub rayon_relais: f32,
    /// Ticks.
    pub fraicheur_alerte: u32,
    /// m.
    pub rayon_fumee: f32,
    /// Ticks.
    pub duree_attirance: u32,
    /// m.
    pub rayon_contact: f32,
    /// Ticks.
    pub delai_zombification: u32,
}

/// Fichier de textes Fluent d'un mod, tel quel (le client le donne à un
/// `FluentBundle`, dans l'ordre des mods).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Textes {
    /// Mod d'origine.
    pub module: String,
    /// Chemin dans le mod.
    pub chemin: String,
    /// Source Fluent.
    pub source: String,
}

/// Catalogue compilé : ressource Bevy, en lecture seule pendant la partie.
#[derive(Resource, Debug, Clone)]
pub struct Catalogue {
    /// Mods chargés, dans l'ordre : (identifiant, version).
    pub mods: Vec<(String, String)>,
    /// Types de zombies, triés par identifiant ; l'indice est un [`IdZombie`].
    pub zombies: Vec<TypeZombie>,
    /// Textes, dans l'ordre des mods.
    pub textes: Vec<Textes>,
    hash: [u8; 32],
}

impl Catalogue {
    /// Type de zombie par identifiant (chargement de l'état initial, pas la boucle de simulation).
    pub fn id_zombie(&self, id: &str) -> Option<IdZombie> {
        self.zombies.binary_search_by(|z| z.id.as_str().cmp(id)).ok().map(|i| IdZombie(i as u16))
    }

    /// Type de zombie.
    pub fn zombie(&self, id: IdZombie) -> &TypeZombie {
        &self.zombies[id.0 as usize]
    }

    /// Hash BLAKE3 de l'encodage binaire : il change dès qu'un mod, un objet ou un texte change.
    pub fn hash(&self) -> [u8; 32] {
        self.hash
    }

    /// Les 8 premiers octets du hash, graine du hash d'état.
    pub fn hash_court(&self) -> u64 {
        let mut b = [0; 8];
        b.copy_from_slice(&self.hash[..8]);
        u64::from_le_bytes(b)
    }

    /// Encodage binaire, celui que recevra le client au handshake.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.bytes(b"CATA");
        w.u8(FORMAT);
        w.varint(self.mods.len() as u64);
        for (id, version) in &self.mods {
            w.string(id);
            w.string(version);
        }
        w.varint(self.zombies.len() as u64);
        for z in &self.zombies {
            w.string(&z.id);
            for v in [z.vitesse_errance, z.vitesse_course, z.ecart_vitesse, z.facteur_eau] {
                w.f32(v);
            }
            w.u32(z.periode_cap);
            w.f32(z.cos_demi_cone);
            w.f32(z.rayon_odorat);
            w.u32(z.perception_errance);
            w.u32(z.perception_chasse);
            w.u32(z.memoire_proie);
            w.f32(z.rayon_relais);
            w.u32(z.fraicheur_alerte);
            w.f32(z.rayon_fumee);
            w.u32(z.duree_attirance);
            w.f32(z.rayon_contact);
            w.u32(z.delai_zombification);
        }
        w.varint(self.textes.len() as u64);
        for t in &self.textes {
            w.string(&t.module);
            w.string(&t.chemin);
            w.string(&t.source);
        }
        w.buf
    }
}

/// Objet lu, avec son fichier.
struct Lu {
    objet: Objet,
    chemin: PathBuf,
}

/// Charge les mods de `liste` depuis `racine`, les fusionne, les valide et les compile.
pub fn charger(racine: &Path, liste: &[String]) -> Result<Catalogue, Erreurs> {
    let mut erreurs = Erreurs::default();
    if liste.is_empty() {
        erreurs.ajouter(racine, "aucun mod à charger");
        return Err(erreurs);
    }
    let ordre = mods::ordonner(racine, liste, &mut erreurs);
    if !erreurs.vide() {
        return Err(erreurs);
    }
    // (genre, identifiant) → objet ; un mod suivant remplace.
    let mut objets: BTreeMap<(&'static str, String), Lu> = BTreeMap::new();
    // Clés Fluent de tous les mods.
    let mut cles: BTreeSet<String> = BTreeSet::new();
    let mut textes = Vec::new();
    for m in &ordre {
        let id_mod = &m.manifeste.id;
        let mut fichiers = Vec::new();
        parcourir(&m.dossier, &m.dossier, &mut fichiers, &mut erreurs);
        fichiers.sort();
        let mut dans_ce_mod: BTreeMap<(&'static str, String), PathBuf> = BTreeMap::new();
        let mut cles_du_mod: BTreeMap<String, PathBuf> = BTreeMap::new();
        for relatif in fichiers {
            let chemin = m.dossier.join(&relatif);
            let texte = match std::fs::read_to_string(&chemin) {
                Ok(t) => t,
                Err(e) => {
                    erreurs.ajouter(&chemin, format!("lecture impossible : {e}"));
                    continue;
                }
            };
            if relatif.starts_with(LOCALES) {
                for cle in lire_fluent(&chemin, texte.clone(), &mut erreurs) {
                    if let Some(avant) = cles_du_mod.insert(cle.clone(), chemin.clone()) {
                        erreurs.ajouter(&chemin, format!("clé « {cle} » déjà définie dans {}", avant.display()));
                    }
                    cles.insert(cle);
                }
                let chemin_texte = relatif.to_string_lossy().replace('\\', "/");
                textes.push(Textes { module: id_mod.clone(), chemin: chemin_texte, source: texte });
                continue;
            }
            let Some(id) = relatif.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
                erreurs.ajouter(&chemin, "nom de fichier illisible");
                continue;
            };
            if !identifiant_valide(&id) {
                erreurs
                    .ajouter(&chemin, format!("identifiant « {id} » invalide (minuscules ASCII, chiffres et tirets)"));
                continue;
            }
            let objet: Objet = match toml::from_str(&texte) {
                Ok(o) => o,
                Err(e) => {
                    erreurs.ajouter(&chemin, e.to_string().trim_end());
                    continue;
                }
            };
            let mut hors_bornes = Vec::new();
            objet.verifier(&mut hors_bornes);
            for h in hors_bornes {
                erreurs.ajouter(&chemin, h);
            }
            let cle = (objet.genre(), id);
            if let Some(avant) = dans_ce_mod.insert(cle.clone(), chemin.clone()) {
                erreurs.ajouter(&chemin, format!("{} « {} » déjà défini dans {}", cle.0, cle.1, avant.display()));
                continue;
            }
            objets.insert(cle, Lu { objet, chemin });
        }
    }
    // Chaque objet a son nom affiché.
    for ((genre, id), lu) in &objets {
        let cle = format!("{genre}-{id}");
        if !cles.contains(&cle) {
            erreurs.ajouter(&lu.chemin, format!("texte « {cle} » absent des fichiers {LOCALES}/{LANGUE}/*.ftl"));
        }
    }
    let mut zombies = Vec::new();
    for ((_, id), lu) in &objets {
        match &lu.objet {
            Objet::Zombie(z) => match compiler_zombie(id, z) {
                Ok(t) => zombies.push(t),
                Err(e) => {
                    for m in e {
                        erreurs.ajouter(&lu.chemin, m);
                    }
                }
            },
        }
    }
    if zombies.len() > u16::MAX as usize {
        erreurs.ajouter(racine, "plus de 65 535 types de zombies");
    }
    if !erreurs.vide() {
        return Err(erreurs);
    }
    let mut c = Catalogue {
        mods: ordre.iter().map(|m| (m.manifeste.id.clone(), m.manifeste.version.clone())).collect(),
        zombies,
        textes,
        hash: [0; 32],
    };
    c.hash = *blake3::hash(&c.encode()).as_bytes();
    Ok(c)
}

/// Liste les fichiers d'un mod (chemins relatifs), sauf `mod.toml` ; refuse ce qui n'a rien à y faire.
fn parcourir(racine: &Path, dossier: &Path, sortie: &mut Vec<PathBuf>, erreurs: &mut Erreurs) {
    let entrees = match std::fs::read_dir(dossier) {
        Ok(e) => e,
        Err(e) => {
            erreurs.ajouter(dossier, format!("lecture impossible : {e}"));
            return;
        }
    };
    for entree in entrees.flatten() {
        let chemin = entree.path();
        let Ok(relatif) = chemin.strip_prefix(racine).map(Path::to_path_buf) else { continue };
        if chemin.is_dir() {
            parcourir(racine, &chemin, sortie, erreurs);
            continue;
        }
        let ext = chemin.extension().and_then(|e| e.to_str()).unwrap_or("");
        let dans_locales = relatif.starts_with(Path::new(LOCALES).join(LANGUE));
        match ext {
            _ if relatif == Path::new("mod.toml") => {}
            "md" => {}
            "ftl" if dans_locales => sortie.push(relatif),
            "toml" if !relatif.starts_with(LOCALES) => sortie.push(relatif),
            _ => erreurs.ajouter(&chemin, "fichier inattendu dans un mod"),
        }
    }
}

/// Vérifie la syntaxe d'un fichier Fluent et rend ses clés de messages.
fn lire_fluent(chemin: &Path, source: String, erreurs: &mut Erreurs) -> Vec<String> {
    let ressource = match fluent_syntax::parser::parse(source) {
        Ok(r) => r,
        Err((_, fautes)) => {
            for f in fautes {
                erreurs.ajouter(chemin, format!("Fluent, octets {:?} : {:?}", f.pos, f.kind));
            }
            return Vec::new();
        }
    };
    ressource
        .body
        .into_iter()
        .filter_map(|e| match e {
            Entry::Message(m) => Some(m.id.name),
            _ => None,
        })
        .collect()
}

/// Secondes en ticks ; refuse ce qui ne tombe pas sur un tick.
fn ticks(champ: &str, secondes: f64, erreurs: &mut Vec<String>) -> u32 {
    let t = secondes * f64::from(TICKS_PER_SECOND);
    let arrondi = t.round();
    if (t - arrondi).abs() > 1e-6 {
        erreurs.push(format!("{champ} = {secondes} s ne tombe pas sur un tick (multiple de 1/{TICKS_PER_SECOND} s)"));
    }
    arrondi as u32
}

fn compiler_zombie(id: &str, z: &objets::Zombie) -> Result<TypeZombie, Vec<String>> {
    let mut e = Vec::new();
    let t = TypeZombie {
        id: id.to_owned(),
        vitesse_errance: z.vitesse_errance as f32,
        vitesse_course: z.vitesse_course as f32,
        ecart_vitesse: z.ecart_vitesse as f32,
        facteur_eau: z.facteur_eau as f32,
        periode_cap: ticks("periode_cap", z.periode_cap, &mut e),
        cos_demi_cone: (z.cone_vision / 2.0).to_radians().cos() as f32,
        rayon_odorat: z.rayon_odorat as f32,
        perception_errance: ticks("perception_errance", z.perception_errance, &mut e),
        perception_chasse: ticks("perception_chasse", z.perception_chasse, &mut e),
        memoire_proie: ticks("memoire_proie", z.memoire_proie, &mut e),
        rayon_relais: z.rayon_relais as f32,
        fraicheur_alerte: ticks("fraicheur_alerte", z.fraicheur_alerte, &mut e),
        rayon_fumee: z.rayon_fumee as f32,
        duree_attirance: ticks("duree_attirance", z.duree_attirance, &mut e),
        rayon_contact: z.rayon_contact as f32,
        delai_zombification: ticks("delai_zombification", z.delai_zombification, &mut e),
    };
    if z.vitesse_course < z.vitesse_errance {
        e.push("vitesse_course inférieure à vitesse_errance".to_owned());
    }
    if e.is_empty() { Ok(t) } else { Err(e) }
}

/// JSON Schemas des fichiers de mods, versionnés dans `schema/` (E-02) : (nom du fichier, contenu).
pub fn schemas() -> Vec<(&'static str, String)> {
    let json = |s: schemars::Schema| {
        let mut t = serde_json::to_string_pretty(&s).unwrap_or_default();
        t.push('\n');
        t
    };
    vec![
        ("mod.schema.json", json(schemars::schema_for!(mods::Manifeste))),
        ("objet.schema.json", json(schemars::schema_for!(Objet))),
    ]
}

/// Insère le catalogue dans l'`App` ; il a été chargé et validé avant.
pub struct CataloguePlugin(pub Catalogue);

impl Plugin for CataloguePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiants() {
        for ok in ["base", "ordinaire", "vab-ultima", "p4"] {
            assert!(identifiant_valide(ok), "{ok}");
        }
        for ko in ["", "Base", "4x4", "a_b", "fin-", "a--b", "é"] {
            assert!(!identifiant_valide(ko), "{ko}");
        }
    }

    #[test]
    fn ticks_exacts() {
        let mut e = Vec::new();
        assert_eq!(ticks("x", 0.25, &mut e), 5);
        assert_eq!(ticks("x", 60.0, &mut e), 1200);
        assert!(e.is_empty());
        ticks("x", 0.01, &mut e);
        assert_eq!(e.len(), 1);
    }
}
