//! Catalogue : le mod `base`, le JSON Schema, et chaque refus de la validation.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use beta1_server::catalogue::{self, Catalogue, Erreurs};

const ZOMBIE: &str = "[zombie]
vitesse_errance = 0.5
vitesse_course = 3.0
ecart_vitesse = 0.18
facteur_eau = 0.4
periode_cap = 4.0
cone_vision = 120.0
rayon_odorat = 10.0
perception_errance = 1.0
perception_chasse = 0.25
memoire_proie = 60.0
rayon_relais = 2.5
fraicheur_alerte = 3.0
rayon_fumee = 2500.0
duree_attirance = 30.0
rayon_contact = 1.0
delai_zombification = 10.0
";

fn mods_du_depot() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("mods")
}

/// Dossier de mods neuf, sous `target/`.
fn dossier() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "catalogue-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ecrire(racine: &Path, chemin: &str, contenu: &str) {
    let p = racine.join(chemin);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, contenu).unwrap();
}

/// Un mod `m` minimal et valide : un zombie `ordinaire` et son texte.
fn mod_valide(racine: &Path, m: &str, deps: &[&str]) {
    let deps: Vec<String> = deps.iter().map(|d| format!("\"{d}\"")).collect();
    ecrire(
        racine,
        &format!("{m}/mod.toml"),
        &format!("id = \"{m}\"\nversion = \"1.0.0\"\ndependances = [{}]\n", deps.join(", ")),
    );
    ecrire(racine, &format!("{m}/zombies/ordinaire.toml"), ZOMBIE);
    ecrire(racine, &format!("{m}/locales/fr/{m}.ftl"), "zombie-ordinaire = Zombie\n");
}

fn liste(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

fn charger(racine: &Path, ids: &[&str]) -> Result<Catalogue, Erreurs> {
    catalogue::charger(racine, &liste(ids))
}

/// La validation refuse, avec un message qui contient `attendu`.
fn refuse(racine: &Path, ids: &[&str], attendu: &str) {
    match charger(racine, ids) {
        Ok(_) => panic!("accepté, alors qu'on attendait : {attendu}"),
        Err(e) => assert!(e.to_string().contains(attendu), "attendu « {attendu} », obtenu :\n{e}"),
    }
}

#[test]
fn mod_base_valide() {
    let c = charger(&mods_du_depot(), &["base"]).unwrap();
    assert_eq!(c.mods[0].0, "base");
    let z = c.zombie(c.id_zombie("ordinaire").unwrap());
    assert_eq!(z.perception_chasse, 5);
    assert_eq!(z.memoire_proie, 1200);
    assert!((z.cos_demi_cone - 0.5).abs() < 1e-6);
}

#[test]
fn hash_stable_et_sensible() {
    let a = charger(&mods_du_depot(), &["base"]).unwrap();
    let b = charger(&mods_du_depot(), &["base"]).unwrap();
    assert_eq!(a.hash(), b.hash());
    let d = dossier();
    mod_valide(&d, "base", &[]);
    let c = charger(&d, &["base"]).unwrap();
    ecrire(&d, "base/locales/fr/base.ftl", "zombie-ordinaire = Mort-vivant\n");
    let texte_change = charger(&d, &["base"]).unwrap();
    assert_ne!(c.hash(), texte_change.hash(), "un texte change le hash");
}

#[test]
fn schema_a_jour() {
    let dossier = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema");
    for (nom, contenu) in catalogue::schemas() {
        let versionne = std::fs::read_to_string(dossier.join(nom)).unwrap_or_default();
        assert!(versionne == contenu, "schema/{nom} n'est pas à jour : cargo run -- --schema schema");
    }
}

#[test]
fn surcharge_par_un_mod_suivant() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "lent/mod.toml", "id = \"lent\"\nversion = \"0.1.0\"\ndependances = [\"base\"]\n");
    ecrire(&d, "lent/zombies/ordinaire.toml", &ZOMBIE.replace("vitesse_course = 3.0", "vitesse_course = 1.0"));
    // Déclaré avant base, chargé après : la dépendance l'emporte sur l'ordre déclaré.
    let c = charger(&d, &["lent", "base"]).unwrap();
    assert_eq!(c.mods.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["base", "lent"]);
    assert_eq!(c.zombies.len(), 1);
    assert_eq!(c.zombies[0].vitesse_course, 1.0);
}

#[test]
fn champ_inconnu() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/zombies/ordinaire.toml", &format!("{ZOMBIE}vitesse_vol = 2.0\n"));
    refuse(&d, &["base"], "vitesse_vol");
}

