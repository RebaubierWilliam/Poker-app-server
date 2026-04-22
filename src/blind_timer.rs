use serde::{Deserialize, Serialize};

/// Paramètres d'entrée décrivant le tournoi que l'utilisateur veut organiser.
///
/// L'utilisateur décrit la **malette** (quantité totale de jetons par valeur)
/// et le nombre de joueurs. L'algorithme se charge de:
///   - répartir la malette équitablement entre les joueurs,
///   - choisir une durée de niveau adaptée,
///   - générer la structure de blinds correspondante, calquée sur les
///     structures professionnelles (WSOP/WPT): SB/BB toujours dans un rapport
///     1:2, valeurs rondes prises dans une échelle canonique.
#[derive(Debug, Deserialize, Clone)]
pub struct TournamentInput {
    /// Nombre de joueurs au départ.
    pub players: u32,
    /// Durée cible totale du tournoi, en minutes.
    pub total_duration_minutes: u32,
    /// Contenu total de la malette — valeur faciale => quantité dans la malette.
    pub case_chips: Vec<ChipDenomination>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChipDenomination {
    pub value: u32,
    pub count: u32,
}

/// Un niveau de blind dans la structure générée.
#[derive(Debug, Serialize, Clone)]
pub struct BlindLevel {
    pub level: u32,
    pub small_blind: u32,
    pub big_blind: u32,
    pub ante: u32,
    pub duration_minutes: u32,
    pub is_break: bool,
}

#[derive(Debug, Serialize)]
pub struct TournamentStructure {
    /// Répartition par joueur calculée par l'algorithme (valeur => nb jetons).
    pub chips_per_player: Vec<ChipDenomination>,
    /// Valeur totale du stack de départ de chaque joueur.
    pub starting_stack: u32,
    /// Valeur totale en jeu (starting_stack * players).
    pub total_chips: u32,
    /// Durée de niveau choisie automatiquement, en minutes.
    pub level_duration_minutes: u32,
    pub number_of_levels: u32,
    pub levels: Vec<BlindLevel>,
}

/// Échelle canonique des SB, exprimée en **unités du plus petit jeton**.
/// Pour un jeton minimal = 1, on obtient SB {1,2,3,4,6,8,10,12,15,20...}.
/// Pour un jeton minimal = 25, on obtient SB {25,50,75,100,150,200,250,300,
/// 375,500...}. Dans tous les cas BB = 2*SB.
///
/// L'échelle est calquée sur celles utilisées par WSOP/WPT: progression en
/// 1.25–1.5× entre chaque palier, avec uniquement des valeurs rondes que l'on
/// peut miser avec des jetons standards (1/2/5, 5/10/25, etc.).
const SB_LADDER_UNITS: &[u32] = &[
    1, 2, 3, 4, 6, 8, 10, 12, 15, 20, 25, 30, 40, 50, 60, 75, 100, 125, 150, 200, 250, 300, 400,
    500, 600, 750, 1000, 1250, 1500, 2000, 2500, 3000, 4000, 5000, 6000, 8000, 10000, 12500, 15000,
    20000, 25000, 30000, 40000, 50000,
];

pub fn compute_structure(input: &TournamentInput) -> TournamentStructure {
    // 1. Répartition de la malette entre les joueurs.
    let chips_per_player: Vec<ChipDenomination> = input
        .case_chips
        .iter()
        .map(|c| ChipDenomination {
            value: c.value,
            count: c.count / input.players.max(1),
        })
        .filter(|c| c.count > 0)
        .collect();

    let starting_stack: u32 = chips_per_player.iter().map(|c| c.value * c.count).sum();
    let total_chips = starting_stack * input.players;

    // Plus petite dénomination du stack joueur = unité d'arrondi des blinds.
    // On l'utilise pour mettre à l'échelle l'échelle canonique.
    let unit = chips_per_player
        .iter()
        .map(|c| c.value)
        .min()
        .filter(|&v| v > 0)
        .unwrap_or(1);

    // 2. Durée de niveau + nombre de niveaux visés.
    let level_duration_minutes = pick_level_duration(input.total_duration_minutes);
    let target_levels = (input.total_duration_minutes / level_duration_minutes).max(1);

    // 3. Cible SB de départ ≈ stack/200 (profondeur ~100 BB, classique).
    let start_sb_target = (starting_stack.max(2) / 200).max(unit);
    let start_idx = find_ladder_index(start_sb_target, unit);

    // 4. Cible SB de fin: au dernier niveau, la table finale à ~3 joueurs doit
    // avoir un M-ratio ≈ 5 BB. total/3 / 5 BB => BB ≈ total/15 => SB ≈ total/30.
    let end_sb_target = (total_chips / 30).max(start_sb_target.saturating_mul(4));
    let end_idx_min = find_ladder_index(end_sb_target, unit);
    // Garantir une progression monotone stricte sur tous les niveaux demandés.
    let min_span_end = start_idx + (target_levels.saturating_sub(1) as usize);
    let end_idx = end_idx_min
        .max(min_span_end)
        .min(SB_LADDER_UNITS.len() - 1);

    // Si on dépasse le sommet de l'échelle, on tronque le nombre de niveaux.
    let number_of_levels = target_levels.min((end_idx - start_idx + 1) as u32);

    // 5. Construction des niveaux via parcours linéaire de l'échelle.
    let ante_starts_at = (number_of_levels / 3).max(1);
    let break_after = if number_of_levels >= 6 {
        Some(number_of_levels / 2)
    } else {
        None
    };
    let break_minutes = break_duration(input.total_duration_minutes);

    let mut levels = Vec::with_capacity(number_of_levels as usize + 1);
    for i in 0..number_of_levels {
        let progress = if number_of_levels > 1 {
            i as f64 / (number_of_levels - 1) as f64
        } else {
            0.0
        };
        let idx_f = start_idx as f64 + (end_idx as f64 - start_idx as f64) * progress;
        let idx = (idx_f.round() as usize).min(SB_LADDER_UNITS.len() - 1);

        let sb = SB_LADDER_UNITS[idx] * unit;
        let bb = sb * 2;

        // Ante ≈ BB/4 à partir du tiers du tournoi, arrondie à l'unité.
        let ante = if i + 1 >= ante_starts_at {
            let raw = bb / 4;
            let rounded = ((raw as f64 / unit as f64).round() as u32) * unit;
            rounded.max(unit)
        } else {
            0
        };

        levels.push(BlindLevel {
            level: i + 1,
            small_blind: sb,
            big_blind: bb,
            ante,
            duration_minutes: level_duration_minutes,
            is_break: false,
        });

        if Some(i + 1) == break_after {
            levels.push(BlindLevel {
                level: 0,
                small_blind: 0,
                big_blind: 0,
                ante: 0,
                duration_minutes: break_minutes,
                is_break: true,
            });
        }
    }

    TournamentStructure {
        chips_per_player,
        starting_stack,
        total_chips,
        level_duration_minutes,
        number_of_levels,
        levels,
    }
}

/// Choisit la durée d'un niveau en visant ~6–12 niveaux selon la durée totale.
///
/// - Très court (≤5 min) → 1 min : même 5 min donne 5 niveaux
/// - Court (6–20 min) → 2–3 min
/// - Moyen (21–150 min) → 5–12 min
/// - Long (>150 min) → 15–30 min
fn pick_level_duration(total_minutes: u32) -> u32 {
    match total_minutes {
        0..=5 => 1,
        6..=10 => 2,
        11..=20 => 3,
        21..=40 => 5,
        41..=80 => 8,
        81..=150 => 12,
        151..=300 => 20,
        301..=480 => 25,
        _ => 30,
    }
}

/// Durée de pause en fonction de la durée totale du tournoi.
fn break_duration(total_minutes: u32) -> u32 {
    match total_minutes {
        0..=29 => 0,
        30..=60 => 3,
        61..=120 => 5,
        121..=240 => 10,
        _ => 15,
    }
}

/// Trouve dans [`SB_LADDER_UNITS`] l'index dont la valeur × `unit` est la plus
/// proche de `target_sb`.
fn find_ladder_index(target_sb: u32, unit: u32) -> usize {
    let target_units = (target_sb as f64 / unit.max(1) as f64).max(1.0);
    SB_LADDER_UNITS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let da = (**a as f64 - target_units).abs();
            let db = (**b as f64 - target_units).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Malette typique 500 jetons pour 9 joueurs.
    fn standard_input() -> TournamentInput {
        TournamentInput {
            players: 9,
            total_duration_minutes: 240,
            case_chips: vec![
                ChipDenomination { value: 25, count: 100 },
                ChipDenomination { value: 100, count: 100 },
                ChipDenomination { value: 500, count: 50 },
                ChipDenomination { value: 1000, count: 25 },
            ],
        }
    }

    /// Malette du serveur de prod (williou malette), 5 joueurs.
    fn server_malette_5p(total_min: u32) -> TournamentInput {
        TournamentInput {
            players: 5,
            total_duration_minutes: total_min,
            case_chips: vec![
                ChipDenomination { value: 1, count: 150 },
                ChipDenomination { value: 5, count: 150 },
                ChipDenomination { value: 10, count: 50 },
                ChipDenomination { value: 25, count: 25 },
                ChipDenomination { value: 100, count: 50 },
                ChipDenomination { value: 500, count: 50 },
                ChipDenomination { value: 1000, count: 25 },
            ],
        }
    }

    #[test]
    fn chips_are_distributed_per_player() {
        let s = compute_structure(&standard_input());
        // 100/9=11, 100/9=11, 50/9=5, 25/9=2 → 11*25 + 11*100 + 5*500 + 2*1000 = 5875
        assert_eq!(s.starting_stack, 5875);
        assert_eq!(s.total_chips, 5875 * 9);
        assert_eq!(s.chips_per_player.len(), 4);
    }

    #[test]
    fn blinds_are_monotonic() {
        let s = compute_structure(&standard_input());
        let playing: Vec<_> = s.levels.iter().filter(|l| !l.is_break).collect();
        for pair in playing.windows(2) {
            assert!(pair[1].big_blind >= pair[0].big_blind);
        }
    }

    #[test]
    fn break_is_inserted_for_long_tournaments() {
        let s = compute_structure(&standard_input());
        assert!(s.levels.iter().any(|l| l.is_break));
    }

    #[test]
    fn level_duration_is_picked_reasonably() {
        // 240 min → 20 min par niveau (12 niveaux).
        let s = compute_structure(&standard_input());
        assert_eq!(s.level_duration_minutes, 20);
    }

    #[test]
    fn drops_denominations_with_zero_per_player() {
        let input = TournamentInput {
            players: 3,
            total_duration_minutes: 120,
            case_chips: vec![
                ChipDenomination { value: 25, count: 30 },
                ChipDenomination { value: 1000, count: 2 },
            ],
        };
        let s = compute_structure(&input);
        assert_eq!(s.chips_per_player.len(), 1);
        assert_eq!(s.chips_per_player[0].value, 25);
        assert_eq!(s.chips_per_player[0].count, 10);
    }

    #[test]
    fn bb_is_always_double_sb() {
        let s = compute_structure(&server_malette_5p(60));
        for lvl in s.levels.iter().filter(|l| !l.is_break) {
            assert_eq!(lvl.big_blind, lvl.small_blind * 2, "level {}", lvl.level);
        }
    }

    #[test]
    fn blinds_use_canonical_round_values() {
        let s = compute_structure(&server_malette_5p(60));
        let unit = 1; // plus petit jeton de la malette
        for lvl in s.levels.iter().filter(|l| !l.is_break) {
            let sb_in_units = lvl.small_blind / unit;
            assert!(
                SB_LADDER_UNITS.contains(&sb_in_units),
                "SB {} ({} units) hors échelle canonique",
                lvl.small_blind,
                sb_in_units
            );
        }
    }

    #[test]
    fn five_minute_game_has_multiple_levels() {
        let s = compute_structure(&server_malette_5p(5));
        let playing = s.levels.iter().filter(|l| !l.is_break).count();
        assert!(
            playing >= 4,
            "5-min game devrait avoir au moins 4 niveaux, a {}",
            playing
        );
        assert_eq!(s.level_duration_minutes, 1);
    }

    #[test]
    fn short_game_has_no_break() {
        let s = compute_structure(&server_malette_5p(10));
        assert!(!s.levels.iter().any(|l| l.is_break));
    }

    #[test]
    fn first_blind_matches_smallest_chip_scale() {
        // Malette avec jeton minimal à 1 → SB initiale multiple de 1, démarre petit.
        let s = compute_structure(&server_malette_5p(60));
        let first = s.levels.iter().find(|l| !l.is_break).unwrap();
        let ratio = first.big_blind as f64 / s.starting_stack as f64;
        assert!(
            ratio >= 0.005 && ratio <= 0.03,
            "1ère BB ({}) hors ratio par rapport au stack ({}): {:.3}",
            first.big_blind,
            s.starting_stack,
            ratio
        );
    }

    #[test]
    fn antes_kick_in_mid_tournament() {
        let s = compute_structure(&standard_input());
        let playing: Vec<_> = s.levels.iter().filter(|l| !l.is_break).collect();
        assert_eq!(playing.first().unwrap().ante, 0);
        assert!(playing.last().unwrap().ante > 0);
    }

    /// Aperçu visuel: `cargo test preview_output -- --nocapture`.
    #[test]
    fn preview_output() {
        for (players, total) in [(5, 5), (5, 30), (5, 60), (5, 120), (9, 240)] {
            let input = TournamentInput {
                players,
                total_duration_minutes: total,
                case_chips: server_malette_5p(0).case_chips,
            };
            let s = compute_structure(&input);
            eprintln!(
                "\n=== {} joueurs / {} min — stack={} total={} lvl={}'x{} ===",
                players,
                total,
                s.starting_stack,
                s.total_chips,
                s.level_duration_minutes,
                s.number_of_levels
            );
            for l in &s.levels {
                if l.is_break {
                    eprintln!("  -- BREAK {} min --", l.duration_minutes);
                } else {
                    eprintln!(
                        "  L{:>2}  SB {:>5}  BB {:>5}  ante {:>4}  ({} min)",
                        l.level, l.small_blind, l.big_blind, l.ante, l.duration_minutes
                    );
                }
            }
        }
    }
}
