# beta1-server

Serveur de simulation de la beta 1 de **La Bataille de France** : Bevy sans rendu, qui fait autorité ; les clients ([beta1-client](https://github.com/labatailledefrance/beta1-client)) affichent et envoient des ordres. Code partagé : [beta1-common](https://github.com/labatailledefrance/beta1-common).

## Lancer

```
cargo run --profile rapide -- --ecoute 127.0.0.1:7425 --threads 8
cargo run --release -- --determinisme 2000 --threads 1   # empreinte après 2 000 ticks
```

## Règles

- **Déterminisme** : même empreinte quel que soit le nombre de threads ; la CI le vérifie à 1 et 16 threads. Un calcul en tâche de fond rend son résultat à un tick fixé à son lancement.
- **Réseau** : WebSocket sans TLS (l'ingress Traefik porte TLS et les limites par adresse IP) ; file d'envoi bornée par client ; message entrant limité à 4 ko.
- **Image** : construite sur `dhi.io/rust`, exécutée sur `dhi.io/static` (glibc), non root, publiée sur ghcr.io avec SBOM et provenance, puis signée (cosign). Le cluster refuse une image non signée.

Contribution : [CONTRIBUTING.md](https://github.com/labatailledefrance/.github/blob/main/CONTRIBUTING.md). Licence : AGPL-3.0-or-later.
