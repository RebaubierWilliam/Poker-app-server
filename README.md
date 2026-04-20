# poker-blind-timer

Générateur de structure de blinds pour soirées poker, avec timer mobile.

Projet d'apprentissage : **Rust** (serveur) et **Flutter** (client mobile).

## Objectif

À partir de :
- nombre de joueurs
- durée totale souhaitée du tournoi
- durée de chaque niveau
- dotation en jetons par joueur

...l'application propose une structure de blinds adaptée (progression géométrique, pause au milieu, antes en deuxième moitié), et fournit un timer mobile pour animer la soirée.

## Stack

| Couche      | Techno                                |
|-------------|---------------------------------------|
| Serveur     | Rust + Axum + Tokio                   |
| Client      | Flutter (Android + iOS)               |
| Déploiement | Fly.io (serveur), stores mobiles plus tard |

## Structure du repo

```
poker-blind-timer/
├── server/     # Rust, Axum API
│   └── src/
│       ├── main.rs          # point d'entrée HTTP
│       └── blind_timer.rs   # algorithme de structure
└── client/     # Flutter, app mobile
    └── lib/
        └── main.dart
```

## Lancer le serveur en local

```bash
cd server
cargo run
# puis POST http://localhost:8080/structure avec un JSON (voir plus bas)
```

Test rapide :

```bash
curl -X POST http://localhost:8080/structure \
  -H "Content-Type: application/json" \
  -d '{
    "players": 9,
    "target_duration_minutes": 240,
    "level_duration_minutes": 20,
    "chips_per_player": [
      {"value": 25,   "count": 8},
      {"value": 100,  "count": 10},
      {"value": 500,  "count": 4},
      {"value": 1000, "count": 2}
    ]
  }'
```

Tests unitaires de l'algorithme :

```bash
cd server
cargo test
```

## Lancer le client Flutter

```bash
cd client
flutter pub get
flutter run
```

## Roadmap

### Phase 1 — MVP (en cours)
- [x] Scaffold serveur Rust + client Flutter
- [x] Algorithme v1 de structure de blinds (progression géométrique)
- [x] Endpoint `POST /structure`
- [ ] Écran Flutter : formulaire de paramètres → affichage de la structure
- [ ] Écran Flutter : timer avec niveau courant, temps restant, blinds

### Phase 2 — Soirée réelle
- [ ] Sauvegarde locale des presets (Hive ou SharedPreferences)
- [ ] Notifications sonores à chaque changement de niveau
- [ ] Mode plein écran pour afficher le timer sur grand écran

### Phase 3 — Déploiement
- [ ] Dockerfile du serveur
- [ ] `fly launch` sur Fly.io
- [ ] CI GitHub Actions (cargo test + flutter test)

## Algorithme de structure

Principe :
1. **Stack initial** par joueur = somme de (valeur × quantité) pour chaque dénomination
2. **BB de départ** ≈ stack_initial / 100 (profondeur confortable de 100 BB)
3. **BB finale** ≈ total_chips / 20 (phase push/fold en fin de tournoi)
4. **Progression géométrique** : chaque niveau multiplie la BB par un ratio constant
5. **Arrondi** sur le plus petit jeton disponible pour des valeurs "propres"
6. **Antes** introduites au tiers du tournoi (~10% de la BB)
7. **Pause** de 10 min au milieu

Voir [`server/src/blind_timer.rs`](server/src/blind_timer.rs).
