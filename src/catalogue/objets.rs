//! Fichiers d'objets du catalogue, tels qu'on les écrit (E-02).
//!
//! Un fichier TOML par objet ; son nom (sans `.toml`) est l'identifiant de
//! l'objet. Le fichier contient une seule table, dont le nom est le genre de
//! l'objet :
//!
//! ```toml
//! [zombie]
//! vitesse_errance = 0.5
//! ```
//!
//! Les unités sont humaines (mètres, secondes, degrés) ; la compilation les
//! convertit en unités de simulation (ticks, cosinus). Chaque paramètre est
//! déclaré une fois, avec ses bornes : la validation et le JSON Schema en
//! sortent tous deux, ils ne peuvent pas diverger.

use schemars::JsonSchema;
use serde::Deserialize;

/// Un objet du catalogue : une table, nommée par son genre.
#[derive(Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Objet {
    /// Type de zombie : déplacement, perception, alerte, contagion (ZOMBIES.md).
    Zombie(Zombie),
    /// Scénario : état initial du monde (G-06).
    Scenario(Scenario),
}

impl Objet {
    /// Genre de l'objet, préfixe de ses clés de traduction.
    pub fn genre(&self) -> &'static str {
        match self {
            Objet::Zombie(_) => "zombie",
            Objet::Scenario(_) => "scenario",
        }
    }

    /// Vérifie les bornes de chaque paramètre.
    pub fn verifier(&self, erreurs: &mut Vec<String>) {
        match self {
            Objet::Zombie(z) => z.verifier(erreurs),
            Objet::Scenario(s) => s.verifier(erreurs),
        }
    }
}

/// Déclare une table de paramètres réels bornés : struct, JSON Schema et vérification.
macro_rules! parametres {
    (
        $(#[$meta:meta])*
        pub struct $nom:ident {
            $( #[doc = $doc:literal] $champ:ident : [$min:literal, $max:literal], )*
        }
    ) => {
        $(#[$meta])*
        #[derive(Deserialize, JsonSchema, Debug, Clone, PartialEq)]
        #[serde(deny_unknown_fields)]
        pub struct $nom {
            $(
                #[doc = $doc]
                #[schemars(range(min = $min, max = $max))]
                pub $champ: f64,
            )*
        }

        impl $nom {
            fn verifier(&self, erreurs: &mut Vec<String>) {
                $( borne(stringify!($champ), self.$champ, $min, $max, erreurs); )*
            }
        }
    };
}

fn borne(champ: &str, v: f64, min: f64, max: f64, erreurs: &mut Vec<String>) {
    if !(min..=max).contains(&v) {
        erreurs.push(format!("{champ} = {v} hors des bornes [{min}, {max}]"));
    }
}

parametres! {
    /// Paramètres d'un type de zombie. Valeurs de départ : celles de P-26.
    pub struct Zombie {
        /// Vitesse en errance (m/s).
        vitesse_errance: [0.05, 5.0],
        /// Vitesse de course, en chasse (m/s).
        vitesse_course: [0.05, 15.0],
        /// Écart de vitesse d'un zombie à l'autre, en fraction (0,18 : de -18 % à +18 %).
        ecart_vitesse: [0.0, 0.5],
        /// Facteur de vitesse dans l'eau peu profonde.
        facteur_eau: [0.05, 1.0],
        /// Période de changement de cap en errance (s).
        periode_cap: [0.05, 600.0],
        /// Cône de vision en errance (degrés) ; en chasse, il voit tout autour.
        cone_vision: [0.0, 360.0],
        /// Distance sous laquelle il sent une proie hors de son cône (m).
        rayon_odorat: [0.0, 100.0],
        /// Période de perception en errance (s).
        perception_errance: [0.05, 60.0],
        /// Période de perception en chasse (s).
        perception_chasse: [0.05, 60.0],
        /// Durée pendant laquelle il poursuit une proie perdue de vue (s).
        memoire_proie: [0.05, 3600.0],
        /// Rayon de relais de l'alerte, de zombie à zombie (m).
        rayon_relais: [0.0, 50.0],
        /// Âge maximal d'une alerte encore relayée (s).
        fraicheur_alerte: [0.05, 600.0],
        /// Distance sous laquelle une fumée l'attire (m).
        rayon_fumee: [0.0, 20000.0],
        /// Durée d'attirance par un bruit ou une fumée (s).
        duree_attirance: [0.05, 3600.0],
        /// Distance de contact, de morsure (m).
        rayon_contact: [0.1, 5.0],
        /// Délai de zombification d'un humain mordu (s).
        delai_zombification: [0.05, 3600.0],
    }
}

/// Zone de projection d'une tile (C-01), dans l'ordre de `beta1_common::geo::ZONES`.
#[derive(Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneProjection {
    /// Lambert-93 : métropole.
    #[serde(rename = "LAMB93")]
    Lamb93,
    /// UTM 20N : Guadeloupe, Martinique.
    #[serde(rename = "UTM20")]
    Utm20,
    /// UTM 22N : Guyane.
    #[serde(rename = "UTM22")]
    Utm22,
    /// UTM 40S : La Réunion.
    #[serde(rename = "UTM40S")]
    Utm40s,
    /// UTM 38S : Mayotte.
    #[serde(rename = "UTM38S")]
    Utm38s,
    /// UTM 21N : Saint-Pierre-et-Miquelon.
    #[serde(rename = "UTM21")]
    Utm21,
}

impl ZoneProjection {
    /// Indice de la zone dans `beta1_common::geo::ZONES`.
    pub fn indice(self) -> u8 {
        self as u8
    }
}

/// Rectangle de tiles, bornes comprises, en kilomètres dans la projection de sa zone.
#[derive(Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rectangle {
    /// Zone de projection.
    pub zone: ZoneProjection,
    /// Colonnes [min, max] (km vers l'est).
    pub x: [i32; 2],
    /// Lignes [min, max] (km vers le nord).
    pub y: [i32; 2],
}

/// Plus de tiles actives au démarrage que cela : sans doute une faute de frappe.
pub const MAX_TILES_ACTIVES: i64 = 10_000;

/// Scénario : ce qui existe au démarrage du monde.
#[derive(Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Tiles actives au démarrage, en attendant que les unités les activent.
    pub tiles_actives: Vec<Rectangle>,
}