#[test]
fn champ_manquant() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/zombies/ordinaire.toml", &ZOMBIE.replace("rayon_contact = 1.0\n", ""));
    refuse(&d, &["base"], "rayon_contact");
}

#[test]
fn genre_inconnu() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/zombies/ordinaire.toml", "[vampire]\nvitesse = 1.0\n");
    refuse(&d, &["base"], "vampire");
}

#[test]
fn hors_bornes() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/zombies/ordinaire.toml", &ZOMBIE.replace("vitesse_course = 3.0", "vitesse_course = 300.0"));
    refuse(&d, &["base"], "vitesse_course = 300 hors des bornes");
}

#[test]
fn duree_hors_tick() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/zombies/ordinaire.toml", &ZOMBIE.replace("periode_cap = 4.0", "periode_cap = 4.01"));
    refuse(&d, &["base"], "ne tombe pas sur un tick");
}

#[test]
fn texte_absent() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/locales/fr/base.ftl", "autre-cle = Autre\n");
    refuse(&d, &["base"], "texte « zombie-ordinaire » absent");
}

#[test]
fn fluent_invalide() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/locales/fr/base.ftl", "zombie-ordinaire = { $\n");
    refuse(&d, &["base"], "Fluent");
}

#[test]
fn identifiant_invalide() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/zombies/Coureur_Rapide.toml", ZOMBIE);
    refuse(&d, &["base"], "identifiant « Coureur_Rapide » invalide");
}

#[test]
fn doublon_dans_un_mod() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/autres/ordinaire.toml", ZOMBIE);
    refuse(&d, &["base"], "déjà défini");
}

#[test]
fn fichier_inattendu() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/zombies/notes.txt", "");
    refuse(&d, &["base"], "fichier inattendu");
}

#[test]
fn dependance_absente() {
    let d = dossier();
    mod_valide(&d, "extra", &["base"]);
    refuse(&d, &["extra"], "dépendance « base » absente");
}

#[test]
fn cycle() {
    let d = dossier();
    mod_valide(&d, "a", &["b"]);
    mod_valide(&d, "b", &["a"]);
    refuse(&d, &["a", "b"], "cycle de dépendances entre : a, b");
}

#[test]
fn id_du_mod_different_du_dossier() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/mod.toml", "id = \"autre\"\nversion = \"1.0.0\"\n");
    refuse(&d, &["base"], "différent du dossier");
}

#[test]
fn mod_introuvable() {
    let d = dossier();
    refuse(&d, &["base"], "lecture impossible");
}

#[test]
fn toutes_les_erreurs_d_un_coup() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/zombies/a.toml", &ZOMBIE.replace("vitesse_course = 3.0", "vitesse_course = 300.0"));
    ecrire(&d, "base/zombies/b.toml", &format!("{ZOMBIE}inconnu = 1\n"));
    let e = charger(&d, &["base"]).unwrap_err();
    // a : borne + texte absent ; b : champ inconnu.
    assert_eq!(e.len(), 3, "{e}");
}

const SCENARIO: &str = "[scenario]
graine = 1
densite_zombies = 1.0
type_zombie = \"ordinaire\"
zones_peuplees = [\"LAMB93\"]
tiles_actives = [{ zone = \"LAMB93\", x = [1, 2], y = [3, 4] }]
";

#[test]
fn scenario_compile() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/scenarios/essai.toml", SCENARIO);
    ecrire(&d, "base/locales/fr/base.ftl", "zombie-ordinaire = Zombie\nscenario-essai = Essai\n");
    let c = charger(&d, &["base"]).unwrap();
    let s = c.scenario("essai").unwrap();
    assert_eq!(s.tiles_actives.len(), 4);
    assert_eq!(s.type_zombie, c.id_zombie("ordinaire").unwrap());
    assert_eq!(s.zones_peuplees, [0]);
}

#[test]
fn scenario_type_de_zombie_inconnu() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/scenarios/essai.toml", &SCENARIO.replace("\"ordinaire\"", "\"coureur\""));
    ecrire(&d, "base/locales/fr/base.ftl", "zombie-ordinaire = Zombie\nscenario-essai = Essai\n");
    refuse(&d, &["base"], "type_zombie « coureur » : aucun zombie de ce nom");
}

#[test]
fn scenario_zone_inconnue() {
    let d = dossier();
    mod_valide(&d, "base", &[]);
    ecrire(&d, "base/scenarios/essai.toml", &SCENARIO.replace("[\"LAMB93\"]", "[\"MARS\"]"));
    ecrire(&d, "base/locales/fr/base.ftl", "zombie-ordinaire = Zombie\nscenario-essai = Essai\n");
    refuse(&d, &["base"], "MARS");
}
