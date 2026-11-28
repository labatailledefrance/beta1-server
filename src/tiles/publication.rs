//! Publication de tiles produite par `beta1-datachain` (H-06), lue sous `$TILES`.
//!
//! ```text
//! <publication>/COMPLET                          écrit en dernier par la chaîne
//! <publication>/MANIFESTE                        publication, version, Dxxx <empreinte>, graphe <empreinte>
//! <publication>/departements/Dxxx/paquets/*.p07  paquets de 16 × 16 tiles, index en tête
//! ```
//!
//! À l'ouverture, le serveur lit l'index de chaque paquet (une lecture par
//! fichier, rien de décompressé) et en fait un index national : tile →
//! (paquet, décalage, taille). Lire une tile, c'est ensuite une lecture à un
//! décalage connu. Une tile n'appartient qu'à un département : une tile vue
//! deux fois est une publication invalide.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use beta1_common::geo::TileId;
use beta1_common::tile::{self, PACK_SIDE, pack_index_len};

/// Erreur d'ouverture ou de lecture d'une publication.
#[derive(Debug)]
pub struct Erreur(pub String);

impl fmt::Display for Erreur {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Erreur {}

fn erreur(chemin: &Path, message: impl fmt::Display) -> Erreur {
    Erreur(format!("{} : {message}", chemin.display()))
}

/// Emplacement d'une tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Emplacement {
    paquet: u32,
    decalage: u64,
    taille: u32,
}

/// Publication ouverte : identifiant, départements, index national.
#[derive(Debug)]
pub struct Publication {
    /// Identifiant (première ligne du MANIFESTE, nom du dossier).
    pub id: String,
    /// Départements, dans l'ordre du MANIFESTE.
    pub departements: Vec<String>,
    paquets: Vec<PathBuf>,
    index: BTreeMap<TileId, Emplacement>,
}

/// Contenu utile d'un MANIFESTE : (publication, départements).
pub fn lire_manifeste(texte: &str) -> Result<(String, Vec<String>), String> {
    let mut lignes = texte.lines();
    let id = match lignes.next().and_then(|l| l.strip_prefix("publication ")) {
        Some(id) if !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit()) => id.to_owned(),
        _ => return Err("première ligne attendue : « publication <hexadécimal> »".into()),
    };
    if !lignes.next().is_some_and(|l| l.starts_with("version ")) {
        return Err("deuxième ligne attendue : « version x.y.z »".into());
    }
    let mut departements = Vec::new();
    for l in lignes {
        let Some(nom) = l.split_whitespace().next() else { continue };
        // D029, D02A, D974 : D suivi de trois caractères alphanumériques.
        if nom.len() == 4 && nom.starts_with('D') && nom[1..].bytes().all(|b| b.is_ascii_alphanumeric()) {
            departements.push(nom.to_owned());
        }
    }
    if departements.is_empty() {
        return Err("aucun département".into());
    }
    Ok((id, departements))
}

