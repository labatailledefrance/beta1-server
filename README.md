# beta1-server

Serveur de simulation de la beta 1 de **La Bataille de France** : Bevy sans rendu, qui fait autorité ; les clients ([beta1-client](https://github.com/labatailledefrance/beta1-client)) affichent et envoient des ordres. Code partagé : [beta1-common](https://github.com/labatailledefrance/beta1-common).

## Lancer

```
cargo run --profile rapide -- --listen 127.0.0.1:7425 --threads 8
cargo run --profile rapide -- --check                          # catalogue et ordre des systèmes, puis sortie
cargo run --release -- --determinism 2000 --threads 1          # hash d'état après 2 000 ticks
cargo run --profile rapide -- --schema schema                  # régénère les JSON Schemas
cargo run --profile rapide -- --mods /chemin --mod base,autre  # autres mods
cargo run --profile rapide -- --tiles /chemin/<publication>    # tiles de la chaîne de données
```

Le dossier des mods vient de `--mods` ou de la variable `MODS` (`/mods` dans l'image, `mods` par défaut). La publication de tiles vient de `--tiles` ou de `TILES` (posée par le chart) ; sans elle, tout est mer : c'est le cas des tests et de la CI. Le scénario se choisit par `--scenario` (`officiel` par défaut).

## Organisation

Ce qui **décrit** le jeu est en données (TOML, dans des mods) ; ce qui le **fait tourner** est en Rust ([E-02](https://github.com/labatailledefrance/documentation/blob/main/OPEN-PROBLEMS.md#e-02--format-des-données), [F-01](https://github.com/labatailledefrance/documentation/blob/main/OPEN-PROBLEMS.md#f-01--moteur), [F-03](https://github.com/labatailledefrance/documentation/blob/main/OPEN-PROBLEMS.md#f-03--modding)).

```
src/
  main.rs          CLI
  app.rs           l'App, les phases et leur ordre : le seul endroit où lire le flux d'un tick
  catalogue/       mods, objets TOML, textes Fluent : validation et compilation
  tiles/           publication, terrain, activation à tick fixé
  zombies/         peuplement des tiles actives, errance, passages de tile
  determinisme.rs  hash d'état
  reseau.rs        WebSocket, handshake, send queue
mods/base/         le contenu de base, un mod comme les autres
schema/            JSON Schemas des fichiers de mods (générés, versionnés)
```

### Plugins et phases

- **Un plugin Bevy par domaine** : il possède ses composants, ses systèmes et ses messages ; il lit les composants des autres, n'écrit que les siens. Les plugins ne s'appellent pas, ils échangent des messages.
- Un tick avance en `FixedFirst`, puis parcourt les **phases** de `FixedUpdate`, toujours dans cet ordre : `Entrees`, `Tiles`, `Perception`, `Decision`, `Mouvement`, `Combat`, `Logistique`, `Nettoyage`, `Sortie`. Chaque système est rangé dans une phase (`.in_set(Phase::Combat)`).
- **Ambiguïté = erreur** : deux systèmes qui touchent la même donnée sans ordre déclaré (`.chain()`, `.before()`, `.after()`) empêchent le démarrage. `--check` construit tous les schedules et le signale, en nommant les deux systèmes ; la CI le lance.
- Chaque plugin se teste seul : `app::app(catalogue, …)`, puis `app::verifier` et `app::avancer_de` (voir `tests/app.rs`).

### Catalogue et mods

```
mods/<id>/mod.toml                identifiant, version (x.y.z), dépendances
mods/<id>/**/<objet>.toml         un objet par fichier ; l'identifiant est le nom du fichier
mods/<id>/locales/fr/*.ftl        textes (Fluent), clé « <genre>-<identifiant> »
```

- Un fichier d'objet contient une seule table, nommée par le **genre** de l'objet (`[zombie]`). Unités humaines (m, m/s, s, degrés) ; les durées tombent sur un tick (multiple de 0,05 s).
- **Validation stricte** : champ inconnu ou manquant, valeur hors bornes, identifiant invalide, doublon, texte absent, Fluent invalide, fichier inattendu, dépendance absente ou cycle : le serveur refuse de démarrer et liste **toutes** les erreurs.
- **Ordre des mods** : les dépendances d'abord, puis l'ordre déclaré par `--mod`. Un mod chargé plus tard **remplace** un objet de même genre et de même identifiant, et un texte de même clé.
- **Compilation** en une ressource `Catalogue` indexée par entiers (unités de simulation : ticks, cosinus). Son hash BLAKE3 (mods, objets, textes) est la graine du hash d'état ; il sera envoyé au client au handshake et inscrit dans les instantanés.
- **Pas de rechargement à chaud** : un catalogue changé, c'est un redémarrage, donc un monde remis à zéro.
- **Éditeur** : les fichiers commencent par `#:schema …/schema/objet.schema.json` (Taplo, Even Better TOML) pour l'autocomplétion et les bornes. Après un changement des types, `--schema schema` ; un test vérifie que `schema/` est à jour.

Genres d'objets : `zombie` (déplacement, perception, alerte, contagion ; valeurs de départ de P-26), `scenario` (état initial : graine, zombies par habitant, type de zombie placé, zones peuplées, tiles actives au démarrage). Une référence vers un autre objet (`type_zombie`) est vérifiée au chargement.

### Tiles

- **Publication** (H-06) : le dossier doit contenir `COMPLET` ; le MANIFESTE donne l'identifiant et les départements. Au démarrage, le serveur lit l'index de chaque paquet et construit un index national tile → (paquet, décalage, taille) : la France entière (790 759 tiles, 4 697 paquets) s'indexe en 11 s sur les disques de srv, 0,2 s quand ils sont en cache. Une tile vue dans deux paquets est refusée.
- **Terrain** : grille de passage de 500 × 500 cases de 2 m (piéton, zombie, bâti, eau ; règles de P-26), occupation du sol, part de bâti. L'empreinte BLAKE3 de la tile est vérifiée. Une tile hors des données est de la mer. L'estran reste à terre tant que les marées ne sont pas simulées (A-11).
- **Activation** : message `ActiverTile` ; lecture et décodage en tâche de fond (`IoTaskPool`), mise en service au tick `demande + DELAI_ACTIVATION` (10 ticks), que le tick attend au besoin. Une tile illisible arrête le serveur plutôt que de simuler un autre monde que celui des données. Les tiles actives sont des entités `TileActive`, rangées par `TileId` dans `TilesActives`.
- Le hash d'état part du hash du catalogue et de l'identifiant de la publication, et couvre les tiles actives.

### Zombies

- Les zombies d'une tile active sont un composant `Zombies` de l'entité de la tile : une liste et une grille d'occupation (3 zombies au plus par case de 2 m), comme dans P-26. Une entité par zombie coûterait plus cher pour rien.
- **Peuplement** à la mise en service d'une tile : habitants × `densite_zombies` du scénario, dans les zones peuplées (la métropole ; l'outre-mer est humain, A-10). Placement par hachage (graine, tile, rang, essai), de préférence à 12 m du bâti, toujours sur une case de terre ouverte aux piétons. Identifiants attribués dans l'ordre des tiles.
- **Errance**, en phase Mouvement : décision en parallèle par tile (chaque zombie ne lit que sa tile), application dans l'ordre des tiles puis des zombies, glissement le long des obstacles, passages vers une tile active après tous les déplacements ; une tile inactive fait bord.
- Brest (16 tiles, profil `rapide`) : 48 761 zombies, 1,9 ms par tick, hash compris.
- À venir avec les humains : perception, relais de l'alerte, champs de poursuite, contacts.

## Règles

- **Déterminisme** : même hash d'état quel que soit le nombre de threads ; la CI le vérifie à 1 et 16 threads. Un calcul en tâche de fond rend son résultat à un tick fixé à son lancement.
- **Réseau** : WebSocket sans TLS (l'ingress Traefik porte TLS et les rate limits par adresse IP) ; send queue bornée par client ; message entrant limité à 4 ko. Le `Welcome` donne la caméra de départ (milieu des tiles du scénario). Le client envoie sa vue (centre, rayon ≤ 1 km) ; à 10 Hz, la simulation publie l'état et un instantané partagé des zombies, et le thread de chaque client en tire les zombies de sa vue, les plus proches d'abord, 1 200 au plus (60 ko/s). Un message invalide après le handshake coupe la connexion.
- **Image** : construite sur `dhi.io/rust`, exécutée sur `dhi.io/static` (glibc), non root, avec le mod `base` dans `/mods` ; publiée sur ghcr.io avec SBOM et provenance, puis signée (cosign). La signature se vérifie au déploiement (`cosign verify`, voir beta1-helm).

Contribution : [CONTRIBUTING.md](https://github.com/labatailledefrance/.github/blob/main/CONTRIBUTING.md). Licence : AGPL-3.0-or-later.
