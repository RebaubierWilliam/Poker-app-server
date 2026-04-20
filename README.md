# poker-blind-timer (serveur)

API Rust qui génère une structure de blinds pour une soirée poker à partir de la composition de la malette et du nombre de joueurs.

Projet d'apprentissage **Rust** (Axum + Tokio).

Le client mobile vit dans un autre repo.

## Objectif

À partir de :
- nombre de joueurs
- durée totale souhaitée du tournoi
- contenu total de la malette (quantité de jetons par valeur faciale)

...l'API calcule :
- la répartition des jetons par joueur (stack de départ)
- une durée de niveau adaptée
- une progression de blinds (géométrique, pause au milieu, antes en deuxième moitié)

## Stack

| Couche      | Techno               |
|-------------|----------------------|
| Serveur     | Rust + Axum + Tokio  |
| Déploiement | Fly.io (prévu)       |

## Lancer le serveur en local

```bash
cargo run
# puis POST http://localhost:8080/structure
```

Test rapide :

```bash
curl -X POST http://localhost:8080/structure \
  -H "Content-Type: application/json" \
  -d '{
    "players": 9,
    "total_duration_minutes": 240,
    "case_chips": [
      {"value": 25,   "count": 100},
      {"value": 100,  "count": 100},
      {"value": 500,  "count": 50},
      {"value": 1000, "count": 25}
    ]
  }'
```

Tests unitaires :

```bash
cargo test
```

## Roadmap

### Phase 1 — MVP
- [x] Algorithme v1 de structure de blinds (progression géométrique)
- [x] Endpoint `POST /structure`
- [x] Répartition automatique de la malette par joueur

### Phase 2 — Déploiement
- [ ] Dockerfile
- [ ] `fly launch` sur Fly.io
- [ ] CI GitHub Actions (`cargo test`)

## Algorithme

Principe :
1. **Répartition** : chaque valeur de la malette est divisée par le nombre de joueurs (division entière)
2. **Stack de départ** = somme des (valeur × quantité par joueur)
3. **BB initiale** ≈ stack / 100 (profondeur 100 BB)
4. **BB finale** ≈ total_chips / 20 (push/fold)
5. **Progression géométrique** entre ces deux bornes
6. **Arrondi** sur le plus petit jeton disponible
7. **Antes** introduites au tiers du tournoi (~10% de la BB)
8. **Pause** de 10 min au milieu

Voir [`src/blind_timer.rs`](src/blind_timer.rs).