impl Scenario {
    fn verifier(&self, erreurs: &mut Vec<String>) {
        let mut total = 0i64;
        for (k, r) in self.tiles_actives.iter().enumerate() {
            let cote = beta1_common::geo::ZONE_STRIDE;
            if r.x[0] > r.x[1] || r.y[0] > r.y[1] {
                erreurs.push(format!("tiles_actives[{k}] : bornes inversées"));
                continue;
            }
            if r.x[0] < 0 || r.x[1] >= cote {
                erreurs.push(format!("tiles_actives[{k}] : x hors de [0, {}]", cote - 1));
            }
            if r.y[0] < -20_000 || r.y[1] > 20_000 {
                erreurs.push(format!("tiles_actives[{k}] : y hors de [-20000, 20000]"));
            }
            total += (r.x[1] - r.x[0] + 1) as i64 * (r.y[1] - r.y[0] + 1) as i64;
        }
        if total > MAX_TILES_ACTIVES {
            erreurs.push(format!("tiles_actives : {total} tiles, plus de {MAX_TILES_ACTIVES}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_dans_l_ordre_de_common() {
        let zones = [
            ZoneProjection::Lamb93,
            ZoneProjection::Utm20,
            ZoneProjection::Utm22,
            ZoneProjection::Utm40s,
            ZoneProjection::Utm38s,
            ZoneProjection::Utm21,
        ];
        assert_eq!(zones.len(), beta1_common::geo::ZONES.len());
        for z in zones {
            let texte =
                format!("zone = \"{}\"\nx = [0, 0]\ny = [0, 0]\n", beta1_common::geo::ZONES[z.indice() as usize]);
            let r: Rectangle = toml::from_str(&texte).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(r.zone, z);
        }
    }
}