impl Publication {
    /// Ouvre la publication du dossier `racine` ; `threads` lisent les index en parallèle.
    pub fn ouvrir(racine: &Path, threads: usize) -> Result<Publication, Erreur> {
        if !racine.join("COMPLET").is_file() {
            return Err(erreur(racine, "publication incomplète (pas de fichier COMPLET)"));
        }
        let chemin = racine.join("MANIFESTE");
        let texte = std::fs::read_to_string(&chemin).map_err(|e| erreur(&chemin, e))?;
        let (id, departements) = lire_manifeste(&texte).map_err(|e| erreur(&chemin, e))?;
        // Paquets de chaque département, triés : l'index ne dépend pas de l'ordre du disque.
        let mut paquets = Vec::new();
        for d in &departements {
            let dossier = racine.join("departements").join(d).join("paquets");
            let mut fichiers: Vec<PathBuf> = std::fs::read_dir(&dossier)
                .map_err(|e| erreur(&dossier, e))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "p07"))
                .collect();
            fichiers.sort();
            paquets.extend(fichiers);
        }
        // Index des paquets, lus en parallèle, réunis dans l'ordre des paquets.
        let threads = threads.clamp(1, 64);
        let mut lus: Vec<Result<Vec<tile::PackEntry>, Erreur>> = Vec::with_capacity(paquets.len());
        std::thread::scope(|s| {
            let parts: Vec<_> = (0..threads)
                .map(|t| {
                    let paquets = &paquets;
                    s.spawn(move || {
                        (t..paquets.len()).step_by(threads).map(|k| (k, lire_index(&paquets[k]))).collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut tous: Vec<_> = parts.into_iter().flat_map(|p| p.join().unwrap_or_default()).collect();
            tous.sort_by_key(|(k, _)| *k);
            lus = tous.into_iter().map(|(_, r)| r).collect();
        });
        if lus.len() != paquets.len() {
            return Err(erreur(racine, "lecture des index interrompue"));
        }
        let mut index = BTreeMap::new();
        for (k, r) in lus.into_iter().enumerate() {
            for e in r? {
                let t = TileId { ix: e.ix, iy: e.iy };
                let place = Emplacement { paquet: k as u32, decalage: e.offset, taille: e.len };
                if index.insert(t, place).is_some() {
                    return Err(erreur(&paquets[k], format!("tile ({}, {}) déjà dans un autre paquet", t.ix, t.iy)));
                }
            }
        }
        Ok(Publication { id, departements, paquets, index })
    }

    /// Nombre de tiles.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Aucune tile.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Nombre de paquets.
    pub fn nb_paquets(&self) -> usize {
        self.paquets.len()
    }

    /// La publication a cette tile (sinon : hors des données, de la mer).
    pub fn contient(&self, t: TileId) -> bool {
        self.index.contains_key(&t)
    }

    /// Octets d'une tile ; `None` si elle est hors des données.
    pub fn lire(&self, t: TileId) -> Result<Option<Vec<u8>>, Erreur> {
        let Some(e) = self.index.get(&t) else { return Ok(None) };
        let chemin = &self.paquets[e.paquet as usize];
        let mut f = File::open(chemin).map_err(|x| erreur(chemin, x))?;
        f.seek(SeekFrom::Start(e.decalage)).map_err(|x| erreur(chemin, x))?;
        let mut b = vec![0; e.taille as usize];
        f.read_exact(&mut b).map_err(|x| erreur(chemin, format!("tile ({}, {}) : {x}", t.ix, t.iy)))?;
        Ok(Some(b))
    }
}

/// Index d'un paquet, en une lecture (l'index le plus long possible tient en 5 ko).
fn lire_index(chemin: &Path) -> Result<Vec<tile::PackEntry>, Erreur> {
    let mut f = File::open(chemin).map_err(|e| erreur(chemin, e))?;
    let mut b = vec![0; pack_index_len((PACK_SIDE * PACK_SIDE) as usize)];
    let mut n = 0;
    while n < b.len() {
        match f.read(&mut b[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) => return Err(erreur(chemin, e)),
        }
    }
    b.truncate(n);
    tile::decode_pack_index(&b).map_err(|_| erreur(chemin, "index de paquet invalide"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifeste() {
        let t = "publication e3bcea35\nversion 0.1.0\nD029 eb1c\nD02A bb\nD974 aa\ngraphe c52b\n";
        let (id, d) = lire_manifeste(t).unwrap();
        assert_eq!(id, "e3bcea35");
        assert_eq!(d, ["D029", "D02A", "D974"]);
        assert!(lire_manifeste("version 0.1.0\n").is_err());
        assert!(lire_manifeste("publication ../x\nversion 0.1.0\nD029 a\n").is_err());
        assert!(lire_manifeste("publication ab\nversion 0.1.0\ngraphe c\n").is_err());
    }
}
